use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use reqwest::Client;
use sha2::Digest;
use tauri::Emitter;
use tokio::fs;
use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult};
use crate::models::integrity::{
    expected_file_exists, new_hasher, remove_verification_marker, safe_artifact_path,
    safe_model_dir, sha256_hex, verify_or_migrate_file, verify_stream_result,
    write_verification_marker, ArtifactSpec,
};
use crate::models::manager::ModelManager;
use crate::models::types::{DownloadProgress, DownloadStatus, ModelFamily};

/// Reject suspiciously large downloads. The largest supported Whisper model
/// is about 1.7 GB; this also provides headroom for future catalog entries.
const MAX_DOWNLOAD_BYTES: u64 = 3_500_000_000;

/// Streaming model downloader with progress events and cancellation.
///
/// Every accepted model ID resolves through the static artifact manifest.
/// Downloads use an immutable upstream revision, are hashed while streaming,
/// and become visible at the final path only after size and SHA-256 checks.
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

    /// Download a catalog entry, dispatching on the family's on-disk shape.
    /// Single-file (Whisper) and directory-backed (Parakeet) models enforce the
    /// same per-file size and SHA-256 contract; only the layout differs.
    pub async fn download(
        &self,
        model_id: &str,
        app_handle: &tauri::AppHandle,
    ) -> AppResult<PathBuf> {
        match ModelManager::family(model_id) {
            Some(ModelFamily::Parakeet) => self.download_directory(model_id, app_handle).await,
            _ => self.download_single(model_id, app_handle).await,
        }
    }

    async fn download_single(
        &self,
        model_id: &str,
        app_handle: &tauri::AppHandle,
    ) -> AppResult<PathBuf> {
        let spec = model_artifact(model_id)?;

        // Existing artifacts from older releases undergo a one-time hash
        // migration. A changed file invalidates its metadata-bound marker.
        let target_path =
            safe_artifact_path(&self.models_dir, spec.filename).map_err(AppError::Model)?;
        if target_path.exists() {
            let root = self.models_dir.clone();
            return tokio::task::spawn_blocking(move || verify_or_migrate_file(&root, spec))
                .await
                .map_err(|e| AppError::Model(format!("Model verification task failed: {e}")))?
                .map_err(AppError::Model);
        }

        fs::create_dir_all(&self.models_dir)
            .await
            .map_err(|e| AppError::Model(format!("Failed to create models dir: {e}")))?;

        self.cancel_flag.store(false, Ordering::SeqCst);
        self.emit_progress(
            app_handle,
            model_id,
            0,
            spec.size_bytes,
            DownloadStatus::Downloading,
        );

        let path = self
            .fetch_verified_file(
                app_handle,
                model_id,
                spec,
                &self.models_dir,
                0,
                spec.size_bytes,
            )
            .await?;

        self.emit_progress(
            app_handle,
            model_id,
            spec.size_bytes,
            spec.size_bytes,
            DownloadStatus::Completed,
        );
        Ok(path)
    }

    /// Download every artifact of a directory-backed model into
    /// `models_dir/<dir_name>/`, reporting one aggregated progress stream.
    ///
    /// Progress is summed across the files (already-verified files count as
    /// complete before their download would start), so the existing UI bar sees
    /// a single monotonic 0..total sequence rather than four restarts. The
    /// directory only becomes usable once every file has been verified, because
    /// `model_path` re-checks all of them.
    async fn download_directory(
        &self,
        model_id: &str,
        app_handle: &tauri::AppHandle,
    ) -> AppResult<PathBuf> {
        let artifact = model_directory_artifact(model_id)?;
        let dir = safe_model_dir(&self.models_dir, artifact.dir_name).map_err(AppError::Model)?;
        let total_bytes = artifact.total_bytes();
        if total_bytes > MAX_DOWNLOAD_BYTES {
            return Err(AppError::Model(format!(
                "Directory model total size ({total_bytes} bytes) exceeds maximum allowed ({MAX_DOWNLOAD_BYTES} bytes)"
            )));
        }

        // Already installed: return immediately, before the first progress
        // event, so re-clicking Download on a complete model doesn't flash a
        // 0% bar that never advances.
        if directory_is_complete(&self.models_dir, model_id) {
            return verified_model_directory(&self.models_dir, model_id);
        }

        fs::create_dir_all(&dir)
            .await
            .map_err(|e| AppError::Model(format!("Failed to create model dir: {e}")))?;

        self.cancel_flag.store(false, Ordering::SeqCst);
        self.emit_progress(
            app_handle,
            model_id,
            0,
            total_bytes,
            DownloadStatus::Downloading,
        );

        let mut downloaded_before = 0_u64;
        for spec in artifact.files {
            self.fetch_verified_file(
                app_handle,
                model_id,
                *spec,
                &dir,
                downloaded_before,
                total_bytes,
            )
            .await?;
            downloaded_before += spec.size_bytes;
        }

        self.emit_progress(
            app_handle,
            model_id,
            total_bytes,
            total_bytes,
            DownloadStatus::Completed,
        );
        Ok(dir)
    }

    /// Stream one artifact into `root`, hashing as it goes, and publish it at
    /// its final path only after the exact size and SHA-256 both match.
    ///
    /// `downloaded_before` and `overall_total` describe this file's place in the
    /// caller's progress stream; for a single-file model they are `0` and the
    /// file's own size, which reproduces the original one-file event sequence.
    async fn fetch_verified_file(
        &self,
        app_handle: &tauri::AppHandle,
        model_id: &str,
        spec: ArtifactSpec,
        root: &std::path::Path,
        downloaded_before: u64,
        overall_total: u64,
    ) -> AppResult<PathBuf> {
        let url = spec.download_url();
        let target_path = safe_artifact_path(root, spec.filename).map_err(AppError::Model)?;
        let part_path = safe_artifact_path(
            root,
            &format!("{}.part-{}", spec.filename, uuid::Uuid::new_v4()),
        )
        .map_err(AppError::Model)?;

        if target_path.exists() {
            let verify_root = root.to_path_buf();
            let verified =
                tokio::task::spawn_blocking(move || verify_or_migrate_file(&verify_root, spec))
                    .await
                    .map_err(|e| AppError::Model(format!("Model verification task failed: {e}")))?;
            match verified {
                Ok(path) => return Ok(path),
                Err(_) => {
                    // Wrong bytes at the right size (or any other verify
                    // failure) must not wedge the model as permanently
                    // broken. Clear the stale artifact and its marker, then
                    // fall through to a fresh download below.
                    let _ = fs::remove_file(&target_path).await;
                    remove_verification_marker(&target_path);
                }
            }
        }

        let mut response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AppError::Model(format!("Request failed: {e}")))?;

        if !response.status().is_success() {
            return Err(AppError::Model(format!(
                "Download failed: HTTP {} for immutable model artifact",
                response.status()
            )));
        }

        let total_bytes = response.content_length().unwrap_or(spec.size_bytes);
        if total_bytes > MAX_DOWNLOAD_BYTES {
            return Err(AppError::Model(format!(
                "File size ({total_bytes} bytes) exceeds maximum allowed ({MAX_DOWNLOAD_BYTES} bytes)"
            )));
        }
        if total_bytes != spec.size_bytes {
            return Err(AppError::Model(format!(
                "Integrity metadata mismatch for '{}': expected {} bytes, server reported {}",
                spec.model_id, spec.size_bytes, total_bytes
            )));
        }

        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part_path)
            .await
            .map_err(|e| AppError::Model(format!("Failed to create temporary model file: {e}")))?;
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
                    return Err(AppError::Model(format!("Download stream error: {e}")));
                }
            };
            if self.cancel_flag.load(Ordering::Relaxed) {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                self.emit_progress(
                    app_handle,
                    model_id,
                    downloaded_before + downloaded,
                    overall_total,
                    DownloadStatus::Cancelled,
                );
                return Err(AppError::Model("Download cancelled".into()));
            }

            if let Err(e) = file.write_all(&chunk).await {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                return Err(AppError::Model(format!("Write error: {e}")));
            }
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;
            if downloaded > MAX_DOWNLOAD_BYTES || downloaded > spec.size_bytes {
                drop(file);
                let _ = fs::remove_file(&part_path).await;
                return Err(AppError::Model(format!(
                    "Downloaded bytes ({downloaded}) exceed the catalog manifest"
                )));
            }

            let percent =
                (((downloaded_before + downloaded) as f64 / overall_total as f64) * 100.0) as u32;
            if percent > last_emit_percent {
                last_emit_percent = percent;
                self.emit_progress(
                    app_handle,
                    model_id,
                    downloaded_before + downloaded,
                    overall_total,
                    DownloadStatus::Downloading,
                );
            }
        }

        file.flush().await.map_err(AppError::Io)?;
        file.sync_all().await.map_err(AppError::Io)?;
        drop(file);

        let actual_sha256 = sha256_hex(hasher);
        if let Err(e) = verify_stream_result(spec, downloaded, &actual_sha256) {
            let _ = fs::remove_file(&part_path).await;
            self.emit_progress(
                app_handle,
                model_id,
                downloaded_before + downloaded,
                overall_total,
                DownloadStatus::Failed("integrity verification failed".into()),
            );
            return Err(AppError::Model(e));
        }

        if let Err(rename_error) = fs::rename(&part_path, &target_path).await {
            let _ = fs::remove_file(&part_path).await;
            // A concurrent verified download may have won the race.
            let root = root.to_path_buf();
            if target_path.exists() {
                return tokio::task::spawn_blocking(move || verify_or_migrate_file(&root, spec))
                    .await
                    .map_err(|e| AppError::Model(format!("Model verification task failed: {e}")))?
                    .map_err(AppError::Model);
            }
            return Err(AppError::Model(format!(
                "Failed to finalize download: {rename_error}"
            )));
        }
        write_verification_marker(&target_path, spec).map_err(AppError::Model)?;

        Ok(target_path)
    }

    pub fn cancel(&self) {
        self.cancel_flag.store(true, Ordering::SeqCst);
    }

    pub fn is_downloaded(&self, model_id: &str) -> bool {
        if let Some(ModelFamily::Parakeet) = ModelManager::family(model_id) {
            return directory_is_complete(&self.models_dir, model_id);
        }
        model_artifact(model_id)
            .map(|spec| expected_file_exists(&self.models_dir, spec))
            .unwrap_or(false)
    }

    pub fn model_path(&self, model_id: &str) -> Option<PathBuf> {
        if let Some(ModelFamily::Parakeet) = ModelManager::family(model_id) {
            return verified_model_directory(&self.models_dir, model_id).ok();
        }
        let spec = model_artifact(model_id).ok()?;
        verify_or_migrate_file(&self.models_dir, spec).ok()
    }

    pub async fn delete(&self, model_id: &str) -> AppResult<()> {
        if let Some(ModelFamily::Parakeet) = ModelManager::family(model_id) {
            let dir = model_directory_path(&self.models_dir, model_id)?;
            if dir.exists() {
                fs::remove_dir_all(&dir)
                    .await
                    .map_err(|e| AppError::Model(format!("Failed to delete: {e}")))?;
            }
            return Ok(());
        }
        let spec = model_artifact(model_id)?;
        let path = safe_artifact_path(&self.models_dir, spec.filename).map_err(AppError::Model)?;
        if path.exists() {
            fs::remove_file(&path)
                .await
                .map_err(|e| AppError::Model(format!("Failed to delete: {e}")))?;
        }
        remove_verification_marker(&path);
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

const WHISPER_CPP_REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";
const DISTIL_REVISION: &str = "0d78dd96ed9fc152325f63b53788fec3b43de031";
const DISTIL_V35_REVISION: &str = "960ecb5c2ecfba3ebb9ebe485c1032ec266cf436";
const PARAKEET_TDT_V2_REVISION: &str = "1ab9323565ddb038682214b292f588070a538ce2";

/// An immutable, integrity-pinned model that lives in its own directory.
///
/// Every file carries a full [`ArtifactSpec`], so each one is size- and
/// SHA-256-checked exactly like a single-file model; `dir_name` is the only
/// additional path element and is validated as a single component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DirectoryArtifact {
    pub dir_name: &'static str,
    pub files: &'static [ArtifactSpec],
}

