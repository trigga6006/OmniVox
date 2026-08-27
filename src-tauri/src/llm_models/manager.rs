use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::llm_models::types::{
    LlmCapabilityTier, LlmLanguageSupport, LlmModelInfo, LlmModelPurpose,
};
use crate::models::integrity::{
    expected_file_exists, remove_verification_marker, safe_artifact_path, verify_or_migrate_file,
    ArtifactSpec,
};

/// Catalog entry + download-status resolver for the LLM side of Structured Mode.
///
/// Deliberately parallel to `ModelManager` instead of shared — the two
/// catalogs carry different metadata (GGUF repos vs. whisper.cpp tiers) and
/// the downstream code would otherwise need `ModelKind` discrimination on
/// every call.  Duplication cost is small; coupling cost would be large.
pub struct LlmModelManager {
    llm_models_dir: PathBuf,
    cache: Mutex<Option<Vec<LlmModelInfo>>>,
}

/// Recommended starter LLM for first-time enable of Structured Mode.
///
/// Qwen 1.7B Q8 is the currently validated extraction path. It is larger than
/// the earlier smaller-model experiments, but it is the model that is actually
/// producing stable slot JSON in the live pipeline.
pub const DEFAULT_LLM_ID: &str = "qwen3-1.7b-instruct-q8";

/// Explicit metadata for an artifact from an older catalog version. This
/// avoids guessing quantization from a filename suffix.
#[derive(Clone, Copy)]
struct LegacyModelFile {
    artifact: ArtifactSpec,
    quantization: &'static str,
}

const QWEN_06_Q8: ArtifactSpec = ArtifactSpec {
    model_id: "qwen3-0.6b-instruct-q8",
    repository: "Qwen/Qwen3-0.6B-GGUF",
    revision: "23749fefcc72300e3a2ad315e1317431b06b590a",
    filename: "Qwen3-0.6B-Q8_0.gguf",
    size_bytes: 639_446_688,
    sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
};

const QWEN_17_Q8: ArtifactSpec = ArtifactSpec {
    model_id: "qwen3-1.7b-instruct-q8",
    repository: "Qwen/Qwen3-1.7B-GGUF",
    revision: "90862c4b9d2787eaed51d12237eafdfe7c5f6077",
    filename: "Qwen3-1.7B-Q8_0.gguf",
    size_bytes: 1_834_426_016,
    sha256: "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a",
};

/// Cleanup-stage normalizer. Fine-tuned from Qwen3-0.6B, so the vendored
/// llama.cpp loads it with the same `qwen3` architecture as the entries above.
const S1_MINI_Q4: ArtifactSpec = ArtifactSpec {
    model_id: "s1-mini-0.6b-q4",
    repository: "superwhisper/s1-mini-GGUF",
    revision: "8eab4779866f477ae6e7f237ca45fc2c65153f50",
    filename: "s1-mini-q4_k_m.gguf",
    size_bytes: 484_219_808,
    sha256: "3b41ebe2502cbd03e811d5d16b022f5ab551eda58d62597d152f89535003c634",
};

const QWEN_06_Q4_LEGACY: ArtifactSpec = ArtifactSpec {
    model_id: "qwen3-0.6b-instruct-legacy-q4",
    repository: "unsloth/Qwen3-0.6B-GGUF",
    revision: "c229f3161ad625e87e3240c144d2e3133eaa5eb5",
    filename: "Qwen3-0.6B-Q4_K_M.gguf",
    size_bytes: 396_705_472,
    sha256: "ac2d97712095a558e31573f62f466a3f9d93990898b0ec79d7c974c1780d524a",
};

const QWEN_17_Q4_LEGACY: ArtifactSpec = ArtifactSpec {
    model_id: "qwen3-1.7b-instruct-legacy-q4",
    repository: "unsloth/Qwen3-1.7B-GGUF",
    revision: "bd59ef4c1c7af8b7ade0d473f3ab0d48b9f1d338",
    filename: "Qwen3-1.7B-Q4_K_M.gguf",
    size_bytes: 1_107_409_472,
    sha256: "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897",
};

impl LlmModelManager {
    pub fn new(llm_models_dir: PathBuf) -> Self {
        Self {
            llm_models_dir,
            cache: Mutex::new(None),
        }
    }

    /// Map model IDs from older catalog versions to their current entries.
    ///
    /// v0.2.9 and earlier used `…-q4` IDs (Q4_K_M quants).  v0.2.10 renamed
    /// the catalog to the official Q8_0 files, which silently orphaned the
    /// persisted `active_llm_model_id` of anyone who had Structured Mode set
    /// up — every lazy-load then failed with "is not downloaded".
    pub fn canonical_id(model_id: &str) -> &str {
        match model_id {
            "qwen3-0.6b-instruct-q4" => "qwen3-0.6b-instruct-q8",
            "qwen3-1.7b-instruct-q4" => "qwen3-1.7b-instruct-q8",
            other => other,
        }
    }

