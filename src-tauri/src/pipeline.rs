use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::asr::engine::AsrEngine;
use crate::error::ErrorCode;
use crate::postprocess::processor::TextProcessor;
use crate::state::AppState;

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
    use std::process::Command;
    // Use osascript to get the frontmost application's PID.
    // NSWorkspace.shared.frontmostApplication is the Cocoa way, but going
    // through osascript avoids objc runtime complexity in the hot path.
    let output = Command::new("osascript")
        .args(["-e", r#"tell application "System Events" to unix id of first process whose frontmost is true"#])
        .output()
        .ok()?;

    let pid_str = String::from_utf8_lossy(&output.stdout);
    pid_str.trim().parse::<i32>().ok().map(|pid| pid as isize)
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
    use std::process::Command;
    // Get the executable name for a given PID via ps.
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .ok()?;

    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        // Extract just the binary name from the path (e.g. "/Applications/Slack.app/Contents/MacOS/Slack" -> "Slack")
        name.rsplit('/').next().map(|s| s.to_string())
    }
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
    use std::process::Command;
    // Activate the application by PID using osascript.
    // This is equivalent to NSRunningApplication.activate().
    let script = format!(
        r#"tell application "System Events"
            set targetProcess to first process whose unix id is {}
            set frontmost of targetProcess to true
        end tell"#,
        pid
    );
    let _ = Command::new("osascript").args(["-e", &script]).output();
    // Give the OS time to process the focus switch.
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn restore_foreground_window(_hwnd: isize) {}

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

    // Auto-switch context mode based on the foreground application.
    if let Some(hwnd) = fg {
        let auto_switch = crate::storage::settings::get_settings(&state.db)
            .map(|s| s.auto_switch_modes)
            .unwrap_or(false);

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
    crate::audio::ducking::duck();

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

    // Spawn a periodic task that emits audio-level events to the frontend
    let handle = app_handle.clone();
    let is_rec_clone = is_recording.clone();
    tauri::async_runtime::spawn(async move {
        use std::sync::atomic::Ordering;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if !is_rec_clone.load(Ordering::Relaxed) {
                break;
            }
            let level = f32::from_bits(rms_level.load(Ordering::Relaxed));
            let _ = handle.emit("audio-level", level);
        }
    });

    // Spawn live preview task — periodically transcribes the last 5s of audio
    // and emits partial results to the overlay pill.
    let live_preview = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.live_preview)
        .unwrap_or(false);

    if live_preview {
        let handle = app_handle.clone();
        let engine: Option<Arc<crate::asr::engine::WhisperEngine>> = state
            .engine
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(Arc::clone));

        if let Some(engine) = engine {
            tauri::async_runtime::spawn(async move {
                use std::sync::atomic::Ordering;
                // 5 seconds of audio at 16 kHz
                const PREVIEW_SAMPLES: usize = 16_000 * 5;

                // Wait 3s before first preview to accumulate enough audio
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                loop {
                    if !is_recording.load(Ordering::Relaxed) {
                        break;
                    }

                    // Snapshot the last 5s of audio (brief mutex hold)
                    let samples = {
                        let state: tauri::State<'_, AppState> = handle.state();
                        let audio = match state.audio.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        audio.snapshot_tail(PREVIEW_SAMPLES)
                    };

                    if samples.len() < 8_000 {
                        // Less than 0.5s of audio — skip this round
                        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                        continue;
                    }

                    // Run greedy transcription on a blocking thread
                    let eng = engine.clone();
                    let mut preview_samples = samples;
                    crate::audio::normalize::normalize_peak(&mut preview_samples);

                    let result = tokio::task::spawn_blocking(move || {
                        eng.transcribe_preview(&preview_samples)
                    })
                    .await;

                    // Check recording state again — may have stopped during transcription
                    if !is_recording.load(Ordering::Relaxed) {
                        break;
                    }

                    if let Ok(Ok(text)) = result {
                        if !text.is_empty() {
                            let _ = handle.emit("transcription-preview", &text);
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
            });
        }
    }
}

/// Stop capture, run Whisper inference, post-process, and output the text.
pub async fn stop_and_transcribe(app_handle: &tauri::AppHandle, state: &AppState) {
    // Restore system volume immediately — don't wait for transcription.
    crate::audio::ducking::unduck();

    let _ = app_handle.emit("recording-state-change", "processing");

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

    // 1b. Conditionally denoise audio with RNNoise before Whisper.
    let mut samples = samples;
    let noise_reduction = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.noise_reduction)
        .unwrap_or(false);
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

    // 4. Apply deterministic list formatting (bullet lists for enumerated
    //     items).  Structural formatting is handled here at zero cost.
    let final_text = crate::postprocess::formatter::format_lists(&processed_text);

    // 4b. Voice command detection (if enabled).
    //     Splits text into [Text | Command] segments so the output router can
    //     type text and execute keystrokes (Shift+Enter, Ctrl+Backspace, etc.).
    let voice_segments = {
        let enabled = crate::storage::settings::get_settings(&state.db)
            .map(|s| s.voice_commands)
            .unwrap_or(false);
        if enabled {
            Some(crate::postprocess::voice_commands::parse_commands(&final_text))
        } else {
            None
        }
    };

    // 5. Kick off focus restoration in parallel with output.
    let prev_hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
    let focus_task = prev_hwnd.map(|hwnd| {
        tokio::task::spawn_blocking(move || restore_foreground_window(hwnd))
    });

    // Wait for focus restoration to complete before outputting text.
    if let Some(task) = focus_task {
        let _ = task.await;
    }

    // 6. Output to the focused application
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let output_result = if let Some(ref segments) = voice_segments {
        state.output.send_segments(segments, &output_config)
    } else {
        state.output.send(&final_text, &output_config)
    };
    if let Err(e) = output_result {
        eprintln!("Output failed: {e}");
        emit_error(app_handle, e.code(), format!("Output failed: {e}"));
    }

    // 6b. Ship Mode — automatically press Enter to send the message.
    //     Only fires when type simulation was used (clipboard-only can't auto-send).
    if output_config.ship_mode
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

    // 7. Notify frontend (before moving final_text into history)
    let _ = app_handle.emit("transcription-result", &final_text);

    // 8. Save to history — move final_text to avoid an extra clone
    let record = crate::storage::types::TranscriptionRecord {
        id: uuid::Uuid::new_v4(),
        text: final_text,
        duration_ms: transcription.duration_ms,
        model_name: transcription.model_name,
        created_at: chrono::Utc::now(),
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
