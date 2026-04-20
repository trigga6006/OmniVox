use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::asr::engine::AsrEngine;
use crate::error::ErrorCode;
use crate::llm::schema::SlotExtraction;
use crate::llm::template::render_markdown;
use crate::postprocess::processor::TextProcessor;
use crate::state::AppState;

/// Payload emitted on `structured-output-ready` so the overlay can render the
/// panel and offer Paste / Copy / Edit / Dismiss actions.
#[derive(Clone, serde::Serialize)]
struct StructuredOutputPayload {
    markdown: String,
    slots: SlotExtraction,
    raw_transcript: String,
}

/// Payload emitted with `recording-state-change` when the state is "error".
#[derive(Clone, serde::Serialize)]
struct ErrorPayload {
    state: &'static str,
    code: ErrorCode,
    message: String,
}

/// Emit a typed error event so the frontend can show specific guidance.
fn emit_error(app_handle: &tauri::AppHandle, code: ErrorCode, message: impl Into<String>) {
    let payload = ErrorPayload {
        state: "error",
        code,
        message: message.into(),
    };
    let _ = app_handle.emit("recording-error", &payload);
    let _ = app_handle.emit("recording-state-change", "error");
}

/// Snapshot the currently focused window so we can restore it before pasting.
/// Returns a platform-specific handle (HWND on Windows, pid on macOS).
#[cfg(target_os = "windows")]
fn capture_foreground_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() { None } else { Some(hwnd as isize) }
}

#[cfg(target_os = "macos")]
fn capture_foreground_window() -> Option<isize> {
    // Use NSWorkspace via the objc runtime to get the frontmost app's PID.
    // This is a direct Cocoa call — no subprocess overhead.
    unsafe {
        let cls = objc::runtime::Class::get("NSWorkspace")?;
        let workspace: *mut objc::runtime::Object = objc::msg_send![cls, sharedWorkspace];
        let app: *mut objc::runtime::Object = objc::msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = objc::msg_send![app, processIdentifier];
        if pid > 0 { Some(pid as isize) } else { None }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn capture_foreground_window() -> Option<isize> {
    None
}

/// Extract the process executable name (e.g. "Code.exe") from a window handle.
#[cfg(target_os = "windows")]
fn get_process_name_from_hwnd(hwnd: isize) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd as *mut std::ffi::c_void, &mut pid as *mut u32);
        if pid == 0 {
            return None;
        }

        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }

        let mut buf = [0u16; 260]; // MAX_PATH
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);

        if ok == 0 || len == 0 {
            return None;
        }

        let path = String::from_utf16_lossy(&buf[..len as usize]);
        // Extract just the filename: "C:\...\Code.exe" -> "Code.exe"
        path.rsplit('\\').next().map(|s| s.to_string())
    }
}

#[cfg(target_os = "macos")]
fn get_process_name_from_hwnd(pid: isize) -> Option<String> {
    // Use proc_pidpath to get the executable path directly — no subprocess overhead.
    let mut buf = [0u8; 4096]; // PROC_PIDPATHINFO_MAXSIZE
    let ret = unsafe {
        libc::proc_pidpath(pid as i32, buf.as_mut_ptr() as *mut _, buf.len() as u32)
    };
    if ret <= 0 {
        return None;
    }
    let path = std::str::from_utf8(&buf[..ret as usize]).ok()?;
    path.rsplit('/').next().map(|s| s.to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn get_process_name_from_hwnd(_hwnd: isize) -> Option<String> {
    None
}

/// Restore focus to the window that was active before recording.
///
/// Only calls `SetForegroundWindow` when the target window is NOT already in
/// the foreground.  Calling it redundantly can trigger `WM_SETFOCUS` handlers
/// that select-all text in some input controls, which causes a subsequent paste
/// to erase existing content.
///
/// When focus restoration IS needed (e.g. the overlay stole focus), we
/// additionally collapse any accidental text selection so the paste inserts
/// rather than replaces.
#[cfg(target_os = "windows")]
fn restore_foreground_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, SetForegroundWindow,
    };

    let current = unsafe { GetForegroundWindow() };
    if !current.is_null() && current as isize == hwnd {
        // Already the foreground window — skip to avoid triggering focus handlers.
        return;
    }

    unsafe {
        SetForegroundWindow(hwnd as *mut std::ffi::c_void);
    }
    // Give the OS time to process the focus switch and any WM_SETFOCUS handlers.
    // 50 ms is sufficient — Windows processes SetForegroundWindow in under 20 ms.
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Collapse any accidental text selection caused by the focus change.
    deselect_after_focus_restore(hwnd);
}

