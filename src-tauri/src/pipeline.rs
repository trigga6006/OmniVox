use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::asr::engine::AsrEngine;
use crate::postprocess::processor::TextProcessor;
use crate::state::AppState;

/// Snapshot the currently focused window so we can restore it before pasting.
/// Returns the HWND as an isize for storage in AppState.
#[cfg(target_os = "windows")]
fn capture_foreground_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() { None } else { Some(hwnd as isize) }
}

#[cfg(not(target_os = "windows"))]
fn capture_foreground_window() -> Option<isize> {
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

#[cfg(not(target_os = "windows"))]
fn restore_foreground_window(_hwnd: isize) {}

/// Toggle recording on/off. Called by the frontend start/stop commands.
pub async fn toggle_recording(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    let is_recording = match state.audio.lock() {
        Ok(audio) => audio.is_recording(),
        Err(_) => {
            let _ = app_handle.emit("recording-state-change", "error");
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
        let _ = app_handle.emit("recording-state-change", "error");
        return;
    }

    // Grab Arc handles for the audio level emitter before dropping the lock
    let is_recording = audio.is_recording_flag();
    let rms_level = audio.rms_level_ref();
    drop(audio);

    let _ = app_handle.emit("recording-state-change", "recording");

    // Spawn a periodic task that emits audio-level events to the frontend
    let handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        use std::sync::atomic::Ordering;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if !is_recording.load(Ordering::Relaxed) {
                break;
            }
            let level = f32::from_bits(rms_level.load(Ordering::Relaxed));
            let _ = handle.emit("audio-level", level);
        }
    });
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
                let _ = app_handle.emit("recording-state-change", "error");
                return;
            }
        }
    };

    if samples.is_empty() {
        let _ = app_handle.emit("recording-state-change", "idle");
        return;
    }

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
                let _ = app_handle.emit("recording-state-change", "error");
                return;
            }
        }
    };

    let transcription = match tokio::task::spawn_blocking(move || engine.transcribe(&samples)).await
    {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            eprintln!("Transcription failed: {e}");
            let _ = app_handle.emit("recording-state-change", "error");
            return;
        }
        Err(e) => {
            eprintln!("Transcription task panicked: {e}");
            let _ = app_handle.emit("recording-state-change", "error");
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

    // 4. Kick off focus restoration in parallel with LLM cleanup.
    //    Focus restore involves OS sleeps (~52 ms) that can overlap with the
    //    much slower LLM inference, saving wall-clock time.
    let prev_hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
    let focus_task = prev_hwnd.map(|hwnd| {
        tokio::task::spawn_blocking(move || restore_foreground_window(hwnd))
    });

    // 5. AI cleanup via local LLM (if enabled and model loaded) — runs
    //    concurrently with focus restoration above.
    let active_prompt = state.active_llm_prompt.lock()
        .map(|g| g.clone())
        .unwrap_or(None);
    let final_text = {
        let mut llm_guard = match state.llm_engine.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(ref mut engine) = *llm_guard {
            match engine.cleanup_text(&processed_text, active_prompt.as_deref()) {
                Ok(cleaned) => cleaned,
                Err(e) => {
                    eprintln!("LLM cleanup error, using raw text: {e}");
                    processed_text
                }
            }
        } else {
            processed_text
        }
    };

    // Wait for focus restoration to complete before outputting text.
    if let Some(task) = focus_task {
        let _ = task.await;
    }

    // 6. Output to the focused application
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Err(e) = state.output.send(&final_text, &output_config) {
        eprintln!("Output failed: {e}");
    }

    // 7. Save to history
    let record = crate::storage::types::TranscriptionRecord {
        id: uuid::Uuid::new_v4(),
        text: final_text.clone(),
        duration_ms: transcription.duration_ms,
        model_name: transcription.model_name.clone(),
        created_at: chrono::Utc::now(),
    };
    if let Err(e) = crate::storage::history::save_transcription(&state.db, &record) {
        eprintln!("Failed to save transcription to history: {e}");
    }

    // 8. Notify frontend
    let _ = app_handle.emit("transcription-result", &final_text);
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
