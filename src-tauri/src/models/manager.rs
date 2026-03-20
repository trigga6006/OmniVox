use std::path::PathBuf;

use crate::error::{AppError, AppResult};
use crate::models::downloader::model_filename;
use crate::models::types::ModelInfo;

/// The model that ships inside the installer. Available immediately on first launch.
pub const BUNDLED_MODEL_ID: &str = "whisper-base-en";

/// Model catalog and hardware-aware recommendation engine.
pub struct ModelManager {
    models_dir: PathBuf,
}

impl ModelManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self { models_dir }
    }

    /// Full catalog with download status, bundled flag, and hardware-aware
    /// recommendation resolved dynamically.
    pub fn list_available(&self) -> Vec<ModelInfo> {
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        let recommended_id = Self::recommend_for_cores(cpu_cores);

        Self::catalog()
            .into_iter()
            .map(|mut m| {
                let path = self.models_dir.join(model_filename(&m.id));
                if path.exists() {
                    m.is_downloaded = true;
                    m.path = Some(path.to_string_lossy().into_owned());
                }
                m.recommended = m.id == recommended_id;
                m
            })
            .collect()
    }

    /// Only models that exist on disk.
    pub fn get_downloaded(&self) -> Vec<ModelInfo> {
        self.list_available()
            .into_iter()
            .filter(|m| m.is_downloaded)
            .collect()
    }

    /// Delete a downloaded model file.
    pub fn delete(&self, model_id: &str) -> AppResult<()> {
        let path = self.models_dir.join(model_filename(model_id));
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::Model(format!("Failed to delete model: {e}")))?;
        }
        Ok(())
    }

    /// Look up a model by ID, resolving its download status.
    pub fn get_model(&self, model_id: &str) -> Option<ModelInfo> {
        self.list_available().into_iter().find(|m| m.id == model_id)
    }

    /// Get the on-disk path for a model, if downloaded.
    pub fn model_path(&self, model_id: &str) -> Option<PathBuf> {
        let path = self.models_dir.join(model_filename(model_id));
        if path.exists() {
            Some(path)
        } else {
            None
        }
    }

    /// Determine the recommended model based on available CPU cores.
    ///
    /// Philosophy: recommend the best model the hardware can run with
    /// acceptable latency for dictation (< 3 s inference on a 10 s clip).
    ///
    /// - 8+ cores  → small.en   (best accuracy, ~2-4 s on 8 cores)
    /// - 4-7 cores → base.en    (good accuracy, ~1-3 s on 4 cores — also the bundled model)
    /// - < 4 cores → tiny.en    (fast, lighter accuracy)
    pub fn recommend_for_cores(cpu_cores: u32) -> &'static str {
        if cpu_cores >= 8 {
            "whisper-small-en"
        } else if cpu_cores >= 4 {
            "whisper-base-en"
        } else {
            "whisper-tiny-en"
        }
    }

    /// The hardcoded model catalog.
    ///
    /// Focused on English-only GGML models for dictation performance.
    /// Multilingual variants are available as downloads but not highlighted.
    fn catalog() -> Vec<ModelInfo> {
        vec![
            ModelInfo {
                id: "whisper-tiny-en".into(),
                name: "Tiny".into(),
                size_bytes: 75_000_000,
                quantization: "f16".into(),
                description: "Fastest inference, lowest resource usage. Best for older hardware or when speed matters more than accuracy.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false, // set dynamically
            },
            ModelInfo {
                id: "whisper-base-en".into(),
                name: "Base".into(),
                size_bytes: 142_000_000,
                quantization: "f16".into(),
                description: "Ships with OmniVox. Excellent balance of speed and accuracy for everyday dictation.".into(),
                is_downloaded: false,
                path: None,
                bundled: true,
                recommended: false, // set dynamically
            },
            ModelInfo {
                id: "whisper-small-en".into(),
                name: "Small".into(),
                size_bytes: 466_000_000,
                quantization: "f16".into(),
                description: "Higher accuracy for complex vocabulary and technical terms. Recommended if you have 8+ CPU cores.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false, // set dynamically
            },
            ModelInfo {
                id: "whisper-medium-en".into(),
                name: "Medium".into(),
                size_bytes: 1_500_000_000,
                quantization: "f16".into(),
                description: "Near-maximum accuracy. Requires significant memory and processing time. For power users.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
        ]
    }
}
