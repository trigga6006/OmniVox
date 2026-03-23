use tauri::{Emitter, State};
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::llm::engine::LlmEngine;
use crate::llm::types::{AiCleanupStatus, LlmConfig};
use crate::models::types::{DownloadProgress, DownloadStatus};
use crate::state::AppState;

// TODO(model-upgrade): Update these once you've confirmed the exact GGUF
// model on HuggingFace.  Candidates:
//   - unsloth/Qwen3-1.7B-GGUF  (same family, 3× larger, known good GGUF)
//   - qingy2024/GRMR-V3-Q1.7B  (grammar-finetuned, but no pre-built GGUF yet)
//   - bartowski/GRMR-2B-Instruct-GGUF  (grammar-finetuned, GGUF available)
// The rest of the codebase (sidecar optimizations, n_threads, n_ctx, frontend)
// is model-agnostic and ready to go.
const LLM_MODEL_URL: &str =
    "https://huggingface.co/unsloth/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q4_K_M.gguf";
const LLM_MODEL_FILENAME: &str = "Qwen3-1.7B-Q4_K_M.gguf";
const LLM_MODEL_ID: &str = "llm-qwen3-1.7b";
/// Previous model filename — used to detect upgrades and show a migration hint.
const OLD_LLM_MODEL_FILENAME: &str = "Qwen3-0.6B-Q4_K_M.gguf";

#[tauri::command]
pub async fn get_ai_cleanup_status(state: State<'_, AppState>) -> Result<AiCleanupStatus, String> {
    let model_path = state.llm_models_dir.join(LLM_MODEL_FILENAME);
    let model_loaded = state.llm_engine.lock().unwrap().is_some();
    let old_model_exists = state.llm_models_dir.join(OLD_LLM_MODEL_FILENAME).exists();
    Ok(AiCleanupStatus {
        enabled: model_loaded,
        model_downloaded: model_path.exists(),
        model_loaded,
        upgrade_available: !model_path.exists() && old_model_exists,
    })
}

#[tauri::command]
pub async fn download_llm_model(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let target_path = state.llm_models_dir.join(LLM_MODEL_FILENAME);
    let part_path = state.llm_models_dir.join(format!("{LLM_MODEL_FILENAME}.part"));

    if target_path.exists() {
        return Ok(());
    }

    fs::create_dir_all(&state.llm_models_dir)
        .await
        .map_err(|e| format!("Failed to create LLM models dir: {e}"))?;

    emit_progress(&app_handle, 0, 0, DownloadStatus::Downloading);

    let client = reqwest::Client::new();
    let mut response = client.get(LLM_MODEL_URL).send().await
        .map_err(|e| format!("Download request failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("Download failed: HTTP {}", response.status()));
    }

    let total_bytes = response.content_length().unwrap_or(0);
    let mut file = fs::File::create(&part_path).await
        .map_err(|e| format!("Failed to create file: {e}"))?;

    let mut downloaded: u64 = 0;
    let mut last_emit_percent: u32 = 0;

    while let Some(chunk) = response.chunk().await
        .map_err(|e| format!("Download stream error: {e}"))?
    {
        file.write_all(&chunk).await
            .map_err(|e| format!("Write error: {e}"))?;
        downloaded += chunk.len() as u64;
        let percent = if total_bytes > 0 {
            ((downloaded as f64 / total_bytes as f64) * 100.0) as u32
        } else { 0 };
        if percent > last_emit_percent {
            last_emit_percent = percent;
            emit_progress(&app_handle, downloaded, total_bytes, DownloadStatus::Downloading);
        }
    }

    file.flush().await.map_err(|e| format!("Flush error: {e}"))?;
    drop(file);

    fs::rename(&part_path, &target_path).await
        .map_err(|e| format!("Failed to finalize download: {e}"))?;

    emit_progress(&app_handle, downloaded, total_bytes, DownloadStatus::Completed);
    Ok(())
}

#[tauri::command]
pub async fn enable_ai_cleanup(state: State<'_, AppState>) -> Result<(), String> {
    let model_path = state.llm_models_dir.join(LLM_MODEL_FILENAME);
    if !model_path.exists() {
        return Err("LLM model not downloaded yet".into());
    }
    if state.llm_engine.lock().unwrap().is_some() {
        return Ok(());
    }
    let config = LlmConfig::default_with_path(model_path);

    // Spawn on a blocking thread — the sidecar process does the heavy
    // model loading internally, but we still block waiting for its response.
    let engine = tokio::task::spawn_blocking(move || LlmEngine::load(config))
        .await
        .map_err(|e| format!("LLM loader task failed: {e}"))?
        .map_err(|e| format!("Failed to load LLM: {e}"))?;

    *state.llm_engine.lock().unwrap() = Some(engine);
    Ok(())
}

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
    } else { 0.0 };
    let _ = app_handle.emit("download-progress", DownloadProgress {
        model_id: LLM_MODEL_ID.to_string(),
        downloaded_bytes,
        total_bytes,
        progress_percent,
        status,
    });
}
