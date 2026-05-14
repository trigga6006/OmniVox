use std::path::Path;
use std::sync::RwLock;

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

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
    /// Hot-swappable initial prompt override. When set, takes precedence over
    /// `config.initial_prompt` for the next transcription call. Updated when
    /// vocabulary or dictionary entries change, avoiding a full model reload.
    prompt_override: RwLock<Option<String>>,
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

        let mut ctx_params = WhisperContextParameters::default();
        // Flash attention reduces memory bandwidth and speeds up inference
        // (5–15% on CPU, more with GPU). Safe to enable since we don't use DTW.
        ctx_params.flash_attn(true);
        // GPU offload via Vulkan/CUDA when the user enables it in settings.
        // Only effective when the binary is compiled with the `vulkan` or `cuda` feature.
        ctx_params.use_gpu(config.use_gpu);
        let ctx = WhisperContext::new_with_params(path, ctx_params)
            .map_err(|e| AppError::Asr(format!("Failed to load model '{path}': {e}")))?;

        // Prove the first real transcription can allocate its decode state
        // while we are still in the loader's GPU/CPU fallback path.
        let state_probe = ctx
            .create_state()
            .map_err(|e| AppError::Asr(format!("Failed to allocate decode state: {e}")))?;
        drop(state_probe);

        Ok(Self {
            ctx,
            config,
            prompt_override: RwLock::new(None),
        })
    }
}

impl WhisperEngine {
    /// Update the initial prompt at runtime without reloading the model.
    ///
    /// Called when vocabulary or dictionary entries change. The new prompt
    /// takes effect on the very next `transcribe()` call.
    pub fn set_initial_prompt(&self, prompt: Option<String>) {
        if let Ok(mut guard) = self.prompt_override.write() {
            *guard = prompt;
        }
    }

    /// Snapshot the currently-set initial prompt override.
    ///
    /// Used by the screen-context capture path to save the user's vocabulary
    /// prompt before temporarily merging in dynamic screen tokens, then
    /// restore it after transcription so subsequent calls aren't biased by
    /// stale screen content.
    pub fn get_initial_prompt(&self) -> Option<String> {
        self.prompt_override.read().ok().and_then(|g| g.clone())
    }

    /// Allocate a fresh `WhisperState` suitable for live preview transcription.
    ///
    /// The state holds the decode tensors (~500 MB for medium models) that
    /// `whisper_full` reuses between calls.  Callers should create ONE state
    /// at the start of a preview session and reuse it across all subsequent
    /// `transcribe_preview_with_state` calls — otherwise every preview tick
    /// re-allocates half a gigabyte, which on 16 GB machines caused user-
    /// visible memory pressure and allocator stalls.
    ///
    /// `WhisperState` owns its context handle via `Arc<WhisperInnerContext>`
    /// internally, so it's fully self-contained (no lifetime parameter) and
    /// safely Send + Sync — it can travel across threads freely.
    pub fn create_preview_state(&self) -> AppResult<WhisperState> {
        self.ctx
            .create_state()
            .map_err(|e| AppError::Asr(format!("Failed to create preview state: {e}")))
    }

    /// Run greedy transcription using a caller-supplied, reused `WhisperState`.
    ///
    /// Uses greedy decoding (beam_size=1), no temperature fallback, no initial
    /// prompt — optimized for speed over accuracy.  Because `state.full` is
    /// designed to be called repeatedly on the same state (whisper.cpp resets
    /// per-call decode buffers internally), reusing the state across
    /// iterations is both safe and ~10-30× cheaper than recreating it.
    pub fn transcribe_preview_with_state(
        &self,
        state: &mut WhisperState,
        audio: &[f32],
    ) -> AppResult<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        match self.config.language.as_deref() {
            Some("auto") | None => {}
            Some(lang) => params.set_language(Some(lang)),
        }

        params.set_translate(false);
        params.set_n_threads(self.config.n_threads as i32);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);

        // No temperature fallback — deterministic single-pass for speed
        params.set_temperature(0.0);
        params.set_temperature_inc(0.0);
        params.set_no_speech_thold(0.6);

        state
            .full(params, audio)
            .map_err(|e| AppError::Asr(format!("Preview inference failed: {e}")))?;

        let mut text = String::new();
        for i in 0..state.full_n_segments() {
            if let Some(seg) = state.get_segment(i) {
                if let Ok(s) = seg.to_str_lossy() {
                    text.push_str(&s);
                }
            }
        }

        Ok(text.trim().to_string())
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

        // Select decoding strategy: beam search (default, better accuracy)
        // or greedy (faster, lower quality). beam_size=1 falls back to greedy.
        let beam_size = self.config.beam_size.unwrap_or(5);
        let strategy = if beam_size <= 1 {
            SamplingStrategy::Greedy { best_of: 1 }
        } else {
            SamplingStrategy::BeamSearch {
                beam_size: beam_size as std::ffi::c_int,
                patience: -1.0, // -1.0 = whisper.cpp default (1.0)
            }
        };
        let mut params = FullParams::new(strategy);

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
        params.set_suppress_nst(true);

        // Temperature fallback: start deterministic, increment on low-confidence
        // segments. This is Whisper's reference behavior — the decoder retries at
        // increasing temperatures when a segment has high entropy or low log prob.
        let temp = self.config.temperature.unwrap_or(0.0);
        let temp_inc = self.config.temperature_inc.unwrap_or(0.2);
        params.set_temperature(temp);
        params.set_temperature_inc(temp_inc);
        params.set_entropy_thold(2.4);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(0.6);

        // Bias Whisper toward domain-specific vocabulary (e.g. programming
        // terms) so it recognizes them on the first pass rather than relying
        // on dictionary post-processing.
        // Check the hot-swappable override first (updated when vocab/dictionary
        // entries change), falling back to the config set at model load time.
        let override_prompt = self
            .prompt_override
            .read()
            .ok()
            .and_then(|g| g.clone());
        let effective_prompt = override_prompt
            .or_else(|| self.config.initial_prompt.clone());
        if let Some(ref prompt) = effective_prompt {
            params.set_initial_prompt(prompt);
        }

        // Run inference — this is CPU-bound and blocks
        state
            .full(params, audio)
            .map_err(|e| AppError::Asr(format!("Inference failed: {e}")))?;

        // Extract segments
        let n_segments = state.full_n_segments();

        let mut segments = Vec::with_capacity(n_segments as usize);
        let mut full_text = String::new();

        for i in 0..n_segments {
            let seg = match state.get_segment(i) {
                Some(s) => s,
                None => continue,
            };

            let text = seg.to_str_lossy()
                .unwrap_or_else(|_| std::borrow::Cow::Borrowed(""))
                .into_owned();

            full_text.push_str(&text);

            segments.push(TranscriptionSegment {
                start_ms: (seg.start_timestamp() as u64) * 10, // whisper timestamps are centiseconds
                end_ms: (seg.end_timestamp() as u64) * 10,
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