/// After a focus restoration, some controls select all their text.  If a text
/// caret is active, send Right→Left arrow keys to collapse the selection without
/// net cursor movement (when nothing was selected the two keys cancel out).
/// Only fires when a text caret is detected — non-text controls are left alone.
#[cfg(target_os = "windows")]
fn deselect_after_focus_restore(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    unsafe {
        let thread_id = GetWindowThreadProcessId(hwnd as *mut std::ffi::c_void, std::ptr::null_mut());
        if thread_id == 0 {
            return;
        }

        let mut gui: GUITHREADINFO = std::mem::zeroed();
        gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

        if GetGUIThreadInfo(thread_id, &mut gui) == 0 {
            return;
        }

        // Only deselect if a text caret is present (i.e. a text field is focused).
        if gui.hwndCaret.is_null() {
            return;
        }
    }

    // Right collapses any selection to its end; Left steps back one position.
    // Net effect when nothing is selected: zero movement.
    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.key(Key::RightArrow, Direction::Click);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = enigo.key(Key::LeftArrow, Direction::Click);
    }
}

#[cfg(target_os = "macos")]
fn restore_foreground_window(pid: isize) {
    // Use NSRunningApplication to activate by PID — no subprocess overhead.
    unsafe {
        let cls = objc::runtime::Class::get("NSRunningApplication")
            .expect("NSRunningApplication class");
        let app: *mut objc::runtime::Object = objc::msg_send![
            cls,
            runningApplicationWithProcessIdentifier: pid as i32
        ];
        if !app.is_null() {
            // NSApplicationActivateIgnoringOtherApps = 1 << 1
            let _: objc::runtime::BOOL = objc::msg_send![
                app,
                activateWithOptions: 0x02u64
            ];
        }
    }
    // Give the OS time to process the focus switch.
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn restore_foreground_window(_hwnd: isize) {}

/// Public wrapper for commands that need to restore focus (e.g. the Structured
/// panel's Paste button).  Keeps the internal helper private while allowing
/// reuse.
pub fn restore_foreground_window_public(hwnd: isize) {
    restore_foreground_window(hwnd);
}

/// Toggle recording on/off. Called by the frontend start/stop commands.
pub async fn toggle_recording(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    let is_recording = match state.audio.lock() {
        Ok(audio) => audio.is_recording(),
        Err(_) => {
            emit_error(app_handle, ErrorCode::InternalError, "Audio state lock poisoned");
            return;
        }
    };

    if !is_recording {
        start_recording(app_handle, &state);
    } else {
        stop_and_transcribe(app_handle, &state).await;
    }
}

/// Start recording only if not already recording (used by hotkey hold/double-tap).
pub async fn start_if_idle(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let is_recording = state.audio.lock().map(|a| a.is_recording()).unwrap_or(false);
    if !is_recording {
        start_recording(app_handle, &state);
    }
}

/// Stop recording only if currently recording (used by hotkey release/toggle-off).
pub async fn stop_if_recording(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();
    let is_recording = state.audio.lock().map(|a| a.is_recording()).unwrap_or(false);
    if is_recording {
        stop_and_transcribe(app_handle, &state).await;
    }
}

/// Begin microphone capture.
pub fn start_recording(app_handle: &tauri::AppHandle, state: &AppState) {
    // Snapshot the foreground window BEFORE we do anything that might steal focus.
    let fg = capture_foreground_window();
    if let Ok(mut prev) = state.prev_foreground.lock() {
        *prev = fg;
    }

    // Load settings once — used for auto-switch and audio ducking below.
    let settings = crate::storage::settings::get_settings(&state.db).ok();

    // Auto-switch context mode based on the foreground application.
    if let Some(hwnd) = fg {
        let auto_switch = settings.as_ref().map(|s| s.auto_switch_modes).unwrap_or(false);

        if auto_switch {
            if let Some(process_name) = get_process_name_from_hwnd(hwnd) {
                // Find the target mode: either a bound mode or General fallback.
                let target_mode_id = match crate::storage::app_bindings::find_mode_for_process(
                    &state.db,
                    &process_name,
                ) {
                    Ok(Some(id)) => Some(id),
                    _ => {
                        // No binding for this app — fall back to the builtin General mode
                        crate::storage::context_modes::get_general_mode_id(&state.db).ok()
                    }
                };

                if let Some(target_mode_id) = target_mode_id {
                    let current_mode = state.active_context_mode_id.lock().unwrap().clone();
                    if current_mode.as_deref() != Some(&target_mode_id) {
                        if let Err(e) = crate::commands::context_modes::activate_mode_internal(
                            state,
                            &target_mode_id,
                        ) {
                            eprintln!("Auto-switch mode failed: {e}");
                        } else if let Ok(mode) =
                            crate::storage::context_modes::get_mode(&state.db, &target_mode_id)
                        {
                            let _ = app_handle.emit(
                                "context-mode-changed",
                                serde_json::json!({
                                    "id": mode.id.to_string(),
                                    "name": mode.name,
                                    "icon": mode.icon,
                                    "color": mode.color,
                                }),
                            );
                        }
                    }
                }
            }
        }
    }

    // Duck system volume so other audio doesn't compete with the mic.
    if settings.as_ref().map(|s| s.audio_ducking).unwrap_or(true) {
        // Convert ducking_amount (0–100, % reduction) to a volume factor.
        // 70 → keep 30% of volume (factor 0.30), 100 → mute (factor 0.0).
        let amount = settings.as_ref().map(|s| s.ducking_amount).unwrap_or(70);
        let factor = 1.0 - (amount.min(100) as f32 / 100.0);
        crate::audio::ducking::duck(Some(factor));
    }

    let mut audio = match state.audio.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            // Recover from a poisoned mutex — the previous holder panicked,
            // but the AudioCapture inside is still usable after a reset.
            let mut guard = poisoned.into_inner();
            guard.cancel(); // Reset to clean state
            guard
        }
    };

    if let Err(e) = audio.start() {
        eprintln!("Failed to start recording: {e}");
        emit_error(app_handle, e.code(), format!("Failed to start recording: {e}"));
        return;
    }

    // Grab Arc handles for the audio level emitter before dropping the lock
    let is_recording = audio.is_recording_flag();
    let rms_level = audio.rms_level_ref();
    drop(audio);

    let _ = app_handle.emit("recording-state-change", "recording");

    // Spawn a periodic task that emits audio-level events to the frontend.
    // 150 ms strikes a balance between smooth VU meter animation and CPU usage.
    // (100 ms was too aggressive for low-end laptops — 10 events/s of React
    // re-renders + CSS transitions caused pill jank on integrated GPUs.)
    let handle = app_handle.clone();
    let is_rec_clone = is_recording.clone();
    tauri::async_runtime::spawn(async move {
        use std::sync::atomic::Ordering;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if !is_rec_clone.load(Ordering::Relaxed) {
                break;
            }
            let level = f32::from_bits(rms_level.load(Ordering::Relaxed));
            let _ = handle.emit("audio-level", level);
        }
    });

    // Spawn live preview task — periodically transcribes the last 5s of
    // audio and emits partial results to the overlay pill.
    //
    // Architecture: a dedicated std::thread owns a single `WhisperState` for
    // the entire preview session.  An async task periodically snapshots
    // audio and forwards it to the worker over a capacity-1 sync channel —
    // which naturally preserves the "at most one inference in flight"
    // invariant (if the worker is busy, try_send fails and we drop this
    // frame rather than queueing it).
    //
    // Win over the old design: the old code called `engine.transcribe_preview`
    // inside `spawn_blocking` every iteration, and each call did
    // `ctx.create_state()` — allocating ~500 MB of decode buffers that got
    // freed seconds later.  On 16 GB machines the churn caused visible
    // pauses and peak memory spikes.  Now the state is allocated ONCE at
    // recording start and reused across every preview tick until recording
    // ends.
    let live_preview = settings.as_ref().map(|s| s.live_preview).unwrap_or(false);

    if live_preview {
        let engine_opt: Option<Arc<crate::asr::engine::WhisperEngine>> = state
            .engine
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(Arc::clone));

        if let Some(engine) = engine_opt {
            // Capacity-1 sync channel: if worker is busy when sender tries
            // to send, try_send fails fast and we skip this round.
            let (tx_audio, rx_audio) = std::sync::mpsc::sync_channel::<Vec<f32>>(1);

            // Worker thread — owns the WhisperState for the duration of
            // this recording session.  Exits when the async task drops its
            // sender (rx.recv returns Err).
            let worker_handle = app_handle.clone();
            let worker_engine = engine.clone();
            let worker_is_rec = is_recording.clone();
            let _ = std::thread::Builder::new()
                .name("omnivox-preview".into())
                .spawn(move || {
                    use std::sync::atomic::Ordering;
                    let mut state = match worker_engine.create_preview_state() {
                        Ok(s) => s,
                        Err(e) => {
                            eprintln!("Preview: create_state failed: {e}");
                            return;
                        }
                    };
                    while let Ok(audio) = rx_audio.recv() {
                        // Cancellation check — if user stopped recording
                        // between send and receive, skip inference.
                        if !worker_is_rec.load(Ordering::Relaxed) {
                            break;
                        }
                        match worker_engine.transcribe_preview_with_state(&mut state, &audio) {
                            Ok(text) if !text.is_empty() => {
                                let _ = worker_handle.emit("transcription-preview", &text);
                            }
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("Preview inference failed: {e}");
                            }
                        }
                    }
                    // state drops here, freeing decode buffers.
                });

            // Async snapshot task — samples audio every 3 s, forwards to
            // worker.  When recording stops it returns, dropping tx_audio
            // and cleanly terminating the worker thread.
            let ctrl_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                use std::sync::atomic::Ordering;
                const PREVIEW_SAMPLES: usize = 16_000 * 5;

                tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                loop {
                    if !is_recording.load(Ordering::Relaxed) {
                        break;
                    }

                    let samples = {
                        let st: tauri::State<'_, AppState> = ctrl_handle.state();
                        let audio = match st.audio.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        audio.snapshot_tail(PREVIEW_SAMPLES)
                    };

                    if samples.len() >= 8_000 {
                        let mut preview_samples = samples;
                        crate::audio::normalize::normalize_peak(&mut preview_samples);
                        // try_send drops this frame if the worker is still
                        // processing the previous one — backpressure without
                        // queueing.  Err(Disconnected) means worker died; exit.
                        use std::sync::mpsc::TrySendError;
                        match tx_audio.try_send(preview_samples) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => { /* worker busy, skip */ }
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
                // Drop tx_audio → worker rx.recv errors → worker exits.
            });
        }
    }
}

