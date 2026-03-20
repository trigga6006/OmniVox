use std::path::Path;

use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::asr::types::{AsrConfig, TranscriptionResult, TranscriptionSegment};
use crate::error::{AppError, AppResult};

/// Speech-to-text engine trait.
///
/// Implemented by WhisperEngine; trait exists so the pipeline can be tested
/// with a mock engine that doesn't require a real model file.
pub trait AsrEngine: Send + Sync {
    fn transcribe(&self, audio: &[f32]) -> AppResult<TranscriptionResult>;
}

/// Production Whisper.cpp engine via whisper-rs.
///
/// Loads a GGML model into memory once, then creates a lightweight inference
/// state per transcription call. Thread-safe: multiple Tauri commands can
/// call `transcribe` concurrently (whisper-rs uses Arc internally).
pub struct WhisperEngine {
    ctx: WhisperContext,
    config: AsrConfig,
}

// SAFETY: WhisperContext holds read-only model weights after construction.
// Each transcription call creates its own WhisperState used locally and dropped
// within the same call. No mutable shared state is accessed across threads.
unsafe impl Send for WhisperEngine {}
unsafe impl Sync for WhisperEngine {}

impl WhisperEngine {
    /// Load a GGML model file from disk.
    ///
    /// This is expensive (1–3 s for base, longer for large). Call once at
    /// startup or when the user switches models, not per-transcription.
    pub fn load(config: AsrConfig) -> AppResult<Self> {
        let path = &config.model_path;
        if !Path::new(path).exists() {
            return Err(AppError::Asr(format!("Model file not found: {path}")));
        }

        let ctx_params = WhisperContextParameters::default();
        let ctx = WhisperContext::new_with_params(path, ctx_params)
            .map_err(|e| AppError::Asr(format!("Failed to load model '{path}': {e}")))?;

        Ok(Self { ctx, config })
    }
}

impl AsrEngine for WhisperEngine {
    fn transcribe(&self, audio: &[f32]) -> AppResult<TranscriptionResult> {
        if audio.is_empty() {
            return Ok(TranscriptionResult {
                text: String::new(),
                segments: vec![],
                duration_ms: 0,
                model_name: self.config.model_path.clone(),
            });
        }

        // Each call gets its own state — cheap to create, owns the decode buffers.
        let mut state = self
            .ctx
            .create_state()
            .map_err(|e| AppError::Asr(format!("Failed to create state: {e}")))?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        // Language: Some("en") forces English, None or Some("auto") auto-detects.
        match self.config.language.as_deref() {
            Some("auto") | None => {} // whisper defaults to auto-detect
            Some(lang) => params.set_language(Some(lang)),
        }

        params.set_translate(self.config.translate);
        params.set_n_threads(self.config.n_threads as i32);

        // Silence whisper.cpp's own stdout output
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);

        // Reduce hallucinations on silence / low-energy audio
        params.set_suppress_blank(true);
        params.set_suppress_non_speech_tokens(true);

        // Run inference — this is CPU-bound and blocks
        state
            .full(params, audio)
            .map_err(|e| AppError::Asr(format!("Inference failed: {e}")))?;

        // Extract segments
        let n_segments = state
            .full_n_segments()
            .map_err(|e| AppError::Asr(format!("Failed to read segments: {e}")))?;

        let mut segments = Vec::with_capacity(n_segments as usize);
        let mut full_text = String::new();

        for i in 0..n_segments {
            let text = state
                .full_get_segment_text(i)
                .map_err(|e| AppError::Asr(format!("Segment {i} text: {e}")))?;
            let t0 = state
                .full_get_segment_t0(i)
                .map_err(|e| AppError::Asr(format!("Segment {i} t0: {e}")))?;
            let t1 = state
                .full_get_segment_t1(i)
                .map_err(|e| AppError::Asr(format!("Segment {i} t1: {e}")))?;

            full_text.push_str(&text);

            segments.push(TranscriptionSegment {
                start_ms: (t0 as u64) * 10, // whisper timestamps are 10 ms units
                end_ms: (t1 as u64) * 10,
                text,
                confidence: 0.0, // whisper.cpp doesn't expose per-segment confidence
            });
        }

        let duration_ms = (audio.len() as f64 / 16_000.0 * 1000.0) as u64;

        Ok(TranscriptionResult {
            text: full_text.trim().to_string(),
            segments,
            duration_ms,
            model_name: self.config.model_path.clone(),
        })
    }
}
