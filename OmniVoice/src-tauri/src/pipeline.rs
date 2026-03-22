use tauri::{Emitter, Manager};

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

    let is_recording = state
        .audio
        .lock()
        .unwrap()
        .is_recording();

    if !is_recording {
        start_recording(app_handle, &state);
    } else {
        stop_and_transcribe(app_handle, &state).await;
    }
}

/// Begin microphone capture.
pub fn start_recording(app_handle: &tauri::AppHandle, state: &AppState) {
    let mut audio = state.audio.lock().unwrap();

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
        let mut audio = state.audio.lock().unwrap();
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

    // 2. Transcribe — CPU-bound, run on blocking thread pool
    let engine_guard = state.engine.lock().unwrap();
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
    let processed_text = {
        let processor = state.processor.lock().unwrap();
        match processor.process(&transcription.text) {
            Ok(processed) => processed.processed,
            Err(_) => transcription.text.clone(),
        }
    };

    // 4. AI cleanup via local LLM (if enabled and model loaded)
    let final_text = {
        let llm_guard = state.llm_engine.lock().unwrap();
        if let Some(ref engine) = *llm_guard {
            match engine.cleanup_text(&processed_text) {
                Ok(cleaned) => cleaned,
                Err(e) => {
                    eprintln!("LLM cleanup failed, using raw text: {e}");
                    processed_text
                }
            }
        } else {
            processed_text
        }
    };

    // 4. Output to the focused application
    let output_config = state.output_config.lock().unwrap().clone();
    if let Err(e) = state.output.send(&final_text, &output_config) {
        eprintln!("Output failed: {e}");
    }

    // 5. Notify frontend
    let _ = app_handle.emit("transcription-result", &final_text);
    let _ = app_handle.emit("recording-state-change", "idle");
}

/// Cancel an in-progress recording without transcribing.
pub fn cancel_recording(app_handle: &tauri::AppHandle, state: &AppState) {
    state.audio.lock().unwrap().cancel();
    let _ = app_handle.emit("recording-state-change", "idle");
}

/// Get the current audio level for the VU meter (0.0–1.0).
pub fn current_audio_level(state: &AppState) -> f32 {
    state.audio.lock().unwrap().current_level()
}