/// Stop capture, run Whisper inference, post-process, and output the text.
pub async fn stop_and_transcribe(app_handle: &tauri::AppHandle, state: &AppState) {
    // Restore system volume immediately — don't wait for transcription.
    crate::audio::ducking::unduck();

    let _ = app_handle.emit("recording-state-change", "processing");

    // Snapshot every setting the rest of this function needs in ONE DB read.
    // Previously this was 3 separate get_settings() calls (noise_reduction,
    // voice_commands/command_send, ship_mode) — each a full table scan and
    // HashMap build.  Cache once, reuse everywhere.
    let settings = crate::storage::settings::get_settings(&state.db).ok();

    // 1. Stop capture and get raw audio samples
    let samples = {
        let mut audio = match state.audio.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match audio.stop() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to stop recording: {e}");
                emit_error(app_handle, e.code(), format!("Failed to stop recording: {e}"));
                return;
            }
        }
    };

    if samples.is_empty() {
        let _ = app_handle.emit("recording-state-change", "idle");
        return;
    }

    // 1a. Give the live preview task time to notice is_recording=false and exit.
    //     The preview checks the flag before and after inference. A short yield
    //     lets it release its WhisperState (and ~500 MB of decode buffers) before
    //     the final transcription allocates its own. Critical on 16 GB machines.
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    // 1b. Conditionally denoise audio with RNNoise before Whisper.
    let mut samples = samples;
    let noise_reduction = settings.as_ref().map(|s| s.noise_reduction).unwrap_or(false);
    if noise_reduction {
        crate::audio::denoise::denoise(&mut samples);
    }

    // 1c. Normalize audio levels for consistent Whisper performance.
    //     Done here (not in the capture callback) to avoid affecting the VU meter.
    crate::audio::normalize::normalize_peak(&mut samples);

    // 2. Transcribe — CPU-bound, runs on a blocking thread to keep the async
    //    runtime free for UI events during inference.
    let engine: Arc<crate::asr::engine::WhisperEngine> = {
        let guard = match state.engine.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.as_ref().map(Arc::clone) {
            Some(e) => e,
            None => {
                drop(guard);
                eprintln!("No model loaded — cannot transcribe");
                emit_error(app_handle, ErrorCode::NoModelLoaded, "No model loaded — go to Models to download one");
                return;
            }
        }
    };

    let transcription = match tokio::task::spawn_blocking(move || engine.transcribe(&samples)).await
    {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            eprintln!("Transcription failed: {e}");
            emit_error(app_handle, ErrorCode::TranscriptionFailed, format!("Transcription failed: {e}"));
            return;
        }
        Err(e) => {
            eprintln!("Transcription task panicked: {e}");
            emit_error(app_handle, ErrorCode::TranscriptionPanicked, format!("Transcription crashed: {e}"));
            return;
        }
    };

    if transcription.text.is_empty() {
        let _ = app_handle.emit("recording-state-change", "idle");
        return;
    }

    // 3. Post-process (dictionary replacements, capitalization, etc.)
    let processed_text = {
        let processor = match state.processor.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match processor.process(&transcription.text) {
            Ok(processed) => processed.processed,
            Err(_) => transcription.text.clone(),
        }
    };

    // 3b. Structured Mode branch.
    //
    // When the user has Structured Mode enabled, we divert to the local LLM
    // to produce a slot-filled Markdown prompt instead of running the
    // deterministic list formatter and voice-command parser.  Failure modes
    // (no runner loaded, timeout, malformed JSON) degrade gracefully to the
    // plain path — structured mode must never block dictation.
    let structured_enabled = settings.as_ref().map(|s| s.structured_mode).unwrap_or(false);
    let min_chars = settings
        .as_ref()
        .map(|s| s.structured_min_chars)
        .unwrap_or(40) as usize;
    let llm_timeout = settings.as_ref().map(|s| s.llm_timeout_secs).unwrap_or(8);

    let configured_llm_id = settings
        .as_ref()
        .and_then(|s| s.active_llm_model_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| {
            state
                .active_llm_model_id
                .lock()
                .ok()
                .and_then(|g| g.clone())
        })
        .or_else(|| crate::commands::llm::preferred_downloaded_llm_id(state));

    let runner_opt = if structured_enabled {
        let existing = state.llm_runner.lock().ok().and_then(|g| g.clone());
        if existing.is_some() {
            crate::llm::diaglog::log("runner: using existing loaded runner");
            existing
        } else if let Some(model_id) = configured_llm_id.clone() {
            crate::llm::diaglog::log(&format!("runner: lazy-loading '{model_id}'"));
            match crate::commands::llm::load_and_activate_llm(&model_id, state) {
                Ok(()) => {
                    crate::llm::diaglog::log("runner: lazy-load ok");
                    state.llm_runner.lock().ok().and_then(|g| g.clone())
                }
                Err(e) => {
                    crate::llm::diaglog::log(&format!("runner: lazy-load FAILED: {e}"));
                    let _ = app_handle.emit("structured-mode-degraded", &format!("Load failed: {e}"));
                    None
                }
            }
        } else {
            crate::llm::diaglog::log("runner: structured_mode=true but no configured model_id");
            None
        }
    } else {
        None
    };

    const STRUCTURED_INPUT_CHAR_CAP: usize = 1600;
    let structured_input = if processed_text.chars().count() > STRUCTURED_INPUT_CHAR_CAP {
        let clipped: String = processed_text.chars().take(STRUCTURED_INPUT_CHAR_CAP).collect();
        crate::llm::diaglog::log(&format!(
            "pipeline: truncating structured input from {} to {} chars",
            processed_text.chars().count(),
            STRUCTURED_INPUT_CHAR_CAP
        ));
        clipped
    } else {
        processed_text.clone()
    };

    let structured: Option<(String, SlotExtraction)> =
        if structured_enabled && processed_text.chars().count() >= min_chars {
            if let Some(runner) = runner_opt {
                let _ = app_handle.emit("recording-state-change", "structuring");
                let t0 = std::time::Instant::now();
                crate::llm::diaglog::log(&format!(
                    "pipeline: starting extraction input_chars={} llm_input_chars={} timeout={}s min_chars={}",
                    processed_text.chars().count(),
                    structured_input.chars().count(),
                    llm_timeout,
                    min_chars
                ));
                match runner
                    .extract_with_timeout(
                        structured_input.clone(),
                        Duration::from_secs(llm_timeout as u64),
                    )
                    .await
                {
                    Ok(slots) => {
                        crate::llm::diaglog::log(&format!(
                            "pipeline: extraction OK in {}ms slots={:?}",
                            t0.elapsed().as_millis(),
                            slots
                        ));
                        let md = render_markdown(&slots);
                        Some((md, slots))
                    }
                    Err(e) => {
                        crate::llm::diaglog::log(&format!(
                            "pipeline: extraction FAILED after {}ms: {e}",
                            t0.elapsed().as_millis()
                        ));
                        let _ = app_handle.emit(
                            "structured-mode-degraded",
                            &format!("Extraction failed: {e}"),
                        );
                        None
                    }
                }
            } else {
                let _ = app_handle.emit(
                    "structured-mode-degraded",
                    "No LLM model available for Structured Mode. Using plain dictation.",
                );
                None
            }
        } else if structured_enabled {
            crate::llm::diaglog::log(&format!(
                "pipeline: SKIPPED (input too short {} < {} chars)",
                processed_text.chars().count(),
                min_chars
            ));
            let _ = app_handle.emit(
                "structured-mode-degraded",
                &format!(
                    "Dictation too short ({} chars) — need at least {}. Using plain output.",
                    processed_text.chars().count(),
                    min_chars
                ),
            );
            None
        } else {
            None
        };

    // 4. Apply deterministic list formatting (bullet lists for enumerated
    //     items).  Structural formatting is handled here at zero cost.
    //     When Structured Mode is active the LLM is the sole formatter —
    //     skip list formatting so we don't double-handle.
    let final_text = if let Some((md, _)) = &structured {
        md.clone()
    } else {
        crate::postprocess::formatter::format_lists(&processed_text)
    };

    // 4b. Voice command detection (if enabled).
    //     Splits text into [Text | Command] segments so the output router can
    //     type text and execute keystrokes (Shift+Enter, Ctrl+Backspace, etc.).
    //     Disabled while Structured Mode is active — the LLM already decided
    //     on the output shape and voice commands would break it.
    let voice_commands_enabled = settings.as_ref().map(|s| s.voice_commands).unwrap_or(false);
    let command_send_enabled = settings.as_ref().map(|s| s.command_send).unwrap_or(true);
    let voice_segments = if voice_commands_enabled && structured.is_none() {
        Some(crate::postprocess::voice_commands::parse_commands_with_options(&final_text, command_send_enabled))
    } else {
        None
    };

    // 5. Kick off focus restoration in parallel with output.
    //     Skipped for Structured Mode since the panel handles pasting.
    let prev_hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
    let focus_task = if structured.is_none() {
        prev_hwnd.map(|hwnd| tokio::task::spawn_blocking(move || restore_foreground_window(hwnd)))
    } else {
        None
    };

    // Wait for focus restoration to complete before outputting text.
    if let Some(task) = focus_task {
        let _ = task.await;
    }

    // 6. Output to the focused application.
    //     When Structured Mode produced a result we skip auto-paste entirely —
    //     the Structured panel becomes the commit point (Paste / Copy / Edit /
    //     Dismiss).  The Markdown still reaches the UI via
    //     `structured-output-ready`, and history still records it.
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if structured.is_none() {
        let output_result = if let Some(ref segments) = voice_segments {
            state.output.send_segments(segments, &output_config)
        } else {
            state.output.send(&final_text, &output_config)
        };
        if let Err(e) = output_result {
            eprintln!("Output failed: {e}");
            emit_error(app_handle, e.code(), format!("Output failed: {e}"));
        }
    }

    // 6b. Ship Mode — automatically press Enter to send the message.
    //     Only fires when type simulation was used (clipboard-only can't auto-send).
    //     When Command Send is enabled it overrides Ship Mode — the user controls
    //     sending by saying "send" at the end, so we skip the automatic Enter.
    //     Also skipped in Structured Mode — pasting is user-driven from the panel.
    let command_send_active = voice_commands_enabled && command_send_enabled;
    if structured.is_none()
        && output_config.ship_mode
        && !command_send_active
        && matches!(
            output_config.mode,
            crate::output::types::OutputMode::TypeSimulation
                | crate::output::types::OutputMode::Both
        )
    {
        let _ = tokio::task::spawn_blocking(|| {
            // Wait for all keystrokes to land in the target app.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            if let Ok(mut enigo) = enigo::Enigo::new(&enigo::Settings::default()) {
                let _ = enigo::Keyboard::key(
                    &mut enigo,
                    enigo::Key::Return,
                    enigo::Direction::Click,
                );
            }
        })
        .await;
    }

    // 7. Notify frontend of the result.
    //
    //    `transcription-result` always fires — History auto-refresh, the
    //    global last-transcription store, and Notes-append all listen for
    //    it, so skipping it on the Structured path would silently break
    //    those flows.  For Structured Mode we also emit the rich payload
    //    so the overlay can render the preview panel.
    let _ = app_handle.emit("transcription-result", &final_text);
    if let Some((md, slots)) = &structured {
        let _ = app_handle.emit(
            "structured-output-ready",
            &StructuredOutputPayload {
                markdown: md.clone(),
                slots: slots.clone(),
                // Use the pre-processor ASR output so "View raw transcript"
                // actually shows what the user said — processed_text has
                // already been through filler removal, dictionary, and
                // capitalization, which would mask the original words.
                raw_transcript: transcription.text.clone(),
            },
        );
    }

    // 8. Save to history.
    //     `text` is the final paste-ready string (Markdown in Structured
    //     Mode, plain text otherwise).  `raw_transcript` stores the
    //     pre-processor ASR text so the Structured panel's "View raw"
    //     disclosure always reflects what the user actually spoke.
    let raw_transcript = if structured.is_some() {
        Some(transcription.text.clone())
    } else {
        None
    };
    let record = crate::storage::types::TranscriptionRecord {
        id: uuid::Uuid::new_v4(),
        text: final_text,
        duration_ms: transcription.duration_ms,
        model_name: transcription.model_name,
        created_at: chrono::Utc::now(),
        raw_transcript,
    };
    if let Err(e) = crate::storage::history::save_transcription(&state.db, &record) {
        eprintln!("Failed to save transcription to history: {e}");
    }

    let _ = app_handle.emit("recording-state-change", "idle");
}

/// Cancel an in-progress recording without transcribing.
pub fn cancel_recording(app_handle: &tauri::AppHandle, state: &AppState) {
    crate::audio::ducking::unduck();
    if let Ok(mut audio) = state.audio.lock() {
        audio.cancel();
    }
    let _ = app_handle.emit("recording-state-change", "idle");
}

/// Get the current audio level for the VU meter (0.0–1.0).
pub fn current_audio_level(state: &AppState) -> f32 {
    state
        .audio
        .lock()
        .map(|a| a.current_level())
        .unwrap_or(0.0)
}
