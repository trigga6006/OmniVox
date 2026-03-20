use tauri::Emitter;

use crate::asr::engine::AsrEngine;
use crate::postprocess::processor::TextProcessor;
use crate::state::AppState;

/// Toggle recording on/off. Called by the global hotkey handler and the
/// frontend start/stop commands.
///
/// Flow:
/// 1. If idle → start mic capture, emit "recording" state
/// 2. If recording → stop capture → transcribe → post-process → output → emit result
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

/// Begin microphone capture.
pub fn start_recording(app_handle: &tauri::AppHandle, state: &AppState) {
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

    let _ = app_handle.emit("recording-state-change", "recording");
}

/// Stop capture, run Whisper inference, post-process, and output the text.
pub async fn stop_and_transcribe(app_handle: &tauri::AppHandle, state: &AppState) {
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

    // 2. Transcribe — CPU-bound
    let engine_guard = match state.engine.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let transcription = match engine_guard.as_ref() {
        Some(engine) => engine.transcribe(&samples),
        None => {
            drop(engine_guard);
            eprintln!("No model loaded — cannot transcribe");
            let _ = app_handle.emit("recording-state-change", "error");
            return;
        }
    };
    drop(engine_guard);

    let transcription = match transcription {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Transcription failed: {e}");
            let _ = app_handle.emit("recording-state-change", "error");
            return;
        }
    };

    if transcription.text.is_empty() {
        let _ = app_handle.emit("recording-state-change", "idle");
        return;
    }

    // 3. Post-process (dictionary replacements, capitalization, etc.)
    let final_text = {
        let processor = match state.processor.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match processor.process(&transcription.text) {
            Ok(processed) => processed.processed,
            Err(_) => transcription.text.clone(),
        }
    };

    // 4. Output to the focused application
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Err(e) = state.output.send(&final_text, &output_config) {
        eprintln!("Output failed: {e}");
    }

    // 5. Save to history
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

    // 6. Notify frontend
    let _ = app_handle.emit("transcription-result", &final_text);
    let _ = app_handle.emit("recording-state-change", "idle");
}

/// Cancel an in-progress recording without transcribing.
pub fn cancel_recording(app_handle: &tauri::AppHandle, state: &AppState) {
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
