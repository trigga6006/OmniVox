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
}

impl Default for AsrConfig {
    fn default() -> Self {
        Self {
            model_path: String::new(),
            language: None,
            translate: false,
            n_threads: 4,
        }
    }
}
