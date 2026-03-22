use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Configuration for the local LLM inference engine.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Path to the GGUF model file on disk.
    pub model_path: PathBuf,
    /// Context window size in tokens. 512 is plenty for dictation cleanup.
    pub context_size: u32,
    /// Maximum tokens to generate in the response.  384 leaves room for
    /// formatted output (bullet lists) while staying well within the 512
    /// context window after the ~130-token system prompt.
    pub max_tokens: u32,
    /// Sampling temperature. Low (0.1) for deterministic cleanup output.
    pub temperature: f32,
}

impl LlmConfig {
    pub fn default_with_path(model_path: PathBuf) -> Self {
        Self {
            model_path,
            context_size: 512,
            max_tokens: 384,
            temperature: 0.1,
        }
    }
}

/// Status of the AI cleanup feature, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCleanupStatus {
    pub enabled: bool,
    pub model_downloaded: bool,
    pub model_loaded: bool,
}
