use serde::{Deserialize, Serialize};

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
