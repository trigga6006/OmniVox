use std::path::PathBuf;
use std::sync::Mutex;

use crate::error::{AppError, AppResult};
use crate::llm_models::types::LlmModelInfo;

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

/// Best-effort quant label from a GGUF filename
/// ("Qwen3-1.7B-Q4_K_M.gguf" → "Q4_K_M").
fn quant_from_path(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| s.split('-').next_back())
        .unwrap_or("unknown")
        .to_string()
}

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
    fn legacy_files(model_id: &str) -> &'static [&'static str] {
        match model_id {
            "qwen3-0.6b-instruct-q8" => &["Qwen3-0.6B-Q4_K_M.gguf"],
            "qwen3-1.7b-instruct-q8" => &["Qwen3-1.7B-Q4_K_M.gguf"],
            _ => &[],
        }
    }

    /// Resolve the on-disk GGUF for an entry: the current catalog file when
    /// present, otherwise a legacy quant from an older catalog version.
    fn resolve_file(&self, model_id: &str, huggingface_file: &str) -> Option<PathBuf> {
        let primary = self.llm_models_dir.join(huggingface_file);
        if primary.exists() {
            return Some(primary);
        }
        for file in Self::legacy_files(model_id) {
            let path = self.llm_models_dir.join(file);
            if path.exists() {
                return Some(path);
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
                if let Some(path) = self.resolve_file(&m.id, &m.huggingface_file) {
                    m.is_downloaded = true;
                    // Label honestly when a legacy quant is what's on disk.
                    if path.file_name().and_then(|f| f.to_str())
                        != Some(m.huggingface_file.as_str())
                    {
                        m.quantization = format!("{} (legacy file)", quant_from_path(&path));
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
        self.resolve_file(&info.id, &info.huggingface_file)
    }

    pub fn delete(&self, model_id: &str) -> AppResult<()> {
        let model_id = Self::canonical_id(model_id);
        let info = self
            .catalog_entry(model_id)
            .ok_or_else(|| AppError::Llm(format!("Unknown LLM model: {model_id}")))?;
        // Remove the catalog file AND any legacy quants so delete actually
        // frees the disk space and resets the entry to "not downloaded".
        let mut files: Vec<PathBuf> = vec![self.llm_models_dir.join(&info.huggingface_file)];
        files.extend(
            Self::legacy_files(model_id)
                .iter()
                .map(|f| self.llm_models_dir.join(f)),
        );
        for path in files {
            if path.exists() {
                std::fs::remove_file(&path)
                    .map_err(|e| AppError::Llm(format!("Failed to delete LLM: {e}")))?;
            }
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
                context_length: 32_768,
                description: "Default. Best structure quality in the current pipeline, 16 GB RAM+ recommended.".into(),
                huggingface_repo: "Qwen/Qwen3-1.7B-GGUF".into(),
                huggingface_file: "Qwen3-1.7B-Q8_0.gguf".into(),
                is_downloaded: false,
                path: None,
                is_default: true,
            },
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::LlmModelManager;

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
    fn legacy_q4_file_satisfies_renamed_catalog_entry() {
        // Regression: v0.2.10 renamed the catalog to Q8_0 filenames, which
        // orphaned previously-downloaded Q4_K_M files — Structured Mode then
        // failed every lazy-load with "is not downloaded".
        let dir = std::env::temp_dir().join(format!(
            "omnivox-llm-mgr-test-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let legacy = dir.join("Qwen3-1.7B-Q4_K_M.gguf");
        std::fs::write(&legacy, b"stub").unwrap();

        let mgr = LlmModelManager::new(dir.clone());

        // The old persisted ID resolves through canonical_id + legacy file.
        let info = mgr
            .get_model("qwen3-1.7b-instruct-q4")
            .expect("legacy id should resolve to current entry");
        assert_eq!(info.id, "qwen3-1.7b-instruct-q8");
        assert!(info.is_downloaded, "legacy file should count as downloaded");
        assert!(
            info.quantization.contains("Q4_K_M"),
            "quant label should reflect the actual on-disk file, got {}",
            info.quantization
        );

        let path = mgr
            .model_path("qwen3-1.7b-instruct-q4")
            .expect("model_path should resolve via the legacy file");
        assert_eq!(path, legacy);

        // The 0.6B entry has no file at all — stays not-downloaded.
        let small = mgr.get_model("qwen3-0.6b-instruct-q8").unwrap();
        assert!(!small.is_downloaded);

        std::fs::remove_dir_all(&dir).ok();
    }
}
