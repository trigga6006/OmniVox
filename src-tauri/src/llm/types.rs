use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Configuration for the local LLM inference engine.
#[derive(Debug, Clone)]
pub struct LlmConfig {
    /// Path to the GGUF model file on disk.
    pub model_path: PathBuf,
    /// Context window size in tokens. 768 accommodates the system prompt,
    /// few-shot examples, and typical dictation input for the 1.7B model.
    pub context_size: u32,
    /// Maximum tokens to generate in the response.  384 leaves room for
    /// formatted output (bullet lists) while staying well within the
    /// context window after the system prompt + few-shot examples.
    pub max_tokens: u32,
    /// Sampling temperature. Low (0.1) for deterministic cleanup output.
    pub temperature: f32,
    /// Number of CPU threads for inference. Passed to the sidecar.
    pub n_threads: u32,
}

impl LlmConfig {
    pub fn default_with_path(model_path: PathBuf) -> Self {
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4)
            .min(8);
        Self {
            model_path,
            context_size: 768,
            max_tokens: 384,
            temperature: 0.1,
            n_threads,
        }
    }
}

/// Status of the AI cleanup feature, returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiCleanupStatus {
    pub enabled: bool,
    pub model_downloaded: bool,
    pub model_loaded: bool,
    /// True when the old (0.6B) model exists but the new (1.7B) model hasn't
    /// been downloaded yet — signals the frontend to show an upgrade prompt.
    pub upgrade_available: bool,
}