    /// Filenames from older catalog versions that still satisfy an entry.
    /// Users who downloaded these before the v0.2.10 rename keep a working
    /// Structured Mode without re-downloading gigabytes.
    fn legacy_files(model_id: &str) -> &'static [LegacyModelFile] {
        match model_id {
            "qwen3-0.6b-instruct-q8" => &[LegacyModelFile {
                artifact: QWEN_06_Q4_LEGACY,
                quantization: "Q4_K_M",
            }],
            "qwen3-1.7b-instruct-q8" => &[LegacyModelFile {
                artifact: QWEN_17_Q4_LEGACY,
                quantization: "Q4_K_M",
            }],
            _ => &[],
        }
    }

    /// Immutable primary download artifact for a validated catalog ID.
    pub(crate) fn download_artifact(model_id: &str) -> AppResult<ArtifactSpec> {
        match Self::canonical_id(model_id) {
            "qwen3-0.6b-instruct-q8" => Ok(QWEN_06_Q8),
            "qwen3-1.7b-instruct-q8" => Ok(QWEN_17_Q8),
            "s1-mini-0.6b-q4" => Ok(S1_MINI_Q4),
            _ => Err(AppError::Llm(format!("Unknown LLM model: {model_id}"))),
        }
    }

    /// Resolve the on-disk GGUF and its integrity manifest. The primary file
    /// wins; known legacy quants remain loadable only when their original
    /// upstream digest and byte size match.
    fn resolve_file(&self, model_id: &str) -> Option<(PathBuf, ArtifactSpec)> {
        let primary = Self::download_artifact(model_id).ok()?;
        if expected_file_exists(&self.llm_models_dir, primary) {
            let path = safe_artifact_path(&self.llm_models_dir, primary.filename).ok()?;
            return Some((path, primary));
        }
        for file in Self::legacy_files(model_id) {
            if expected_file_exists(&self.llm_models_dir, file.artifact) {
                let path = safe_artifact_path(&self.llm_models_dir, file.artifact.filename).ok()?;
                return Some((path, file.artifact));
            }
        }
        None
    }

    /// List the catalog with download status resolved.  Cached until
    /// `invalidate_cache()` is called (post download/delete).
    pub fn list_available(&self) -> Vec<LlmModelInfo> {
        let mut cache = self.cache.lock().unwrap();
        if let Some(ref cached) = *cache {
            return cached.clone();
        }

        let models: Vec<LlmModelInfo> = Self::catalog()
            .into_iter()
            .map(|mut m| {
                if let Some((path, _artifact)) = self.resolve_file(&m.id) {
                    m.is_downloaded = true;
                    // Label honestly when a legacy quant is what's on disk.
                    if let Some(legacy) = Self::legacy_files(&m.id).iter().find(|legacy| {
                        path.file_name().and_then(|f| f.to_str()) == Some(legacy.artifact.filename)
                    }) {
                        m.quantization = format!("{} (legacy file)", legacy.quantization);
                    }
                    m.path = Some(path.to_string_lossy().into_owned());
                }
                m
            })
            .collect();

        *cache = Some(models.clone());
        models
    }

    pub fn invalidate_cache(&self) {
        *self.cache.lock().unwrap() = None;
    }

    pub fn get_model(&self, model_id: &str) -> Option<LlmModelInfo> {
        let model_id = Self::canonical_id(model_id);
        self.list_available().into_iter().find(|m| m.id == model_id)
    }

    pub fn model_path(&self, model_id: &str) -> Option<PathBuf> {
        let info = self.get_model(model_id)?;
        let (_path, artifact) = self.resolve_file(&info.id)?;
        verify_or_migrate_file(&self.llm_models_dir, artifact).ok()
    }

    pub fn delete(&self, model_id: &str) -> AppResult<()> {
        let model_id = Self::canonical_id(model_id);
        let info = self
            .catalog_entry(model_id)
            .ok_or_else(|| AppError::Llm(format!("Unknown LLM model: {model_id}")))?;
        // Remove the catalog file AND any legacy quants so delete actually
        // frees the disk space and resets the entry to "not downloaded".
        let primary = Self::download_artifact(model_id)?;
        debug_assert_eq!(primary.filename, info.huggingface_file);
        let mut artifacts = vec![primary];
        artifacts.extend(Self::legacy_files(model_id).iter().map(|f| f.artifact));
        for artifact in artifacts {
            let path = safe_artifact_path(&self.llm_models_dir, artifact.filename)
                .map_err(AppError::Llm)?;
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| AppError::Llm(format!("Failed to delete LLM: {e}")))?;
            }
            remove_verification_marker(&path);
        }
        self.invalidate_cache();
        Ok(())
    }

    fn catalog_entry(&self, model_id: &str) -> Option<LlmModelInfo> {
        Self::catalog().into_iter().find(|m| m.id == model_id)
    }

    /// Starter catalog — Qwen-only for the currently supported Structured Mode path.
    fn catalog() -> Vec<LlmModelInfo> {
        vec![
            LlmModelInfo {
                id: "qwen3-0.6b-instruct-q8".into(),
                name: "Qwen3 0.6B Instruct (Q8)".into(),
                size_bytes: 639_446_688,
                quantization: "Q8_0".into(),
                family: "qwen3".into(),
                parameter_count_millions: 600,
                language_support: LlmLanguageSupport::Multilingual,
                capability_tier: LlmCapabilityTier::Fast,
                purpose: LlmModelPurpose::Structured,
                estimated_memory_mb: 1_300,
                context_length: 32_768,
                description: "Smaller Qwen option. Faster to download, but less reliable than 1.7B for structured extraction.".into(),
                huggingface_repo: "Qwen/Qwen3-0.6B-GGUF".into(),
                huggingface_file: "Qwen3-0.6B-Q8_0.gguf".into(),
                is_downloaded: false,
                path: None,
                is_default: false,
            },
            LlmModelInfo {
                id: "qwen3-1.7b-instruct-q8".into(),
                name: "Qwen3 1.7B Instruct (Q8)".into(),
                size_bytes: 1_834_426_016,
                quantization: "Q8_0".into(),
                family: "qwen3".into(),
                parameter_count_millions: 1_700,
                language_support: LlmLanguageSupport::Multilingual,
                capability_tier: LlmCapabilityTier::Quality,
                purpose: LlmModelPurpose::Structured,
                estimated_memory_mb: 3_000,
                context_length: 32_768,
                description: "Default. Best structure quality in the current pipeline, 16 GB RAM+ recommended.".into(),
                huggingface_repo: "Qwen/Qwen3-1.7B-GGUF".into(),
                huggingface_file: "Qwen3-1.7B-Q8_0.gguf".into(),
                is_downloaded: false,
                path: None,
                is_default: true,
            },
            LlmModelInfo {
                id: "s1-mini-0.6b-q4".into(),
                name: "S1-mini by Superwhisper".into(),
                size_bytes: 484_219_808,
                quantization: "Q4_K_M".into(),
                family: "qwen3".into(),
                parameter_count_millions: 600,
                language_support: LlmLanguageSupport::Multilingual,
                capability_tier: LlmCapabilityTier::Fast,
                purpose: LlmModelPurpose::Cleanup,
                estimated_memory_mb: 1_100,
                context_length: 40_960,
                description: "Cleanup Mode only, English. Rewrites a dictation transcript as clean written text: fillers removed, self-corrections resolved, punctuation and capitalization applied, and spoken numbers, dates and email addresses written out.".into(),
                huggingface_repo: "superwhisper/s1-mini-GGUF".into(),
                huggingface_file: "s1-mini-q4_k_m.gguf".into(),
                is_downloaded: false,
                path: None,
                is_default: false,
            },
        ]
    }

    /// Pipeline stage a catalog ID may be activated for.  Unknown IDs report
    /// `Structured` so the existing "unknown model" errors stay the ones that
    /// surface, rather than a confusing purpose mismatch.
    pub fn purpose(model_id: &str) -> LlmModelPurpose {
        match Self::canonical_id(model_id) {
            "s1-mini-0.6b-q4" => LlmModelPurpose::Cleanup,
            _ => LlmModelPurpose::Structured,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LlmModelManager;
    use crate::llm_models::types::LlmModelPurpose;

    #[test]
    fn cleanup_catalog_entry_is_pinned_and_named_per_license() {
        let models = LlmModelManager::catalog();
        let s1 = models
            .iter()
            .find(|m| m.id == "s1-mini-0.6b-q4")
            .expect("S1-mini should be in catalog");

        // The license carries a naming clause: the model keeps its name,
        // "S1-mini" by "Superwhisper", with that exact capitalization.
        assert_eq!(s1.name, "S1-mini by Superwhisper");
        assert_eq!(s1.huggingface_repo, "superwhisper/s1-mini-GGUF");
        assert_eq!(s1.huggingface_file, "s1-mini-q4_k_m.gguf");
        assert_eq!(s1.purpose, LlmModelPurpose::Cleanup);
        assert_eq!(s1.quantization, "Q4_K_M");
        assert!(!s1.is_default);

        let artifact = LlmModelManager::download_artifact("s1-mini-0.6b-q4").unwrap();
        assert_eq!(artifact.size_bytes, s1.size_bytes);
        assert_eq!(artifact.revision.len(), 40);
        assert_eq!(artifact.sha256.len(), 64);
        assert!(!artifact.download_url().contains("/resolve/main/"));

        // Every other entry stays a Structured Mode extractor.
        for model in models.iter().filter(|m| m.id != "s1-mini-0.6b-q4") {
            assert_eq!(model.purpose, LlmModelPurpose::Structured);
        }
        assert_eq!(
            LlmModelManager::purpose("s1-mini-0.6b-q4"),
            LlmModelPurpose::Cleanup
        );
        assert_eq!(
            LlmModelManager::purpose("qwen3-1.7b-instruct-q4"),
            LlmModelPurpose::Structured
        );

        // Wire format the Settings UI matches on.
        let json = serde_json::to_value(s1).unwrap();
        assert_eq!(json["purpose"], "cleanup");
        assert_eq!(
            serde_json::to_value(LlmModelPurpose::Structured).unwrap(),
            "structured"
        );
    }

    #[test]
    fn qwen_catalog_uses_existing_official_gguf_filenames() {
        let models = LlmModelManager::catalog();

        let small = models
            .iter()
            .find(|m| m.id == "qwen3-0.6b-instruct-q8")
            .expect("0.6B model should be in catalog");
        assert_eq!(small.huggingface_file, "Qwen3-0.6B-Q8_0.gguf");
        assert_eq!(small.quantization, "Q8_0");

        let default = models
            .iter()
            .find(|m| m.id == "qwen3-1.7b-instruct-q8")
            .expect("1.7B model should be in catalog");
        assert_eq!(default.huggingface_file, "Qwen3-1.7B-Q8_0.gguf");
        assert_eq!(default.quantization, "Q8_0");

        for id in ["qwen3-0.6b-instruct-q8", "qwen3-1.7b-instruct-q8"] {
            let artifact = LlmModelManager::download_artifact(id).unwrap();
            assert_eq!(artifact.revision.len(), 40);
            assert_eq!(artifact.sha256.len(), 64);
            assert!(!artifact.download_url().contains("/resolve/main/"));
        }
    }

    #[test]
    fn legacy_q4_ids_map_to_current_entries() {
        assert_eq!(
            LlmModelManager::canonical_id("qwen3-1.7b-instruct-q4"),
            "qwen3-1.7b-instruct-q8"
        );
        assert_eq!(
            LlmModelManager::canonical_id("qwen3-0.6b-instruct-q4"),
            "qwen3-0.6b-instruct-q8"
        );
        // Current IDs and unknown IDs pass through untouched.
        assert_eq!(
            LlmModelManager::canonical_id("qwen3-1.7b-instruct-q8"),
            "qwen3-1.7b-instruct-q8"
        );
        assert_eq!(LlmModelManager::canonical_id("other"), "other");
    }

    #[test]
    fn corrupt_legacy_file_is_not_treated_as_downloaded() {
        // Regression: v0.2.10 renamed the catalog to Q8_0 filenames, which
        // orphaned previously-downloaded Q4_K_M files — Structured Mode then
        // failed every lazy-load with "is not downloaded".
        let dir =
            std::env::temp_dir().join(format!("omnivox-llm-mgr-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("Qwen3-1.7B-Q4_K_M.gguf");
        std::fs::write(&legacy, b"stub").unwrap();

        let mgr = LlmModelManager::new(dir.clone());

        // The old persisted ID resolves through canonical_id + legacy file.
        let info = mgr
            .get_model("qwen3-1.7b-instruct-q4")
            .expect("legacy id should resolve to current entry");
        assert_eq!(info.id, "qwen3-1.7b-instruct-q8");
        assert!(!info.is_downloaded);
        assert!(mgr.model_path("qwen3-1.7b-instruct-q4").is_none());

        // The 0.6B entry has no file at all — stays not-downloaded.
        let small = mgr.get_model("qwen3-0.6b-instruct-q8").unwrap();
        assert!(!small.is_downloaded);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn filesystem_operations_reject_unknown_and_traversal_ids() {
        let dir =
            std::env::temp_dir().join(format!("omnivox-llm-mgr-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let manager = LlmModelManager::new(dir.clone());

        for id in [
            "unknown",
            "../settings.db",
            "..\\settings.db",
            "C:\\temp\\x",
        ] {
            assert!(manager.model_path(id).is_none());
            assert!(manager.delete(id).is_err());
        }

        std::fs::remove_dir_all(dir).ok();
    }
}
