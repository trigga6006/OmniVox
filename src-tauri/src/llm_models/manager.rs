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
/// Qwen 1.7B Q4 is the currently validated extraction path. It is larger than
/// the earlier smaller-model experiments, but it is the model that is actually
/// producing stable slot JSON in the live pipeline.
pub const DEFAULT_LLM_ID: &str = "qwen3-1.7b-instruct-q4";

impl LlmModelManager {
    pub fn new(llm_models_dir: PathBuf) -> Self {
        Self {
            llm_models_dir,
            cache: Mutex::new(None),
        }
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
                let path = self.llm_models_dir.join(&m.huggingface_file);
                if path.exists() {
                    m.is_downloaded = true;
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
        self.list_available().into_iter().find(|m| m.id == model_id)
    }

    pub fn model_path(&self, model_id: &str) -> Option<PathBuf> {
        let info = self.get_model(model_id)?;
        let path = self.llm_models_dir.join(info.huggingface_file);
        if path.exists() { Some(path) } else { None }
    }

    pub fn delete(&self, model_id: &str) -> AppResult<()> {
        let info = self
            .catalog_entry(model_id)
            .ok_or_else(|| AppError::Llm(format!("Unknown LLM model: {model_id}")))?;
        let path = self.llm_models_dir.join(&info.huggingface_file);
        if path.exists() {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::Llm(format!("Failed to delete LLM: {e}")))?;
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
                id: "qwen3-0.6b-instruct-q4".into(),
                name: "Qwen3 0.6B Instruct (Q4)".into(),
                size_bytes: 400_000_000,
                quantization: "Q4_K_M".into(),
                context_length: 32_768,
                description: "Smaller Qwen option. Faster to download, but less reliable than 1.7B for structured extraction.".into(),
                huggingface_repo: "Qwen/Qwen3-0.6B-GGUF".into(),
                huggingface_file: "Qwen3-0.6B-Q4_K_M.gguf".into(),
                is_downloaded: false,
                path: None,
                is_default: false,
            },
            LlmModelInfo {
                id: "qwen3-1.7b-instruct-q4".into(),
                name: "Qwen3 1.7B Instruct (Q4)".into(),
                size_bytes: 1_000_000_000,
                quantization: "Q4_K_M".into(),
                context_length: 32_768,
                description: "Default. Best structure quality in the current pipeline, 16 GB RAM+ recommended.".into(),
                huggingface_repo: "Qwen/Qwen3-1.7B-GGUF".into(),
                huggingface_file: "Qwen3-1.7B-Q4_K_M.gguf".into(),
                is_downloaded: false,
                path: None,
                is_default: true,
            },
        ]
    }
}
