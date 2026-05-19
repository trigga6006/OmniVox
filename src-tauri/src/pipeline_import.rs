//! Pipeline for the Import Audio feature.
//!
//! Strictly **transcribe + persist** — none of the live-pipeline side
//! effects (focus restore, output routing, ducking, screen context,
//! Ship Mode, voice commands, Voxify trigger).  Imports must never
//! paste into a focused app or auto-press Enter.
//!
//! Events emitted (all carry the `import_id` so a single global
//! listener in the frontend can filter):
//!
//! - `import-progress` — `{ import_id, phase, percent, duration_ms }`
//! - `import-transcription-ready` — `{ import_id, text, source_filename, duration_ms }`
//! - `import-error` — `{ import_id, message }`
//!
//! Cancellation is cooperative: the caller flips
//! `cancel: Arc<AtomicBool>` via the `import_cancel` command, and the
//! decode loop + whisper abort callback both observe it.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::OwnedSemaphorePermit;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::postprocess::processor::TextProcessor;
use crate::state::AppState;
use crate::storage::types::TranscriptionRecord;

#[derive(Clone, Serialize)]
struct ImportProgressPayload {
    import_id: String,
    phase: &'static str, // "decoding" | "transcribing"
    percent: u8,
    duration_ms: Option<u64>,
}

#[derive(Clone, Serialize)]
struct ImportReadyPayload {
    import_id: String,
    text: String,
    source_filename: String,
    duration_ms: u64,
}

#[derive(Clone, Serialize)]
struct ImportErrorPayload {
    import_id: String,
    message: String,
}

#[derive(Clone, Serialize)]
struct ImportCancelledPayload {
    import_id: String,
}

/// Run the full import pipeline.  Always cleans up `active_import` and
/// drops the transcription gate permit when it returns.
pub async fn transcribe_file(
    app_handle: AppHandle,
    import_id: Uuid,
    path: PathBuf,
    cancel: Arc<AtomicBool>,
    permit: OwnedSemaphorePermit,
) {
    let import_id_str = import_id.to_string();
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("audio")
        .to_string();

    let outcome = run_inner(&app_handle, &import_id_str, &filename, path.as_path(), &cancel).await;

    // Always clear active_import + drop the gate permit BEFORE emitting
    // ANY terminal event (ready / error / cancelled).  The frontend
    // treats those events as "backend is fully released — safe to start
    // a new import or recording", so the cleanup has to happen first.
    // Only clears active_import if the id still matches (an interleaved
    // cancel-then-restart shouldn't drop the new entry by mistake).
    let state = app_handle.state::<AppState>();
    if let Ok(mut guard) = state.active_import.lock() {
        if guard.as_ref().map(|s| s.id) == Some(import_id) {
            *guard = None;
        }
    }
    drop(permit);

    match outcome {
        Ok(ready) => {
            let _ = app_handle.emit("import-transcription-ready", &ready);
        }
        Err(AppError::Cancelled) => {
            // Surface to the UI so it can leave the "cancelling" state.
            // Without this event the page would otherwise sit forever
            // showing "Cancelling…" because Whisper's abort latency
            // means cancel-flag-flip and task-actually-exits are
            // seconds apart.
            eprintln!("Import {import_id_str} cancelled");
            let _ = app_handle.emit(
                "import-cancelled",
                &ImportCancelledPayload {
                    import_id: import_id_str.clone(),
                },
            );
        }
        Err(e) => {
            eprintln!("Import {import_id_str} failed: {e}");
            let _ = app_handle.emit(
                "import-error",
                &ImportErrorPayload {
                    import_id: import_id_str.clone(),
                    message: e.to_string(),
                },
            );
        }
    }
}

