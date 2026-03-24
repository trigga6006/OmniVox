use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::downloader::model_filename;
use crate::models::types::ModelInfo;

/// The model that ships inside the installer. Available immediately on first launch.
pub const BUNDLED_MODEL_ID: &str = "whisper-medium-en";

/// Model catalog and hardware-aware recommendation engine.
///
/// Caches the resolved model list to avoid repeated filesystem stat-checks.
/// Call `invalidate_cache()` after downloads or deletions.
pub struct ModelManager {
    models_dir: PathBuf,
    cache: Mutex<Option<Vec<ModelInfo>>>,
}

impl ModelManager {
    pub fn new(models_dir: PathBuf) -> Self {
        Self {
            models_dir,
            cache: Mutex::new(None),
        }
    }

    /// Full catalog with download status, bundled flag, and hardware-aware
    /// recommendation resolved dynamically. Results are cached until
    /// `invalidate_cache()` is called.
    pub fn list_available(&self) -> Vec<ModelInfo> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(ref cached) = *cache {
            return cached.clone();
        }

        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        let recommended_id = Self::recommend_for_cores(cpu_cores);

        let models: Vec<ModelInfo> = Self::catalog()
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
            .collect();

        *cache = Some(models.clone());
        models
    }

    /// Clear the cached model list so the next `list_available()` call
    /// re-checks the filesystem. Call after download/delete operations.
    pub fn invalidate_cache(&self) {
        *self.cache.lock().unwrap() = None;
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
        self.invalidate_cache();
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
    /// - 12+ cores → large-v3-turbo  (best accuracy, distilled decoder keeps speed up)
    /// - 8+ cores  → medium.en       (excellent accuracy, bundled default)
    /// - 4-7 cores → small.en        (good accuracy, moderate resources)
    /// - < 4 cores → base.en         (fast, lighter accuracy)
    pub fn recommend_for_cores(cpu_cores: u32) -> &'static str {
        if cpu_cores >= 12 {
            "whisper-large-v3-turbo"
        } else if cpu_cores >= 8 {
            "whisper-medium-en"
        } else if cpu_cores >= 4 {
            "whisper-small-en"
        } else {
            "whisper-base-en"
        }
    }

    /// The hardcoded model catalog.
    ///
    /// Organized by tier: English-only models first, then multilingual,
    /// then specialized (distilled).  Each entry is tagged in its name
    /// so users can tell at a glance what it supports.
    fn catalog() -> Vec<ModelInfo> {
        vec![
            // ── English-only models ─────────────────────────────
            ModelInfo {
                id: "whisper-tiny-en".into(),
                name: "Tiny (English)".into(),
                size_bytes: 75_000_000,
                quantization: "f16".into(),
                description: "Fastest inference, lowest resource usage. Best for older hardware or when speed matters more than accuracy.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-base-en".into(),
                name: "Base (English)".into(),
                size_bytes: 142_000_000,
                quantization: "f16".into(),
                description: "Good balance of speed and accuracy for everyday dictation.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-small-en".into(),
                name: "Small (English)".into(),
                size_bytes: 466_000_000,
                quantization: "f16".into(),
                description: "Higher accuracy for complex vocabulary and technical terms.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-medium-en".into(),
                name: "Medium (English)".into(),
                size_bytes: 1_500_000_000,
                quantization: "f16".into(),
                description: "Ships with OmniVox. Excellent accuracy with clear handling of technical terms and mumbled speech.".into(),
                is_downloaded: false,
                path: None,
                bundled: true,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-medium-en-q5".into(),
                name: "Medium Q5 (English)".into(),
                size_bytes: 539_000_000,
                quantization: "q5_0".into(),
                description: "Quantized medium model. Near-identical accuracy at ~1/3 the RAM. Best value for most users.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-large-v3-turbo".into(),
                name: "Large V3 Turbo (English)".into(),
                size_bytes: 1_620_000_000,
                quantization: "f16".into(),
                description: "Full large-v3 encoder with distilled decoder. Top-tier English accuracy at medium-like speed.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-large-v3-turbo-q5".into(),
                name: "Large V3 Turbo Q5 (English)".into(),
                size_bytes: 574_000_000,
                quantization: "q5_0".into(),
                description: "Quantized large-v3-turbo. Near-identical accuracy at ~1/3 the size. Great accuracy-to-resource ratio.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },

            // ── Multilingual models ─────────────────────────────
            // Auto-detect 99 languages. Use these for non-English dictation
            // or bilingual workflows. Also support translate-to-English mode.
            ModelInfo {
                id: "whisper-medium".into(),
                name: "Medium (Multilingual \u{1f310})".into(),
                size_bytes: 1_530_000_000,
                quantization: "f16".into(),
                description: "99 languages with auto-detection. Same accuracy as Medium English for non-English dictation and bilingual workflows.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-large-v3-turbo-multi".into(),
                name: "Large V3 Turbo (Multilingual \u{1f310})".into(),
                size_bytes: 1_620_000_000,
                quantization: "f16".into(),
                description: "Best multilingual accuracy. 99 languages with auto-detection, translation to English, and top-tier recognition quality.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },

            // ── Specialized models ──────────────────────────────
            ModelInfo {
                id: "whisper-distil-large-v3".into(),
                name: "Distil Large V3 (\u{26a1} Fast)".into(),
                size_bytes: 1_520_000_000,
                quantization: "f16".into(),
                description: "5x faster than large-v3 with only 0.8% lower accuracy. Ideal for rapid-fire short dictations and real-time workflows.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
        ]
    }
}
