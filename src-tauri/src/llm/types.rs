use serde::{Deserialize, Serialize};

/// Load-time configuration for the local LLM engine.
///
/// Temperature is deliberately NOT user-tunable — non-zero temperature combined
/// with GBNF grammar occasionally produces malformed JSON, which defeats the
/// whole point of the structured-mode design.  We hardcode `0.0` (greedy).
#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub model_path: String,
    /// Number of CPU threads for inference.
    pub n_threads: i32,
    /// Enable GPU offload via the compile-time `vulkan` / `cuda` feature.
    pub use_gpu: bool,
    /// Context window to allocate (tokens). The bundled Qwen models support
    /// far more than we need; Structured Mode keeps this lower because the
    /// grammar-constrained output is short and prompts are brief.
    pub n_ctx: u32,
    /// Max tokens the model may emit per extraction.  Keep this small because
    /// Structured Mode only needs a compact slot object, not long-form output.
    pub max_tokens: u32,
}

impl Default for LlmConfig {
    fn default() -> Self {
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).max(2).min(8) as i32)
            .unwrap_or(4);
        Self {
            model_path: String::new(),
            n_threads,
            use_gpu: false,
            n_ctx: 2048,
            // Enough headroom for a fully-populated JSON (goal + 3 array
            // slots with a few items each).  192 was too tight and would
            // cause the model to truncate mid-JSON on rich dictations.
            max_tokens: 384,
        }
    }
}

/// Raw JSON output of a single inference call, plus metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmInferenceResult {
    /// The JSON the LLM emitted (should always parse — GBNF enforces shape).
    pub raw_json: String,
    /// Wall-clock milliseconds the inference took.
    pub duration_ms: u64,
    /// Model filename (for debugging and history).
    pub model_name: String,
}