async fn run_inner(
    app_handle: &AppHandle,
    import_id_str: &str,
    filename: &str,
    path: &Path,
    cancel: &Arc<AtomicBool>,
) -> AppResult<ImportReadyPayload> {
    // ─── 1. Initial event so the UI knows we accepted the file ────────
    let _ = app_handle.emit(
        "import-progress",
        &ImportProgressPayload {
            import_id: import_id_str.to_string(),
            phase: "decoding",
            percent: 0,
            duration_ms: None,
        },
    );

    // ─── 2. Decode + resample (CPU-bound — runs on blocking pool) ─────
    let decoded = {
        let decode_handle = app_handle.clone();
        let decode_id = import_id_str.to_string();
        let decode_cancel = cancel.clone();
        let decode_path: PathBuf = path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            let mut last_emit: i32 = -10;
            crate::audio::decode::decode_file(
                &decode_path,
                |p: f32| {
                    let pct = (p * 100.0).clamp(0.0, 100.0) as i32;
                    if (pct - last_emit).abs() >= 1 {
                        last_emit = pct;
                        let _ = decode_handle.emit(
                            "import-progress",
                            &ImportProgressPayload {
                                import_id: decode_id.clone(),
                                phase: "decoding",
                                percent: pct.clamp(0, 100) as u8,
                                duration_ms: None,
                            },
                        );
                    }
                },
                &decode_cancel,
            )
        })
        .await
        .map_err(|e| AppError::Internal(format!("decode task panic: {e}")))??
    };

    if decoded.samples.is_empty() {
        return Err(AppError::Decode("file contained no audio".into()));
    }

    // Cancel check between phases.
    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }

    // ─── 3. Phase transition event with duration ──────────────────────
    let _ = app_handle.emit(
        "import-progress",
        &ImportProgressPayload {
            import_id: import_id_str.to_string(),
            phase: "transcribing",
            percent: 0,
            duration_ms: Some(decoded.duration_ms),
        },
    );

    // ─── 4. Audio post-processing (denoise + normalize) ───────────────
    let state = app_handle.state::<AppState>();
    let mut samples = decoded.samples;

    let noise_reduction = crate::storage::settings::get_settings(&state.db)
        .ok()
        .map(|s| s.noise_reduction)
        .unwrap_or(false);
    if noise_reduction {
        crate::audio::denoise::denoise(&mut samples);
    }
    crate::audio::normalize::normalize_peak(&mut samples);

    // ─── 5. Grab the loaded WhisperEngine (Arc — released before spawn) ─
    let engine = {
        let guard = state
            .engine
            .lock()
            .map_err(|_| AppError::Internal("engine lock poisoned".into()))?;
        match guard.as_ref() {
            Some(e) => Arc::clone(e),
            None => {
                return Err(AppError::Asr(
                    "No Whisper model loaded — open the Models page to download one".into(),
                ));
            }
        }
    };

    // ─── 6. Run Whisper inference with progress + abort callbacks ─────
    let progress_handle = app_handle.clone();
    let progress_id = import_id_str.to_string();
    let progress_duration_ms = decoded.duration_ms;
    let mut last_emit: i32 = -10;
    let on_progress = move |p: i32| {
        if (p - last_emit).abs() >= 1 {
            last_emit = p;
            let _ = progress_handle.emit(
                "import-progress",
                &ImportProgressPayload {
                    import_id: progress_id.clone(),
                    phase: "transcribing",
                    percent: p.clamp(0, 100) as u8,
                    duration_ms: Some(progress_duration_ms),
                },
            );
        }
    };
    let abort_cancel = cancel.clone();
    let abort_when = move || abort_cancel.load(Ordering::Relaxed);

    let engine_for_transcribe = Arc::clone(&engine);
    let transcription = match tokio::task::spawn_blocking(move || {
        engine_for_transcribe.transcribe_with_progress(&samples, on_progress, abort_when)
    })
    .await
    {
        Ok(Ok(t)) => t,
        Ok(Err(e)) => {
            // Whisper bails with Err when abort returns true — surface
            // cancellation rather than a misleading "transcription failed".
            if cancel.load(Ordering::Relaxed) {
                return Err(AppError::Cancelled);
            }
            return Err(e);
        }
        Err(join_err) => {
            return Err(AppError::Internal(format!(
                "transcription task panic: {join_err}"
            )));
        }
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(AppError::Cancelled);
    }
    if transcription.text.trim().is_empty() {
        return Err(AppError::Asr("Whisper produced no text".into()));
    }

    // ─── 7. Dictionary / capitalization (same as live path) ───────────
    // NOTE: deliberately NOT applying voice-command parsing, Ship Mode,
    // Structured Mode, or output routing — see §6.2 of the spec.
    let processed = {
        let processor = state
            .processor
            .lock()
            .map_err(|_| AppError::Internal("processor lock poisoned".into()))?;
        processor
            .process(&transcription.text)
            .map(|p| p.processed)
            .unwrap_or_else(|_| transcription.text.clone())
    };

    // ─── 8. Persist + emit ready ──────────────────────────────────────
    let record = TranscriptionRecord {
        id: Uuid::new_v4(),
        text: processed.clone(),
        duration_ms: transcription.duration_ms,
        model_name: transcription.model_name,
        created_at: chrono::Utc::now(),
        raw_transcript: None,
        source: Some("import".to_string()),
        source_filename: Some(filename.to_string()),
        source_duration_ms: Some(decoded.duration_ms),
    };
    if let Err(e) = crate::storage::history::save_transcription(&state.db, &record) {
        // Non-fatal — the transcript still reaches the UI via the
        // ready event returned below.  History will rebuild from disk
        // next launch.
        eprintln!("Import: failed to persist transcription: {e}");
    }

    // Return the payload so the OUTER function can emit it after
    // clearing `active_import` and dropping the gate permit.  The
    // frontend treats this event as "backend is fully released" — see
    // the cleanup-before-emit comment in `transcribe_file`.
    Ok(ImportReadyPayload {
        import_id: import_id_str.to_string(),
        text: processed,
        source_filename: filename.to_string(),
        duration_ms: decoded.duration_ms,
    })
}
