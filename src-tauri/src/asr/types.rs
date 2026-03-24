use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
    pub duration_ms: u64,
    pub model_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsrConfig {
    pub model_path: String,
    pub language: Option<String>,
    pub translate: bool,
    pub n_threads: u32,
    /// Enable GPU acceleration via Vulkan/CUDA (requires compile-time feature).
    pub use_gpu: bool,
    /// Optional initial prompt to bias Whisper toward specific vocabulary.
    /// Useful for domain-specific terms (e.g. programming keywords) that
    /// Whisper might otherwise mis-transcribe.
    pub initial_prompt: Option<String>,
    /// Beam search size. None = default (5). 1 = greedy decoding.
    /// Higher values explore more decoding paths at the cost of latency.
    pub beam_size: Option<u32>,
    /// Initial temperature for decoding. None = default (0.0 = deterministic).
    pub temperature: Option<f32>,
    /// Temperature increment for fallback on low-confidence segments.
    /// None = default (0.2). Whisper retries at increasing temperatures when
    /// a segment has low average log probability or high compression ratio.
    pub temperature_inc: Option<f32>,
}

impl Default for AsrConfig {
    fn default() -> Self {
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get() as u32)
            .unwrap_or(4);
        Self {
            model_path: String::new(),
            language: None,
            translate: false,
            n_threads,
            use_gpu: false,
            initial_prompt: None,
            beam_size: None,
            temperature: None,
            temperature_inc: None,
        }
    }
}
