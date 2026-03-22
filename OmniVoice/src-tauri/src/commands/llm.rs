use tauri::{Emitter, State};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::llm::engine::LlmEngine;
use crate::llm::types::{AiCleanupStatus, LlmConfig};
use crate::models::types::{DownloadProgress, DownloadStatus};
use crate::state::AppState;

/// HuggingFace download URL for Qwen3-0.6B Q4_K_M GGUF.
const LLM_MODEL_URL: &str =
    "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/qwen3-0.6b-q4_k_m.gguf";

/// Local filename for the downloaded LLM model.
const LLM_MODEL_FILENAME: &str = "qwen3-0.6b-q4_k_m.gguf";

/// Model ID used in download progress events (distinguishes from Whisper downloads).
const LLM_MODEL_ID: &str = "llm-qwen3-0.6b";

/// Get the current status of the AI cleanup feature.
#[tauri::command]
pub async fn get_ai_cleanup_status(state: State<'_, AppState>) -> Result<AiCleanupStatus, String> {
    let model_path = state.llm_models_dir.join(LLM_MODEL_FILENAME);
    let model_loaded = state.llm_engine.lock().unwrap().is_some();

    Ok(AiCleanupStatus {
        enabled: model_loaded,
        model_downloaded: model_path.exists(),
        model_loaded,
    })
}

/// Download the LLM model from HuggingFace.
///
/// Emits `download-progress` events with model_id "llm-qwen3-0.6b" so the
/// frontend can show progress alongside (but distinct from) Whisper downloads.
#[tauri::command]
pub async fn download_llm_model(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_path = state.llm_models_dir.join(LLM_MODEL_FILENAME);
    let part_path = state.llm_models_dir.join(format!("{LLM_MODEL_FILENAME}.part"));

    // Skip if already downloaded
    if target_path.exists() {
        return Ok(());
    }

    fs::create_dir_all(&state.llm_models_dir)
        .await
        .map_err(|e| format!("Failed to create LLM models dir: {e}"))?;

    emit_progress(&app_handle, 0, 0, DownloadStatus::Downloading);

    let client = reqwest::Client::new();
    let mut response = client
        .get(LLM_MODEL_URL)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(format!("Download failed: HTTP {status}"));
    }

    let total_bytes = response.content_length().unwrap_or(0);

    let mut file = fs::File::create(&part_path)
        .await
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut last_emit_percent: u32 = 0;

    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| format!("Download stream error: {e}"))?
    {
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Write error: {e}"))?;

        downloaded += chunk.len() as u64;

        let percent = if total_bytes > 0 {
            ((downloaded as f64 / total_bytes as f64) * 100.0) as u32
        } else {
            0
        };

        if percent > last_emit_percent {
            last_emit_percent = percent;
            emit_progress(&app_handle, downloaded, total_bytes, DownloadStatus::Downloading);
        }
    }

    file.flush()
        .await
        .map_err(|e| format!("Flush error: {e}"))?;
    drop(file);

    // Atomic rename: .part → final name
    fs::rename(&part_path, &target_path)
        .await
        .map_err(|e| format!("Failed to finalize download: {e}"))?;

    emit_progress(&app_handle, downloaded, total_bytes, DownloadStatus::Completed);

    Ok(())
}

/// Enable AI cleanup: load the LLM model into memory.
///
/// The model must already be downloaded. Loading takes ~3-5s on first call.
#[tauri::command]
pub async fn enable_ai_cleanup(state: State<'_, AppState>) -> Result<(), String> {
    let model_path = state.llm_models_dir.join(LLM_MODEL_FILENAME);

    if !model_path.exists() {
        return Err("LLM model not downloaded yet".into());
    }

    // Check if already loaded
    if state.llm_engine.lock().unwrap().is_some() {
        return Ok(());
    }

    // Load the model (CPU-bound, may take a few seconds)
    let config = LlmConfig::default_with_path(model_path);
    let engine =
        LlmEngine::load(config).map_err(|e| format!("Failed to load LLM model: {e}"))?;

    *state.llm_engine.lock().unwrap() = Some(engine);

    Ok(())
}

/// Disable AI cleanup: unload the LLM model from memory.
#[tauri::command]
pub async fn disable_ai_cleanup(state: State<'_, AppState>) -> Result<(), String> {
    *state.llm_engine.lock().unwrap() = None;
    Ok(())
}

fn emit_progress(
    app_handle: &tauri::AppHandle,
    downloaded_bytes: u64,
    total_bytes: u64,
    status: DownloadStatus,
) {
    let progress_percent = if total_bytes > 0 {
        (downloaded_bytes as f32 / total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let _ = app_handle.emit(
        "download-progress",
        DownloadProgress {
            model_id: LLM_MODEL_ID.to_string(),
            downloaded_bytes,
            total_bytes,
            progress_percent,
            status,
        },
    );
}
