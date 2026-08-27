use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::models::downloader::{
    directory_is_complete, model_artifact, model_directory_path, verified_model_directory,
};
use crate::models::integrity::{
    expected_file_exists, remove_verification_marker, safe_artifact_path, verify_or_migrate_file,
};
use crate::models::types::{
    AsrCapabilityTier, ComputeBackend, HardwareProfile, ModelCapabilityTier, ModelFamily,
    ModelInfo, ModelLanguageSupport,
};

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

    /// Normalize IDs persisted by older releases. Large V3 Turbo is a single
    /// multilingual upstream artifact; the former `-multi` row was only an
    /// alias for the same bytes and decode behavior.
    pub fn canonical_id(model_id: &str) -> &str {
        match model_id {
            "whisper-large-v3-turbo-multi" => "whisper-large-v3-turbo",
            other => other,
        }
    }

    /// Stable language capability from the catalog. Runtime decode policy
    /// must use this instead of guessing from model ID suffixes.
    pub fn language_support(model_id: &str) -> Option<ModelLanguageSupport> {
        let canonical = Self::canonical_id(model_id);
        Self::catalog()
            .into_iter()
            .find(|model| model.id == canonical)
            .map(|model| model.language_support)
    }

    /// Stable inference backend from the catalog. The load path and every
    /// on-disk layout decision key off this, so — like `language_support` —
    /// it is never inferred from the ID's shape.
    ///
    /// A static match instead of scanning `catalog()`: this is one of the
    /// hottest per-model lookups (every hardware profile and download-status
    /// check runs it), and `catalog()` rebuilds a fresh `Vec<ModelInfo>` of
    /// owned `String`s on every call. `catalog_families_match_the_artifact_manifests`
    /// cross-checks every catalog row against this match, so a new row that
    /// misses it fails a test instead of silently returning `None`.
    pub fn family(model_id: &str) -> Option<ModelFamily> {
        let canonical = Self::canonical_id(model_id);
        match canonical {
            "whisper-tiny-en"
            | "whisper-base-en"
            | "whisper-small-en"
            | "whisper-medium-en"
            | "whisper-medium-en-q5"
            | "whisper-large-v3-turbo"
            | "whisper-large-v3-turbo-q5"
            | "whisper-medium"
            | "whisper-distil-large-v3"
            | "whisper-distil-large-v3-5" => Some(ModelFamily::Whisper),
            "parakeet-tdt-0.6b-v2-int8" => Some(ModelFamily::Parakeet),
            _ => None,
        }
    }

    /// Full catalog using the best local information available to this layer.
    /// Runtime code should prefer [`Self::list_available_for_profile`] after it
    /// has probed its active inference backend.
    pub fn list_available(&self) -> Vec<ModelInfo> {
        let logical_cores = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(0);
        self.list_available_for_profile(&HardwareProfile::from_logical_cores(logical_cores))
    }

    /// Full catalog with download status and a recommendation tailored to a
    /// typed hardware profile. The cache excludes the dynamic recommendation,
    /// so a later benchmark cannot be hidden behind a stale cached value.
    pub fn list_available_for_profile(&self, profile: &HardwareProfile) -> Vec<ModelInfo> {
        let mut cache = self.cache.lock().unwrap();
        let base_models = match *cache {
            Some(ref cached) => cached.clone(),
            None => {
                let models: Vec<ModelInfo> = Self::catalog()
                    .into_iter()
                    .map(|mut m| {
                        // Every UI catalog entry must have an immutable artifact
                        // manifest. A mismatch is a programming error, not a
                        // user-controlled fallback filename.
                        let (path, downloaded) = match m.family {
                            ModelFamily::Whisper => {
                                let spec = model_artifact(&m.id)
                                    .expect("ASR catalog entry is missing an artifact manifest");
                                let path = safe_artifact_path(&self.models_dir, spec.filename)
                                    .expect("ASR artifact manifest contains an unsafe filename");
                                let downloaded = expected_file_exists(&self.models_dir, spec);
                                (path, downloaded)
                            }
                            ModelFamily::Parakeet => {
                                let path = model_directory_path(&self.models_dir, &m.id)
                                    .expect("ASR catalog entry is missing an artifact manifest");
                                let downloaded = directory_is_complete(&self.models_dir, &m.id);
                                (path, downloaded)
                            }
                        };
                        if downloaded {
                            m.is_downloaded = true;
                            m.path = Some(path.to_string_lossy().into_owned());
                        }
                        m
                    })
                    .collect();
                *cache = Some(models.clone());
                models
            }
        };

        let recommended_id = Self::recommend_for_profile(profile);
        base_models
            .into_iter()
            .map(|mut model| {
                model.recommended = model.id == recommended_id;
                model
            })
            .collect()
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

    /// Delete a downloaded model from disk.
    pub fn delete(&self, model_id: &str) -> AppResult<()> {
        let model_id = Self::canonical_id(model_id);
        if let Some(ModelFamily::Parakeet) = Self::family(model_id) {
            // The whole directory goes, including the per-file verification
            // markers that live beside the artifacts.
            let dir = model_directory_path(&self.models_dir, model_id)?;
            if dir.exists() {
                std::fs::remove_dir_all(&dir)
                    .map_err(|e| AppError::Model(format!("Failed to delete model: {e}")))?;
            }
            self.invalidate_cache();
            return Ok(());
        }
        let spec = model_artifact(model_id)?;
        let path = safe_artifact_path(&self.models_dir, spec.filename).map_err(AppError::Model)?;
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::Model(format!("Failed to delete model: {e}")))?;
        }
        remove_verification_marker(&path);
        self.invalidate_cache();
        Ok(())
    }

    /// Look up a model by ID, resolving its download status.
    pub fn get_model(&self, model_id: &str) -> Option<ModelInfo> {
        let model_id = Self::canonical_id(model_id);
        self.list_available().into_iter().find(|m| m.id == model_id)
    }

    /// Get the on-disk path for a model, if downloaded. Directory-backed models
    /// resolve to their directory, and only once every artifact verifies.
    pub fn model_path(&self, model_id: &str) -> Option<PathBuf> {
        let model_id = Self::canonical_id(model_id);
        if let Some(ModelFamily::Parakeet) = Self::family(model_id) {
            return verified_model_directory(&self.models_dir, model_id).ok();
        }
        let spec = model_artifact(model_id).ok()?;
        verify_or_migrate_file(&self.models_dir, spec).ok()
    }

    /// Determine the recommended model based on available CPU cores.
    ///
    /// Philosophy: recommend the best model the hardware can run with
    /// acceptable latency for dictation (< 3 s inference on a 10 s clip).
    ///
    /// * 12+ cores → large-v3-turbo  (best accuracy, distilled decoder keeps speed up)
    /// * 8+ cores  → medium.en       (excellent accuracy, bundled default)
    /// * 4-7 cores → small.en        (good accuracy, moderate resources)
    /// * < 4 cores → base.en         (fast, lighter accuracy)
    ///
    /// Return the model suited to observed ASR capability first, then a
    /// conservative hardware heuristic. A GPU affects the choice only when a
    /// usable backend and trustworthy allocatable memory are both known.
    pub fn recommend_for_profile(profile: &HardwareProfile) -> &'static str {
        let tier = profile
            .measured_asr_tier
            .or_else(|| Self::heuristic_tier(profile));
        match tier {
            Some(AsrCapabilityTier::High) => "whisper-distil-large-v3",
            Some(AsrCapabilityTier::Balanced) => "whisper-medium-en-q5",
            Some(AsrCapabilityTier::Entry) | None => "whisper-base-en",
        }
    }

    fn heuristic_tier(profile: &HardwareProfile) -> Option<AsrCapabilityTier> {
        let logical = profile.logical_cpu_cores?;
        let ram = profile.ram_total_mb?;
        let physical = profile.physical_cpu_cores.unwrap_or(logical);
        let has_trusted_gpu = matches!(
            profile.compute_backend,
            ComputeBackend::Vulkan | ComputeBackend::Cuda | ComputeBackend::Metal
        ) && profile.gpu_vram_mb.unwrap_or(0) >= 6_000;

        if ram >= 16_000 && (has_trusted_gpu || (physical >= 8 && logical >= 12)) {
            Some(AsrCapabilityTier::High)
        } else if ram >= 12_000 && physical >= 4 && logical >= 6 {
            Some(AsrCapabilityTier::Balanced)
        } else {
            Some(AsrCapabilityTier::Entry)
        }
    }

    /// Compatibility shim for callers that have not yet collected a hardware
    /// profile. Core count alone says nothing about RAM, a usable GPU, or
    /// measured real-time factor, so this deliberately remains conservative.
    #[deprecated(note = "collect a HardwareProfile and use recommend_for_profile")]
    pub fn recommend_for_cores(cpu_cores: u32) -> &'static str {
        Self::recommend_for_profile(&HardwareProfile::from_logical_cores(cpu_cores))
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
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::Entry,
                estimated_memory_mb: 350,
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
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::Entry,
                estimated_memory_mb: 600,
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
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::Balanced,
                estimated_memory_mb: 1_100,
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
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 2_700,
                description: "Excellent accuracy with clear handling of technical terms and mumbled speech.".into(),
                is_downloaded: false,
                path: None,
                // The installer stopped shipping a model in-box when it was
                // slimmed to ~18 MB; `copy_bundled_resources` is a no-op with
                // no file in resources/, so claiming "Included" here would be
                // false on a fresh install.
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-medium-en-q5".into(),
                name: "Medium Q5 (English)".into(),
                size_bytes: 539_000_000,
                quantization: "q5_0".into(),
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::Balanced,
                estimated_memory_mb: 1_300,
                description: "Quantized medium model. Near-identical accuracy at ~1/3 the RAM. Best value for most users.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-large-v3-turbo".into(),
                name: "Large V3 Turbo (Multilingual \u{1f310})".into(),
                size_bytes: 1_620_000_000,
                quantization: "f16".into(),
                language_support: ModelLanguageSupport::Multilingual,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 2_900,
                description: "Full large-v3 encoder with distilled decoder. Top-tier multilingual accuracy at medium-like speed.".into(),
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
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 1_500,
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
                language_support: ModelLanguageSupport::Multilingual,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 2_800,
                description: "99 languages with auto-detection. Same accuracy as Medium English for non-English dictation and bilingual workflows.".into(),
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
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 2_700,
                description: "5x faster than large-v3 with only 0.8% lower accuracy. Ideal for rapid-fire short dictations and real-time workflows.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
            ModelInfo {
                id: "whisper-distil-large-v3-5".into(),
                name: "Distil Large V3.5 (\u{26a1} Fast)".into(),
                size_bytes: 1_520_000_000,
                quantization: "f16".into(),
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Whisper,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 2_700,
                description: "Same 5x speed as Distil Large V3 with improved training for better accuracy. Ideal for rapid-fire short dictations and real-time workflows.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },

            // ── Parakeet (sherpa-onnx transducer) ───────────────
            // Not a Whisper model: activating this swaps the ASR engine.
            // Licensed CC-BY-4.0, so the display name keeps the NVIDIA
            // attribution.
            ModelInfo {
                id: "parakeet-tdt-0.6b-v2-int8".into(),
                name: "Parakeet TDT 0.6B V2 (NVIDIA)".into(),
                size_bytes: 661_190_513,
                quantization: "int8".into(),
                language_support: ModelLanguageSupport::English,
                family: ModelFamily::Parakeet,
                capability_tier: ModelCapabilityTier::High,
                estimated_memory_mb: 1_800,
                description: "State-of-the-art English accuracy — roughly 25% fewer errors than Large V3 Turbo. Runs int8 on CPU, punctuates and capitalizes natively, and does not hallucinate on silence.".into(),
                is_downloaded: false,
                path: None,
                bundled: false,
                recommended: false,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_is_conservative_without_memory_or_backend_metadata() {
        assert_eq!(
            ModelManager::recommend_for_profile(&HardwareProfile::from_logical_cores(32)),
            "whisper-base-en"
        );
    }

    #[test]
    fn low_end_profile_prefers_base() {
        let profile = HardwareProfile {
            physical_cpu_cores: Some(2),
            logical_cpu_cores: Some(4),
            ram_total_mb: Some(8_000),
            ..HardwareProfile::default()
        };
        assert_eq!(
            ModelManager::recommend_for_profile(&profile),
            "whisper-base-en"
        );
    }

    #[test]
    fn mid_range_profile_prefers_quantized_medium() {
        let profile = HardwareProfile {
            physical_cpu_cores: Some(6),
            logical_cpu_cores: Some(12),
            ram_total_mb: Some(16_000),
            ..HardwareProfile::default()
        };
        assert_eq!(
            ModelManager::recommend_for_profile(&profile),
            "whisper-medium-en-q5"
        );
    }

    #[test]
    fn high_profile_requires_trusted_backend_or_strong_cpu() {
        let profile = HardwareProfile {
            physical_cpu_cores: Some(6),
            logical_cpu_cores: Some(12),
            ram_total_mb: Some(32_000),
            gpu_name: Some("RX 7800 XT".into()),
            gpu_vram_mb: Some(15_000),
            compute_backend: ComputeBackend::Vulkan,
            ..HardwareProfile::default()
        };
        assert_eq!(
            ModelManager::recommend_for_profile(&profile),
            "whisper-distil-large-v3"
        );
    }

    #[test]
    fn benchmark_tier_overrides_optimistic_hardware_heuristic() {
        let profile = HardwareProfile {
            physical_cpu_cores: Some(12),
            logical_cpu_cores: Some(24),
            ram_total_mb: Some(32_000),
            measured_asr_tier: Some(AsrCapabilityTier::Entry),
            ..HardwareProfile::default()
        };
        assert_eq!(
            ModelManager::recommend_for_profile(&profile),
            "whisper-base-en"
        );
    }

    #[test]
    fn catalog_exposes_stable_capability_metadata() {
        let model = ModelManager::catalog()
            .into_iter()
            .find(|model| model.id == "whisper-medium-en-q5")
            .unwrap();
        assert_eq!(model.language_support, ModelLanguageSupport::English);
        assert_eq!(model.capability_tier, ModelCapabilityTier::Balanced);
        assert!(model.estimated_memory_mb > 0);

        assert_eq!(
            ModelManager::canonical_id("whisper-large-v3-turbo-multi"),
            "whisper-large-v3-turbo"
        );
        assert_eq!(
            ModelManager::language_support("whisper-large-v3-turbo-multi"),
            Some(ModelLanguageSupport::Multilingual)
        );
        assert_eq!(
            ModelManager::language_support("whisper-distil-large-v3"),
            Some(ModelLanguageSupport::English)
        );
        assert!(ModelManager::catalog()
            .iter()
            .all(|model| model.id != "whisper-large-v3-turbo-multi"));
    }

    #[test]
    fn catalog_families_match_the_artifact_manifests() {
        // The family decides which engine loads AND which on-disk layout is
        // read, so a row whose family disagrees with its manifest would either
        // fail to resolve a path or load into the wrong backend.
        for model in ModelManager::catalog() {
            match model.family {
                ModelFamily::Whisper => {
                    assert!(
                        model_artifact(&model.id).is_ok(),
                        "{} has no single-file manifest",
                        model.id
                    );
                }
                ModelFamily::Parakeet => {
                    assert!(
                        model_directory_path(std::path::Path::new("."), &model.id).is_ok(),
                        "{} has no directory manifest",
                        model.id
                    );
                }
            }
            // `family()` is a static match kept separate from `catalog()` for
            // performance — a new catalog row that isn't added to that match
            // must fail here instead of silently returning `None`.
            assert_eq!(
                ModelManager::family(&model.id),
                Some(model.family),
                "{} is missing from the ModelManager::family match",
                model.id
            );
        }

        assert_eq!(
            ModelManager::family("parakeet-tdt-0.6b-v2-int8"),
            Some(ModelFamily::Parakeet)
        );
        assert_eq!(
            ModelManager::family("whisper-medium-en"),
            Some(ModelFamily::Whisper)
        );
        assert_eq!(ModelManager::family("unknown"), None);

        let parakeet = ModelManager::catalog()
            .into_iter()
            .find(|model| model.id == "parakeet-tdt-0.6b-v2-int8")
            .unwrap();
        assert_eq!(parakeet.language_support, ModelLanguageSupport::English);
        assert_eq!(parakeet.capability_tier, ModelCapabilityTier::High);
        assert_eq!(parakeet.size_bytes, 661_190_513);
        assert!(parakeet.name.contains("NVIDIA"), "CC-BY-4.0 attribution");
    }

    #[test]
    fn family_serializes_as_the_lowercase_string_the_frontend_reads() {
        let rendered = serde_json::to_value(ModelManager::catalog()).unwrap();
        let families: Vec<&str> = rendered
            .as_array()
            .unwrap()
            .iter()
            .map(|model| model["family"].as_str().unwrap())
            .collect();
        assert!(families
            .iter()
            .all(|family| matches!(*family, "whisper" | "parakeet")));
        assert!(families.contains(&"whisper"));
        assert!(families.contains(&"parakeet"));
    }

    #[test]
    fn filesystem_operations_reject_unknown_and_traversal_ids() {
        let dir = std::env::temp_dir().join(format!(
            "omnivox-model-manager-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manager = ModelManager::new(dir.clone());

        for model_id in [
            "unknown",
            "../settings.db",
            "..\\settings.db",
            "C:\\temp\\x",
        ] {
            assert!(manager.model_path(model_id).is_none());
            assert!(manager.delete(model_id).is_err());
        }

        std::fs::remove_dir_all(dir).ok();
    }
}
