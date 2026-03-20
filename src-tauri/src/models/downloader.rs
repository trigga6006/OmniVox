use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use reqwest::Client;
use tauri::Emitter;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::models::types::{DownloadProgress, DownloadStatus};

/// Streaming model downloader with progress events and cancellation.
///
/// Downloads GGML whisper models from HuggingFace, writes to a `.part` file
/// during download, and atomically renames on completion to avoid partial files.
pub struct ModelDownloader {
    client: Client,
    models_dir: PathBuf,
    cancel_flag: Arc<AtomicBool>,
}

impl ModelDownloader {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            client: Client::new(),
            models_dir,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Download a model by its catalog ID.
    ///
    /// Emits `download-progress` Tauri events throughout the download.
    /// Returns the final path to the downloaded model file.
    pub async fn download(
        &self,
        model_id: &str,
        app_handle: &tauri::AppHandle,
    ) -> AppResult<PathBuf> {
        let url = model_url(model_id)?;
        let file_name = model_filename(model_id);
        let target_path = self.models_dir.join(&file_name);
        let part_path = self.models_dir.join(format!("{file_name}.part"));

        // Skip if already downloaded
        if target_path.exists() {
            return Ok(target_path);
        }

        fs::create_dir_all(&self.models_dir)
            .await
            .map_err(|e| AppError::Model(format!("Failed to create models dir: {e}")))?;

        self.cancel_flag.store(false, Ordering::SeqCst);

        self.emit_progress(app_handle, model_id, 0, 0, DownloadStatus::Downloading);

        let mut response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Model(format!("Request failed: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            return Err(AppError::Model(format!(
                "Download failed: HTTP {status} for {url}"
            )));
        }

        let total_bytes = response.content_length().unwrap_or(0);

        // Reject suspiciously large downloads (max 3 GB — largest whisper model is ~3.1 GB)
        const MAX_DOWNLOAD_BYTES: u64 = 3_500_000_000;
        if total_bytes > MAX_DOWNLOAD_BYTES {
            return Err(AppError::Model(format!(
                "File size ({total_bytes} bytes) exceeds maximum allowed ({MAX_DOWNLOAD_BYTES} bytes)"
            )));
        }

        let mut file = fs::File::create(&part_path)
            .await
            .map_err(|e| AppError::Model(format!("Failed to create file: {e}")))?;

        let mut downloaded: u64 = 0;
        let mut last_emit_percent: u32 = 0;

        // Stream response body chunk by chunk
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| AppError::Model(format!("Download stream error: {e}")))?
        {
            // Check for cancellation between chunks
            if self.cancel_flag.load(Ordering::Relaxed) {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                self.emit_progress(
                    app_handle,
                    model_id,
                    downloaded,
                    total_bytes,
                    DownloadStatus::Cancelled,
                );
                return Err(AppError::Model("Download cancelled".into()));
            }

            file.write_all(&chunk)
                .await
                .map_err(|e| AppError::Model(format!("Write error: {e}")))?;

            downloaded += chunk.len() as u64;

            // Throttle progress events to ~1 per percentage point
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
                    DownloadStatus::Downloading,
                );
            }
        }

        file.flush().await.map_err(|e| AppError::Io(e))?;
        drop(file);

        // Atomic rename: .part → final name
        fs::rename(&part_path, &target_path)
            .await
            .map_err(|e| AppError::Model(format!("Failed to finalize download: {e}")))?;

        self.emit_progress(
            app_handle,
            model_id,
            downloaded,
            total_bytes,
            DownloadStatus::Completed,
        );

        Ok(target_path)
    }

    /// Signal the current download to cancel. The next chunk read will abort.
    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    /// Check if a model is already downloaded.
    pub fn is_downloaded(&self, model_id: &str) -> bool {
        let file_name = model_filename(model_id);
        self.models_dir.join(file_name).exists()
    }

    /// Get the path to a downloaded model, if it exists.
    pub fn model_path(&self, model_id: &str) -> Option<PathBuf> {
        let path = self.models_dir.join(model_filename(model_id));
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Delete a downloaded model file.
    pub async fn delete(&self, model_id: &str) -> AppResult<()> {
        let path = self.models_dir.join(model_filename(model_id));
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| AppError::Model(format!("Failed to delete: {e}")))?;
        }
        Ok(())
    }

    fn emit_progress(
        &self,
        app_handle: &tauri::AppHandle,
        model_id: &str,
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
                model_id: model_id.to_string(),
                downloaded_bytes,
                total_bytes,
                progress_percent,
                status,
            },
        );
    }
}

/// Map model catalog IDs to HuggingFace download URLs.
fn model_url(model_id: &str) -> AppResult<String> {
    let filename = match model_id {
        "whisper-tiny" => "ggml-tiny.bin",
        "whisper-tiny-en" => "ggml-tiny.en.bin",
        "whisper-base" => "ggml-base.bin",
        "whisper-base-en" => "ggml-base.en.bin",
        "whisper-small" => "ggml-small.bin",
        "whisper-small-en" => "ggml-small.en.bin",
        "whisper-medium" => "ggml-medium.bin",
        "whisper-medium-en" => "ggml-medium.en.bin",
        "whisper-large" => "ggml-large-v3.bin",
        _ => {
            return Err(AppError::Model(format!(
                "Unknown model ID: '{model_id}'"
            )))
        }
    };

    Ok(format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{filename}"
    ))
}

/// Map model catalog IDs to local filenames.
pub fn model_filename(model_id: &str) -> String {
    match model_id {
        "whisper-tiny" => "ggml-tiny.bin",
        "whisper-tiny-en" => "ggml-tiny.en.bin",
        "whisper-base" => "ggml-base.bin",
        "whisper-base-en" => "ggml-base.en.bin",
        "whisper-small" => "ggml-small.bin",
        "whisper-small-en" => "ggml-small.en.bin",
        "whisper-medium" => "ggml-medium.bin",
        "whisper-medium-en" => "ggml-medium.en.bin",
        "whisper-large" => "ggml-large-v3.bin",
        _ => model_id,
    }
    .to_string()
}
