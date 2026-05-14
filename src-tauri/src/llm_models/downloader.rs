use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use reqwest::Client;
use tauri::Emitter;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::llm_models::manager::LlmModelManager;
use crate::llm_models::types::{LlmDownloadProgress, LlmDownloadStatus};

/// Streaming downloader for GGUF LLM files.  Mirrors `ModelDownloader` but:
/// - Emits on the `llm-download-progress` channel (separate from Whisper
///   downloads so existing listeners aren't confused by mixed events).
/// - Resolves repo+filename dynamically from `LlmModelManager` instead of
///   hard-coded whisper.cpp paths.
pub struct LlmModelDownloader {
    client: Client,
    llm_models_dir: PathBuf,
    cancel_flag: Arc<AtomicBool>,
}

/// Upper bound on any LLM download.  Covers the full Qwen3 1.7B quant; bigger
/// than that and we want the user to be explicit about memory pressure.
const MAX_LLM_DOWNLOAD_BYTES: u64 = 3_000_000_000;

impl LlmModelDownloader {
    pub fn new(llm_models_dir: PathBuf) -> Self {
        Self {
            client: Client::new(),
            llm_models_dir,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Download a catalog model, emitting `llm-download-progress` events.
    /// Returns the final path on disk (after atomic `.part` → final rename).
    pub async fn download(
        &self,
        manager: &LlmModelManager,
        model_id: &str,
        app_handle: &tauri::AppHandle,
    ) -> AppResult<PathBuf> {
        let info = manager
            .get_model(model_id)
            .ok_or_else(|| AppError::Llm(format!("Unknown LLM model: {model_id}")))?;

        let url = format!(
            "https://huggingface.co/{}/resolve/main/{}",
            info.huggingface_repo, info.huggingface_file
        );
        let target_path = self.llm_models_dir.join(&info.huggingface_file);
        let part_path = self
            .llm_models_dir
            .join(format!("{}.part", info.huggingface_file));

        if target_path.exists() {
            return Ok(target_path);
        }

        fs::create_dir_all(&self.llm_models_dir)
            .await
            .map_err(|e| AppError::Llm(format!("Failed to create LLM dir: {e}")))?;

        self.cancel_flag.store(false, Ordering::SeqCst);
        self.emit_progress(app_handle, model_id, 0, 0, LlmDownloadStatus::Downloading);

        let mut response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Llm(format!("LLM download request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(AppError::Llm(format!(
                "LLM download failed: HTTP {status} for {url}"
            )));
        }

        let total_bytes = response.content_length().unwrap_or(0);
        if total_bytes > MAX_LLM_DOWNLOAD_BYTES {
            return Err(AppError::Llm(format!(
                "LLM size ({total_bytes} bytes) exceeds maximum ({MAX_LLM_DOWNLOAD_BYTES} bytes)"
            )));
        }

        let mut file = fs::File::create(&part_path)
            .await
            .map_err(|e| AppError::Llm(format!("Failed to create .part: {e}")))?;

        let mut downloaded: u64 = 0;
        let mut last_emit_percent: u32 = 0;

        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AppError::Llm(format!("LLM download stream error: {e}")))?
        {
            if self.cancel_flag.load(Ordering::Relaxed) {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                self.emit_progress(
                    app_handle,
                    model_id,
                    downloaded,
                    total_bytes,
                    LlmDownloadStatus::Cancelled,
                );
                return Err(AppError::Llm("LLM download cancelled".into()));
            }

            file.write_all(&chunk)
                .await
                .map_err(|e| AppError::Llm(format!("LLM write error: {e}")))?;

            downloaded += chunk.len() as u64;
            if downloaded > MAX_LLM_DOWNLOAD_BYTES {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                return Err(AppError::Llm(format!(
                    "Downloaded LLM bytes ({downloaded}) exceed maximum ({MAX_LLM_DOWNLOAD_BYTES})"
                )));
            }

            let percent = if total_bytes > 0 {
                ((downloaded as f64 / total_bytes as f64) * 100.0) as u32
            } else {
                0
            };
            if percent > last_emit_percent {
                last_emit_percent = percent;
                self.emit_progress(
                    app_handle,
                    model_id,
                    downloaded,
                    total_bytes,
                    LlmDownloadStatus::Downloading,
                );
            }
        }

        file.flush()
            .await
            .map_err(|e| AppError::Llm(format!("LLM flush error: {e}")))?;
        drop(file);

        fs::rename(&part_path, &target_path)
            .await
            .map_err(|e| AppError::Llm(format!("Failed to finalize LLM download: {e}")))?;

        self.emit_progress(
            app_handle,
            model_id,
            downloaded,
            total_bytes,
            LlmDownloadStatus::Completed,
        );

        Ok(target_path)
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    fn emit_progress(
        &self,
        app_handle: &tauri::AppHandle,
        model_id: &str,
        downloaded_bytes: u64,
        total_bytes: u64,
        status: LlmDownloadStatus,
    ) {
        let progress_percent = if total_bytes > 0 {
            (downloaded_bytes as f32 / total_bytes as f32) * 100.0
        } else {
            0.0
        };
        let _ = app_handle.emit(
            "llm-download-progress",
            LlmDownloadProgress {
                model_id: model_id.to_string(),
                downloaded_bytes,
                total_bytes,
                progress_percent,
                status,
            },
        );
    }
}
