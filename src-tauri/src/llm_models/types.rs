use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmLanguageSupport {
    Multilingual,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmCapabilityTier {
    Fast,
    Quality,
}

/// Which pipeline stage a catalog entry is usable for.
///
/// The two purposes are NOT interchangeable: Structured Mode drives its model
/// with GBNF-constrained JSON prompts, while a cleanup model is a single-task
/// text normalizer that only understands its own documented prompt format.
/// Activation paths key off this so a cleanup model can never be installed as
/// the Structured Mode extractor (and vice versa).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmModelPurpose {
    Structured,
    Cleanup,
}

/// Catalog entry for a downloadable LLM model.
///
/// Mirrors `ModelInfo` for Whisper but with LLM-specific fields (quantization
/// labels are GGUF-style, huggingface_repo/file map to the upstream GGUF
/// hosting location).  Kept as a separate type instead of a union with
/// `ModelInfo` because the two catalogs carry different metadata and are
/// surfaced in different parts of the Settings UI.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmModelInfo {
    pub id: String,
    pub name: String,
    pub size_bytes: u64,
    pub quantization: String,
    /// Stable catalog metadata; do not infer these fields from the ID or file
    /// suffix at call sites.
    pub family: String,
    pub parameter_count_millions: u32,
    pub language_support: LlmLanguageSupport,
    pub capability_tier: LlmCapabilityTier,
    /// Which pipeline stage this entry may be activated for.
    pub purpose: LlmModelPurpose,
    /// Approximate RAM budget for weights, a 4k context, and native runtime
    /// overhead. It is intentionally larger than `size_bytes`.
    pub estimated_memory_mb: u64,
    /// Context window the model was trained with (tokens).
    pub context_length: u32,
    pub description: String,
    pub huggingface_repo: String,
    pub huggingface_file: String,
    pub is_downloaded: bool,
    pub path: Option<String>,
    /// True for the recommended starter model — highlighted in the UI.
    pub is_default: bool,
}

/// Download status for a single LLM file.  Mirrors the Whisper download
/// progress but travels over a separate event channel (`llm-download-progress`)
/// so existing `download-progress` listeners don't see double events.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LlmDownloadStatus {
    Downloading,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmDownloadProgress {
    pub model_id: String,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub progress_percent: f32,
    pub status: LlmDownloadStatus,
}