impl DirectoryArtifact {
    /// Bytes the UI should treat as this model's whole download.
    pub fn total_bytes(self) -> u64 {
        self.files.iter().map(|spec| spec.size_bytes).sum()
    }
}

/// Upstream artifacts of the sherpa-onnx int8 export of NVIDIA
/// Parakeet-TDT-0.6B-v2, kept at their upstream filenames because
/// `ParakeetEngine::load` joins exactly these names.
const PARAKEET_TDT_V2_FILES: &[ArtifactSpec] = &[
    ArtifactSpec {
        model_id: "parakeet-tdt-0.6b-v2-int8",
        repository: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
        revision: PARAKEET_TDT_V2_REVISION,
        filename: "encoder.int8.onnx",
        size_bytes: 652_184_296,
        sha256: "a32b12d17bbbc309d0686fbbcc2987b5e9b8333a7da83fa6b089f0a2acd651ab",
    },
    ArtifactSpec {
        model_id: "parakeet-tdt-0.6b-v2-int8",
        repository: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
        revision: PARAKEET_TDT_V2_REVISION,
        filename: "decoder.int8.onnx",
        size_bytes: 7_257_753,
        sha256: "b6bb64963457237b900e496ee9994b59294526439fbcc1fecf705b31a15c6b4e",
    },
    ArtifactSpec {
        model_id: "parakeet-tdt-0.6b-v2-int8",
        repository: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
        revision: PARAKEET_TDT_V2_REVISION,
        filename: "joiner.int8.onnx",
        size_bytes: 1_739_080,
        sha256: "7946164367946e7f9f29a122407c3252b680dbae9a51343eb2488d057c3c43d2",
    },
    ArtifactSpec {
        model_id: "parakeet-tdt-0.6b-v2-int8",
        repository: "csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8",
        revision: PARAKEET_TDT_V2_REVISION,
        filename: "tokens.txt",
        size_bytes: 9_384,
        sha256: "ec182b70dd42113aff6c5372c75cac58c952443eb22322f57bbd7f53977d497d",
    },
];

