//! Tauri commands for the Import Audio feature.
//!
//! `import_transcribe` returns **immediately** after spawning the
//! background task — if it awaited the full pipeline, the frontend
//! would only register progress listeners after the await resolved,
//! by which point every `import-progress` event would already have
//! fired.  See `IMPORT_AUDIO_PLAN.md` §6.5.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::state::{AppState, ImportSlot};

/// Soft cap on input file size.  500 MB is longer than any realistic
/// dictation session yet still fits comfortably in RAM when decoded
/// to 16 kHz mono f32 (~4.7 hours = ~1 GB samples).
const MAX_IMPORT_BYTES: u64 = 500 * 1024 * 1024;

/// Recognized extensions.  We don't *enforce* via this list (symphonia
/// probes the actual content), but we reject obvious mismatches early
/// so the user gets a clean error instead of a cryptic decode failure.
const SUPPORTED_EXTENSIONS: &[&str] = &["wav", "mp3", "m4a", "mp4", "flac", "ogg", "oga", "aac"];

#[tauri::command]
pub async fn import_transcribe(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    import_id: String,
    path: String,
) -> Result<(), String> {
    // ─── Parse + validate inputs ──────────────────────────────────────
    let import_uuid =
        Uuid::parse_str(&import_id).map_err(|e| format!("bad import_id: {e}"))?;

    let path_buf = PathBuf::from(&path);
    if !path_buf.exists() {
        return Err(format!("File not found: {path}"));
    }
    if !path_buf.is_file() {
        return Err(format!("Not a file: {path}"));
    }

    if let Some(ext) = path_buf.extension().and_then(|s| s.to_str()) {
        let ext_lower = ext.to_ascii_lowercase();
        if !SUPPORTED_EXTENSIONS.iter().any(|s| *s == ext_lower) {
            return Err(format!(
                "Unsupported file type: .{ext_lower}. Try WAV, MP3, M4A, FLAC, OGG, or AAC."
            ));
        }
    } else {
        return Err("File has no extension — cannot determine format".to_string());
    }

    let file_size = std::fs::metadata(&path_buf)
        .map_err(|e| format!("stat failed: {e}"))?
        .len();
    if file_size > MAX_IMPORT_BYTES {
        return Err(format!(
            "File is {} MB; the import cap is {} MB.",
            file_size / (1024 * 1024),
            MAX_IMPORT_BYTES / (1024 * 1024)
        ));
    }

    // ─── Mutual-exclusion guards ──────────────────────────────────────
    // Reject if mic is actively capturing.
    let is_recording = state
        .audio
        .lock()
        .map(|a| a.is_recording())
        .unwrap_or(false);
    if is_recording {
        return Err("Stop recording before importing".to_string());
    }

    // Reject if live Whisper inference is still processing (mic stopped
    // but stop_and_transcribe holds the gate).
    let permit = state
        .transcription_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Dictation is still processing — try again in a moment".to_string())?;

    // Claim the active_import slot.  If this fails, the permit we just
    // acquired drops automatically and the gate is released.
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut guard = state
            .active_import
            .lock()
            .map_err(|_| "active_import lock poisoned".to_string())?;
        if guard.is_some() {
            return Err("Another import is already in progress".to_string());
        }
        *guard = Some(ImportSlot {
            id: import_uuid,
            cancel: Arc::clone(&cancel),
        });
    }

    // ─── Spawn the background task ────────────────────────────────────
    // The task owns the permit + cancel flag; AppState is re-fetched
    // inside via the AppHandle.
    let handle = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        crate::pipeline_import::transcribe_file(
            handle,
            import_uuid,
            path_buf,
            cancel,
            permit,
        )
        .await;
    });

    Ok(())
}

#[tauri::command]
pub async fn import_cancel(
    state: State<'_, AppState>,
    import_id: String,
) -> Result<(), String> {
    let import_uuid =
        Uuid::parse_str(&import_id).map_err(|e| format!("bad import_id: {e}"))?;

    let guard = state
        .active_import
        .lock()
        .map_err(|_| "active_import lock poisoned".to_string())?;

    match guard.as_ref() {
        Some(slot) if slot.id == import_uuid => {
            slot.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            Ok(())
        }
        Some(_) => Err("import_id does not match the active import".to_string()),
        None => Err("No import is currently in progress".to_string()),
    }
}
