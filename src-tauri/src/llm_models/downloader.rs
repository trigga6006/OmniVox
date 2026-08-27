use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use reqwest::Client;
use sha2::Digest;
use tauri::Emitter;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::llm_models::manager::LlmModelManager;
use crate::llm_models::types::{LlmDownloadProgress, LlmDownloadStatus};
use crate::models::integrity::{
    new_hasher, safe_artifact_path, sha256_hex, verify_or_migrate_file, verify_stream_result,
    write_verification_marker,
};

/// Upper bound on any LLM download. Covers the supported Qwen 1.7B quant;
/// larger files require an explicit catalog and resource-policy update.
const MAX_LLM_DOWNLOAD_BYTES: u64 = 3_000_000_000;

pub struct LlmModelDownloader {
    client: Client,
    llm_models_dir: PathBuf,
    cancel_flag: Arc<AtomicBool>,
}

impl LlmModelDownloader {
    pub fn new(llm_models_dir: PathBuf) -> Self {
        Self {
            client: Client::new(),
            llm_models_dir,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn download(
        &self,
        manager: &LlmModelManager,
        model_id: &str,
        app_handle: &tauri::AppHandle,
    ) -> AppResult<PathBuf> {
        let info = manager
            .get_model(model_id)
            .ok_or_else(|| AppError::Llm(format!("Unknown LLM model: {model_id}")))?;
        let spec = LlmModelManager::download_artifact(model_id)?;
        if info.huggingface_repo != spec.repository || info.huggingface_file != spec.filename {
            return Err(AppError::Llm(format!(
                "LLM catalog manifest mismatch for '{}'",
                spec.model_id
            )));
        }

        let url = spec.download_url();
        let target_path =
            safe_artifact_path(&self.llm_models_dir, spec.filename).map_err(AppError::Llm)?;
        let part_path = safe_artifact_path(
            &self.llm_models_dir,
            &format!("{}.part-{}", spec.filename, uuid::Uuid::new_v4()),
        )
        .map_err(AppError::Llm)?;

        if target_path.exists() {
            let root = self.llm_models_dir.clone();
            return tokio::task::spawn_blocking(move || verify_or_migrate_file(&root, spec))
                .await
                .map_err(|e| AppError::Llm(format!("LLM verification task failed: {e}")))?
                .map_err(AppError::Llm);
        }

        fs::create_dir_all(&self.llm_models_dir)
            .await
            .map_err(|e| AppError::Llm(format!("Failed to create LLM dir: {e}")))?;

        self.cancel_flag.store(false, Ordering::SeqCst);
        self.emit_progress(
            app_handle,
            model_id,
            0,
            spec.size_bytes,
            LlmDownloadStatus::Downloading,
        );

        let mut response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Llm(format!("LLM download request failed: {e}")))?;
        if !response.status().is_success() {
            return Err(AppError::Llm(format!(
                "LLM download failed: HTTP {} for immutable model artifact",
                response.status()
            )));
        }

        let total_bytes = response.content_length().unwrap_or(spec.size_bytes);
        if total_bytes > MAX_LLM_DOWNLOAD_BYTES {
            return Err(AppError::Llm(format!(
                "LLM size ({total_bytes} bytes) exceeds maximum ({MAX_LLM_DOWNLOAD_BYTES} bytes)"
            )));
        }
        if total_bytes != spec.size_bytes {
            return Err(AppError::Llm(format!(
                "Integrity metadata mismatch for '{}': expected {} bytes, server reported {}",
                spec.model_id, spec.size_bytes, total_bytes
            )));
        }

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part_path)
            .await
            .map_err(|e| AppError::Llm(format!("Failed to create temporary LLM file: {e}")))?;
        let mut downloaded = 0_u64;
        let mut last_emit_percent = 0_u32;
        let mut hasher = new_hasher();

        loop {
            let chunk = match response.chunk().await {
                Ok(Some(chunk)) => chunk,
                Ok(None) => break,
                Err(e) => {
                    drop(file);
                    let _ = fs::remove_file(&part_path).await;
                    return Err(AppError::Llm(format!("LLM download stream error: {e}")));
                }
            };
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

            if let Err(e) = file.write_all(&chunk).await {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                return Err(AppError::Llm(format!("LLM write error: {e}")));
            }
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            if downloaded > MAX_LLM_DOWNLOAD_BYTES || downloaded > spec.size_bytes {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                return Err(AppError::Llm(format!(
                    "Downloaded LLM bytes ({downloaded}) exceed the catalog manifest"
                )));
            }

            let percent = ((downloaded as f64 / total_bytes as f64) * 100.0) as u32;
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
        file.sync_all()
            .await
            .map_err(|e| AppError::Llm(format!("LLM sync error: {e}")))?;
        drop(file);

        let actual_sha256 = sha256_hex(hasher);
        if let Err(e) = verify_stream_result(spec, downloaded, &actual_sha256) {
            let _ = fs::remove_file(&part_path).await;
            self.emit_progress(
                app_handle,
                model_id,
                downloaded,
                total_bytes,
                LlmDownloadStatus::Failed,
            );
            return Err(AppError::Llm(e));
        }

        if let Err(rename_error) = fs::rename(&part_path, &target_path).await {
            let _ = fs::remove_file(&part_path).await;
            let root = self.llm_models_dir.clone();
            if target_path.exists() {
                return tokio::task::spawn_blocking(move || verify_or_migrate_file(&root, spec))
                    .await
                    .map_err(|e| AppError::Llm(format!("LLM verification task failed: {e}")))?
                    .map_err(AppError::Llm);
            }
            return Err(AppError::Llm(format!(
                "Failed to finalize LLM download: {rename_error}"
            )));
        }
        write_verification_marker(&target_path, spec).map_err(AppError::Llm)?;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_unknown_and_traversal_ids() {
        for id in [
            "unknown",
            "../settings.db",
            "..\\settings.db",
            "C:\\temp\\x",
        ] {
            assert!(LlmModelManager::download_artifact(id).is_err());
        }
    }
}
