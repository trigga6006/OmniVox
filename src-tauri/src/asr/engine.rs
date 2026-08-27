use std::ffi::c_void;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

use crate::asr::types::{
    AsrConfig, DecodePolicy, TranscriptionOptions, TranscriptionResult, TranscriptionSegment,
    TranscriptionTimings,
};
use crate::error::{AppError, AppResult};

/// Raw whisper.cpp abort callbacks used instead of
/// `FullParams::set_abort_callback_safe` from whisper-rs 0.16.0. That wrapper
/// stores a boxed trait object but casts it back to the concrete closure type in
/// its C trampoline, which can make an uncancelled inference abort with native
/// error -6. The `Arc<AtomicBool>` that owns each pointer remains in the calling
/// function's scope until the synchronous `WhisperState::full` call returns.
unsafe extern "C" fn abort_when_cancelled(user_data: *mut c_void) -> bool {
    if user_data.is_null() {
        return false;
    }
    // SAFETY: callers pass `Arc::as_ptr` for a live `Arc<AtomicBool>` and keep
    // that Arc alive until whisper.cpp has returned from its synchronous call.
    unsafe { (&*user_data.cast::<AtomicBool>()).load(Ordering::Acquire) }
}

unsafe extern "C" fn abort_when_recording_stops(user_data: *mut c_void) -> bool {
    if user_data.is_null() {
        return false;
    }
    // SAFETY: same pointer/lifetime contract as `abort_when_cancelled`.
    !unsafe { (&*user_data.cast::<AtomicBool>()).load(Ordering::Acquire) }
}

fn install_abort_callback(
    params: &mut FullParams<'_, '_>,
    flag: &Arc<AtomicBool>,
    callback: unsafe extern "C" fn(*mut c_void) -> bool,
) {
    // SAFETY: `flag` stays owned by the caller through `WhisperState::full`,
    // and both callbacks only perform an atomic load through this pointer.
    unsafe {
        params.set_abort_callback(Some(callback));
        params.set_abort_callback_user_data(Arc::as_ptr(flag).cast_mut().cast());
    }
}

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

/// Reusable decode state for a dedicated or otherwise serialized ASR worker.
///
/// A session is intentionally separate from [`WhisperEngine`]: callers choose
/// their own admission policy (one worker, a small bounded pool, priorities)
/// without putting a global inference mutex inside the model context. It must
/// never be used concurrently; the mutable borrow on `transcribe_with_session`
/// enforces that at the Rust boundary.
pub struct WhisperSession {
    state: WhisperState,
}

// SAFETY: WhisperContext holds read-only model weights after construction.
// Concurrent compatibility calls create independent WhisperState values;
// reusable states live in caller-owned WhisperSession values and require a
// mutable borrow for inference, so they cannot be used concurrently in safe
// Rust. No mutable decode state is shared through WhisperEngine itself.
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

        // Allocate a decode state here (while still in the loader's GPU/CPU
        // fallback path) AND run one throwaway pass on it so the GPU compute
        // graph — Vulkan/CUDA pipeline compilation, buffer allocation, first-
        // kernel JIT — is built on this loader thread instead of stalling the
        // user's first real dictation. Best-effort: a warmup failure is non-fatal.
        let mut state_probe = ctx
            .create_state()
            .map_err(|e| AppError::Asr(format!("Failed to allocate decode state: {e}")))?;
        {
            let mut warm = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
            warm.set_n_threads(config.n_threads as i32);
            warm.set_translate(false);
            warm.set_print_progress(false);
            warm.set_print_special(false);
            warm.set_print_realtime(false);
            warm.set_print_timestamps(false);
            warm.set_no_speech_thold(0.6);
            // ~1s of silence — enough to build and execute the full graph.
            let silence = vec![0.0f32; 16_000];
            let _ = state_probe.full(warm, &silence);
        }
        // Drop the warmed state. The Vulkan/CUDA pipeline + kernel compilation it
        // triggered is cached at the backend/device level and persists, so the
        // warmup benefit survives — but we do NOT keep the (hundreds-of-MB)
        // decode buffers resident, which would overlap the live-preview worker's
        // state on smaller GPUs (the exact contention the pipeline serializes to
        // avoid) and could reuse a state left bad by a failed warmup.
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

    /// Snapshot the effective persistent vocabulary prompt.
    ///
    /// The runtime override wins when the dictionary has changed since model
    /// load; otherwise this returns the prompt stored in the model config.
    /// Screen-context callers merge against this value before applying their
    /// immutable, request-local override.
    pub fn get_initial_prompt(&self) -> Option<String> {
        self.prompt_override
            .read()
            .ok()
            .and_then(|guard| guard.clone())
            .or_else(|| self.config.initial_prompt.clone())
    }

    /// Backend preference used to build this context. A CPU fallback returns
    /// false because the fallback constructs a new engine with GPU disabled.
    pub fn uses_gpu(&self) -> bool {
        self.config.use_gpu
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

    /// Allocate a reusable final-transcription session.
    pub fn create_session(&self) -> AppResult<WhisperSession> {
        self.ctx
            .create_state()
            .map(|state| WhisperSession { state })
            .map_err(|e| AppError::Asr(format!("Failed to create ASR session: {e}")))
    }

    /// Run greedy transcription using a caller-supplied, reused `WhisperState`.
    ///
    /// Uses greedy decoding (beam_size=1), no temperature fallback, no initial
    /// prompt — optimized for speed over accuracy.  Because `state.full` is
    /// designed to be called repeatedly on the same state (whisper.cpp resets
    /// per-call decode buffers internally), reusing the state across
    /// iterations is both safe and ~10-30× cheaper than recreating it.
    ///
    /// When `is_recording` is supplied, inference aborts as soon as the flag
    /// flips false — so a preview tick that's mid-flight when the user stops
    /// recording bails out in tens of milliseconds instead of making the
    /// final transcription wait for it to finish.
    pub fn transcribe_preview_with_state(
        &self,
        state: &mut WhisperState,
        audio: &[f32],
        is_recording: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    ) -> AppResult<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

        match self.config.language.as_deref() {
            Some("auto") | None => {}
            Some(lang) => params.set_language(Some(lang)),
        }

        if let Some(flag) = is_recording.as_ref() {
            // whisper.cpp polls this between encoder/decoder steps; returning
            // true aborts the pass. Keep the Arc alive through `state.full` so
            // the raw callback's user-data pointer remains valid.
            install_abort_callback(&mut params, flag, abort_when_recording_stops);
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
        self.transcribe_with_options(audio, &TranscriptionOptions::configured())
    }
}