/// Resolve a public catalog ID to its directory-backed artifact manifest.
pub(crate) fn model_directory_artifact(model_id: &str) -> AppResult<DirectoryArtifact> {
    match model_id {
        "parakeet-tdt-0.6b-v2-int8" => Ok(DirectoryArtifact {
            dir_name: "parakeet-tdt-0.6b-v2-int8",
            files: PARAKEET_TDT_V2_FILES,
        }),
        _ => Err(AppError::Model(format!(
            "Unknown directory model ID: '{model_id}'"
        ))),
    }
}

/// Contained path of a directory-backed model, whether or not it exists.
pub(crate) fn model_directory_path(root: &std::path::Path, model_id: &str) -> AppResult<PathBuf> {
    let artifact = model_directory_artifact(model_id)?;
    safe_model_dir(root, artifact.dir_name).map_err(AppError::Model)
}

/// A directory model counts as downloaded only when every artifact is present
/// at its exact manifest size — a partial set must never look installable.
pub(crate) fn directory_is_complete(root: &std::path::Path, model_id: &str) -> bool {
    let Ok(artifact) = model_directory_artifact(model_id) else {
        return false;
    };
    artifact_is_complete(root, artifact)
}

fn artifact_is_complete(root: &std::path::Path, artifact: DirectoryArtifact) -> bool {
    let Ok(dir) = safe_model_dir(root, artifact.dir_name) else {
        return false;
    };
    artifact
        .files
        .iter()
        .all(|spec| expected_file_exists(&dir, *spec))
}