impl WhisperEngine {
    /// Transcribe with immutable request-local decode and prompt options.
    ///
    /// This compatibility path creates a temporary state. A dedicated worker
    /// should retain a [`WhisperSession`] and use [`Self::transcribe_with_session`]
    /// to avoid decode-buffer allocation churn.
    pub fn transcribe_with_options(
        &self,
        audio: &[f32],
        options: &TranscriptionOptions,
    ) -> AppResult<TranscriptionResult> {
        if audio.is_empty() {
            return Ok(self.empty_result());
        }

        let state_started = std::time::Instant::now();
        let mut session = self.create_session()?;
        let state_setup_us = elapsed_us(state_started);
        self.transcribe_session_inner(&mut session, audio, options, state_setup_us, None)
    }

    /// Transcribe with a caller-owned reusable decode state.
    pub fn transcribe_with_session(
        &self,
        session: &mut WhisperSession,
        audio: &[f32],
        options: &TranscriptionOptions,
    ) -> AppResult<TranscriptionResult> {
        self.transcribe_with_session_cancellable(session, audio, options, None)
    }

    /// Transcribe with a caller-owned decode state and cooperative cancellation.
    /// The native callback is polled between encoder/decoder steps, allowing a
    /// latest-generation worker to abandon stale inference promptly.
    pub fn transcribe_with_session_cancellable(
        &self,
        session: &mut WhisperSession,
        audio: &[f32],
        options: &TranscriptionOptions,
        cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> AppResult<TranscriptionResult> {
        if audio.is_empty() {
            return Ok(self.empty_result());
        }
        self.transcribe_session_inner(session, audio, options, 0, cancelled)
    }

    fn empty_result(&self) -> TranscriptionResult {
        TranscriptionResult {
            text: String::new(),
            segments: vec![],
            duration_ms: 0,
            model_name: self.config.model_path.clone(),
            timings: TranscriptionTimings::default(),
        }
    }

    fn transcribe_session_inner(
        &self,
        session: &mut WhisperSession,
        audio: &[f32],
        options: &TranscriptionOptions,
        state_setup_us: u64,
        cancelled: Option<Arc<std::sync::atomic::AtomicBool>>,
    ) -> AppResult<TranscriptionResult> {
        let (strategy, temperature, temperature_increment) = match options.decode_policy() {
            DecodePolicy::Configured => {
                let beam_size = self.config.beam_size.unwrap_or(5);
                let strategy = if beam_size <= 1 {
                    SamplingStrategy::Greedy { best_of: 1 }
                } else {
                    SamplingStrategy::BeamSearch {
                        beam_size: beam_size as std::ffi::c_int,
                        patience: -1.0,
                    }
                };
                (
                    strategy,
                    self.config.temperature.unwrap_or(0.0),
                    self.config.temperature_inc.unwrap_or(0.2),
                )
            }
            DecodePolicy::Greedy {
                best_of,
                temperature,
                temperature_increment,
            } => (
                SamplingStrategy::Greedy {
                    best_of: best_of.clamp(1, std::ffi::c_int::MAX as u32) as std::ffi::c_int,
                },
                temperature,
                temperature_increment,
            ),
        };
        let mut params = FullParams::new(strategy);

        match self.config.language.as_deref() {
            Some("auto") | None => {}
            Some(lang) => params.set_language(Some(lang)),
        }
        params.set_translate(self.config.translate);
        params.set_n_threads(self.config.n_threads as i32);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_suppress_blank(true);
        params.set_suppress_nst(true);
        params.set_single_segment(options.single_segment());
        params.set_temperature(temperature);
        params.set_temperature_inc(temperature_increment);
        params.set_entropy_thold(2.4);
        params.set_logprob_thold(-1.0);
        params.set_no_speech_thold(0.6);

        if let Some(cancelled) = cancelled.as_ref() {
            if cancelled.load(Ordering::Acquire) {
                return Err(AppError::Asr("Inference cancelled before start".into()));
            }
            install_abort_callback(&mut params, cancelled, abort_when_cancelled);
        }

        // Request-local dynamic context takes precedence over the engine's
        // persistent vocabulary prompt and cannot leak across concurrent calls.
        let effective_prompt = match options.initial_prompt_override() {
            Some(prompt) => prompt.map(ToOwned::to_owned),
            None => self
                .prompt_override
                .read()
                .ok()
                .and_then(|guard| guard.clone())
                .or_else(|| self.config.initial_prompt.clone()),
        };
        if let Some(ref prompt) = effective_prompt {
            params.set_initial_prompt(prompt);
        }

        let inference_started = std::time::Instant::now();
        session
            .state
            .full(params, audio)
            .map_err(|e| AppError::Asr(format!("Inference failed: {e}")))?;
        let inference_us = elapsed_us(inference_started);

        let extraction_started = std::time::Instant::now();
        let n_segments = session.state.full_n_segments();
        let mut segments = Vec::with_capacity(n_segments as usize);
        let mut full_text = String::new();
        for i in 0..n_segments {
            let seg = match session.state.get_segment(i) {
                Some(segment) => segment,
                None => continue,
            };
            let text = seg
                .to_str_lossy()
                .unwrap_or(std::borrow::Cow::Borrowed(""))
                .into_owned();
            full_text.push_str(&text);
            segments.push(TranscriptionSegment {
                start_ms: (seg.start_timestamp() as u64) * 10,
                end_ms: (seg.end_timestamp() as u64) * 10,
                text,
                confidence: 0.0,
            });
        }
        let result_extraction_us = elapsed_us(extraction_started);
        let duration_ms = (audio.len() as f64 / 16_000.0 * 1000.0) as u64;

        Ok(TranscriptionResult {
            text: full_text.trim().to_string(),
            segments,
            duration_ms,
            model_name: self.config.model_path.clone(),
            timings: TranscriptionTimings {
                state_setup_us,
                inference_us,
                result_extraction_us,
            },
        })
    }
}

fn elapsed_us(started: std::time::Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::{abort_when_cancelled, abort_when_recording_stops, WhisperSession};

    #[test]
    fn raw_final_abort_callback_tracks_cancellation_flag() {
        let flag = AtomicBool::new(false);
        let user_data = (&flag as *const AtomicBool).cast_mut().cast::<c_void>();

        // SAFETY: `user_data` points to `flag`, which lives for this entire test.
        assert!(!unsafe { abort_when_cancelled(user_data) });
        flag.store(true, Ordering::Release);
        // SAFETY: same live pointer as above.
        assert!(unsafe { abort_when_cancelled(user_data) });
    }

    #[test]
    fn raw_preview_abort_callback_tracks_recording_flag() {
        let flag = AtomicBool::new(true);
        let user_data = (&flag as *const AtomicBool).cast_mut().cast::<c_void>();

        // SAFETY: `user_data` points to `flag`, which lives for this entire test.
        assert!(!unsafe { abort_when_recording_stops(user_data) });
        flag.store(false, Ordering::Release);
        // SAFETY: same live pointer as above.
        assert!(unsafe { abort_when_recording_stops(user_data) });
    }

    #[test]
    fn reusable_session_can_be_owned_by_a_worker_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<WhisperSession>();
    }
}