/// Verify (or one-time migrate) every artifact and return the model directory.
/// Files staged by an earlier release are accepted here exactly as downloaded
/// ones are: same size check, same SHA-256, same verification marker.
pub(crate) fn verified_model_directory(
    root: &std::path::Path,
    model_id: &str,
) -> AppResult<PathBuf> {
    verify_artifact_directory(root, model_directory_artifact(model_id)?)
}

fn verify_artifact_directory(
    root: &std::path::Path,
    artifact: DirectoryArtifact,
) -> AppResult<PathBuf> {
    let dir = safe_model_dir(root, artifact.dir_name).map_err(AppError::Model)?;
    for spec in artifact.files {
        verify_or_migrate_file(&dir, *spec).map_err(AppError::Model)?;
    }
    Ok(dir)
}

/// Resolve a public catalog ID to an immutable, integrity-pinned artifact.
pub(crate) fn model_artifact(model_id: &str) -> AppResult<ArtifactSpec> {
    let (repository, revision, filename, size_bytes, sha256) = match model_id {
        "whisper-tiny-en" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-tiny.en.bin",
            77_704_715,
            "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        ),
        "whisper-base-en" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-base.en.bin",
            147_964_211,
            "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        ),
        "whisper-small-en" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-small.en.bin",
            487_614_201,
            "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        ),
        "whisper-medium-en" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-medium.en.bin",
            1_533_774_781,
            "cc37e93478338ec7700281a7ac30a10128929eb8f427dda2e865faa8f6da4356",
        ),
        "whisper-medium-en-q5" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-medium.en-q5_0.bin",
            539_225_533,
            "76733e26ad8fe1c7a5bf7531a9d41917b2adc0f20f2e4f5531688a8c6cd88eb0",
        ),
        "whisper-large-v3-turbo" | "whisper-large-v3-turbo-multi" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-large-v3-turbo.bin",
            1_624_555_275,
            "1fc70f774d38eb169993ac391eea357ef47c88757ef72ee5943879b7e8e2bc69",
        ),
        "whisper-large-v3-turbo-q5" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-large-v3-turbo-q5_0.bin",
            574_041_195,
            "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        ),
        "whisper-medium" => (
            "ggerganov/whisper.cpp",
            WHISPER_CPP_REVISION,
            "ggml-medium.bin",
            1_533_763_059,
            "6c14d5adee5f86394037b4e4e8b59f1673b6cee10e3cf0b11bbdbee79c156208",
        ),
        "whisper-distil-large-v3" => (
            "distil-whisper/distil-large-v3-ggml",
            DISTIL_REVISION,
            "ggml-distil-large-v3.bin",
            1_519_521_155,
            "2883a11b90fb10ed592d826edeaee7d2929bf1ab985109fe9e1e7b4d2b69a298",
        ),
        "whisper-distil-large-v3-5" => (
            "distil-whisper/distil-large-v3.5-ggml",
            DISTIL_V35_REVISION,
            "ggml-model.bin",
            1_519_521_155,
            "ec2498919b498c5f6b00041adb45650124b3cd9f26f545fffa8f5d11c28dcf26",
        ),
        _ => return Err(AppError::Model(format!("Unknown model ID: '{model_id}'"))),
    };

    let manifest_id = match model_id {
        "whisper-tiny-en" => "whisper-tiny-en",
        "whisper-base-en" => "whisper-base-en",
        "whisper-small-en" => "whisper-small-en",
        "whisper-medium-en" => "whisper-medium-en",
        "whisper-medium-en-q5" => "whisper-medium-en-q5",
        "whisper-large-v3-turbo" => "whisper-large-v3-turbo",
        "whisper-large-v3-turbo-q5" => "whisper-large-v3-turbo-q5",
        // Compatibility alias from older catalogs. Both IDs have always
        // resolved to the same multilingual upstream artifact.
        "whisper-large-v3-turbo-multi" => "whisper-large-v3-turbo",
        "whisper-medium" => "whisper-medium",
        "whisper-distil-large-v3" => "whisper-distil-large-v3",
        "whisper-distil-large-v3-5" => "whisper-distil-large-v3-5",
        _ => unreachable!("unknown model IDs return before artifact construction"),
    };

    Ok(ArtifactSpec {
        model_id: manifest_id,
        repository,
        revision,
        filename,
        size_bytes,
        sha256,
    })
}

/// Map a validated catalog ID to its local filename.
pub fn model_filename(model_id: &str) -> AppResult<&'static str> {
    Ok(model_artifact(model_id)?.filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_ids_use_immutable_revisions_and_sha256_values() {
        for model_id in [
            "whisper-tiny-en",
            "whisper-base-en",
            "whisper-small-en",
            "whisper-medium-en",
            "whisper-medium-en-q5",
            "whisper-large-v3-turbo",
            "whisper-large-v3-turbo-q5",
            "whisper-large-v3-turbo-multi",
            "whisper-medium",
            "whisper-distil-large-v3",
            "whisper-distil-large-v3-5",
        ] {
            let artifact = model_artifact(model_id).unwrap();
            assert_eq!(artifact.revision.len(), 40);
            assert_eq!(artifact.sha256.len(), 64);
            assert!(!artifact.download_url().contains("/resolve/main/"));
        }
    }

    #[test]
    fn directory_catalog_ids_pin_every_file_to_an_immutable_revision() {
        let artifact = model_directory_artifact("parakeet-tdt-0.6b-v2-int8").unwrap();
        assert_eq!(artifact.files.len(), 4);
        assert_eq!(artifact.total_bytes(), 661_190_513);
        for spec in artifact.files {
            assert_eq!(spec.revision.len(), 40);
            assert_eq!(spec.sha256.len(), 64);
            assert!(!spec.download_url().contains("/resolve/main/"));
            assert_eq!(spec.model_id, "parakeet-tdt-0.6b-v2-int8");
        }
        // The engine joins these upstream names verbatim.
        let names: Vec<&str> = artifact.files.iter().map(|spec| spec.filename).collect();
        assert_eq!(
            names,
            vec![
                "encoder.int8.onnx",
                "decoder.int8.onnx",
                "joiner.int8.onnx",
                "tokens.txt"
            ]
        );
    }

    /// Two-file stand-in for the real manifest so completeness can be checked
    /// without writing hundreds of megabytes. Both entries hash "hello world".
    const TEST_FILES: &[ArtifactSpec] = &[
        ArtifactSpec {
            model_id: "test-directory-model",
            repository: "example/test",
            revision: "0123456789012345678901234567890123456789",
            filename: "encoder.int8.onnx",
            size_bytes: 11,
            sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        },
        ArtifactSpec {
            model_id: "test-directory-model",
            repository: "example/test",
            revision: "0123456789012345678901234567890123456789",
            filename: "tokens.txt",
            size_bytes: 11,
            sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9",
        },
    ];
    const TEST_ARTIFACT: DirectoryArtifact = DirectoryArtifact {
        dir_name: "test-directory-model",
        files: TEST_FILES,
    };

    #[test]
    fn directory_models_require_every_artifact_before_they_count_as_downloaded() {
        let root = std::env::temp_dir().join(format!(
            "omnivox-directory-download-test-{}",
            uuid::Uuid::new_v4()
        ));
        let dir = root.join(TEST_ARTIFACT.dir_name);
        std::fs::create_dir_all(&dir).unwrap();

        assert!(!artifact_is_complete(&root, TEST_ARTIFACT));
        std::fs::write(dir.join("encoder.int8.onnx"), b"hello world").unwrap();
        assert!(!artifact_is_complete(&root, TEST_ARTIFACT));

        // Present but the wrong bytes: size passes, SHA-256 must still reject.
        std::fs::write(dir.join("tokens.txt"), b"hello rust!").unwrap();
        assert!(artifact_is_complete(&root, TEST_ARTIFACT));
        assert!(verify_artifact_directory(&root, TEST_ARTIFACT).is_err());

        std::fs::write(dir.join("tokens.txt"), b"hello world").unwrap();
        assert_eq!(
            verify_artifact_directory(&root, TEST_ARTIFACT).unwrap(),
            dir
        );

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn unknown_and_traversal_ids_are_rejected() {
        for model_id in [
            "unknown",
            "../settings.db",
            "..\\settings.db",
            "C:\\temp\\x",
        ] {
            assert!(model_filename(model_id).is_err(), "accepted {model_id:?}");
            assert!(model_artifact(model_id).is_err(), "accepted {model_id:?}");
            assert!(
                model_directory_artifact(model_id).is_err(),
                "accepted {model_id:?}"
            );
            assert!(!directory_is_complete(std::path::Path::new("."), model_id));
        }
    }
}
