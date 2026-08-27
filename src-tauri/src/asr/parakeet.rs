//! NVIDIA Parakeet-TDT offline transducer via the official sherpa-onnx bindings.
//!
//! Unlike whisper.cpp, decoding here holds no reusable per-utterance state worth
//! caching: a stream is a thin handle created and dropped around one `decode`
//! call. The recognizer itself owns the ONNX sessions and is `Send + Sync`, so a
//! single loaded engine serves every worker.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

use crate::asr::engine::AsrEngine;
use crate::asr::types::{TranscriptionResult, TranscriptionSegment, TranscriptionTimings};
use crate::error::{AppError, AppResult};

/// Upstream artifact names inside the model directory. These are fixed by the
/// pinned manifest in `models::downloader`, which downloads them verbatim.
const ENCODER_FILE: &str = "encoder.int8.onnx";
const DECODER_FILE: &str = "decoder.int8.onnx";
const JOINER_FILE: &str = "joiner.int8.onnx";
const TOKENS_FILE: &str = "tokens.txt";

/// Sample rate every caller feeds us. Capture already resamples to 16 kHz, so
/// sherpa-onnx's internal resampler is never exercised.
const SAMPLE_RATE_HZ: i32 = 16_000;

/// Production sherpa-onnx Parakeet engine.
pub struct ParakeetEngine {
    recognizer: OfflineRecognizer,
    /// Directory the model was loaded from; reported as the trace model name.
    model_name: String,
    n_threads: u32,
}

impl ParakeetEngine {
    /// Load the four ONNX/token artifacts from a verified model directory.
    ///
    /// `decoding_method` is deliberately left unset so the recognizer uses
    /// greedy search: modified beam search has an open TDT hallucination bug
    /// (k2-fsa/sherpa-onnx#3267). `model_type` is set to `nemo_transducer`,
    /// exactly as the sherpa-onnx 1.13.5 crate's own doc example does — this
    /// skips the metadata-sniffing double load sherpa-onnx otherwise performs
    /// to infer the model type.
    pub fn load(dir: &Path, n_threads: u32) -> AppResult<Self> {
        // These errors reach the ALWAYS-ON model-load log (`crate::diag`) via the
        // ASR switch/rollback path, so they name the artifact and the model
        // folder only — never the full path, which on Windows starts with the
        // user's profile name.
        let artifact = |name: &str| -> AppResult<String> {
            let path: PathBuf = dir.join(name);
            if !path.exists() {
                return Err(AppError::Asr(format!(
                    "Parakeet model file not found: {name}"
                )));
            }
            Ok(path.to_string_lossy().into_owned())
        };

        let mut config = OfflineRecognizerConfig::default();
        config.model_config.transducer = OfflineTransducerModelConfig {
            encoder: Some(artifact(ENCODER_FILE)?),
            decoder: Some(artifact(DECODER_FILE)?),
            joiner: Some(artifact(JOINER_FILE)?),
        };
        config.model_config.tokens = Some(artifact(TOKENS_FILE)?);
        config.model_config.model_type = Some("nemo_transducer".into());
        config.model_config.provider = Some("cpu".into());
        config.model_config.num_threads = n_threads as i32;

        let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
            let folder = dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "<unknown>".into());
            AppError::Asr(format!("Failed to load Parakeet model '{folder}'"))
        })?;

        // Run one throwaway decode so ONNX Runtime's lazy initialization (graph
        // optimization, arena allocation, first-kernel JIT) happens here on the
        // loader thread instead of stalling the user's first dictation.  Mirrors
        // `WhisperEngine::load`'s warm-up; best-effort, so the result is dropped.
        {
            let warm = recognizer.create_stream();
            // ~1s of silence — enough to execute the full encoder/decoder/joiner.
            warm.accept_waveform(SAMPLE_RATE_HZ, &vec![0.0f32; SAMPLE_RATE_HZ as usize]);
            recognizer.decode(&warm);
        }

        Ok(Self {
            recognizer,
            model_name: dir.to_string_lossy().into_owned(),
            n_threads,
        })
    }

    /// Threads handed to the ONNX Runtime session at load time.
    pub fn n_threads(&self) -> u32 {
        self.n_threads
    }

    /// Transcribe with coarse cooperative cancellation.
    ///
    /// sherpa-onnx exposes no abort hook inside `decode`, so a cancelled request
    /// is detected on either side of inference rather than during it. That is
    /// sufficient here: the pipeline's admission layer discards results whose
    /// generation was superseded, so a late completion is dropped safely.
    pub fn transcribe_cancellable(
        &self,
        audio: &[f32],
        cancelled: Option<Arc<AtomicBool>>,
    ) -> AppResult<TranscriptionResult> {
        if audio.is_empty() {
            return Ok(self.empty_result());
        }
        if let Some(flag) = cancelled.as_ref() {
            if flag.load(Ordering::Acquire) {
                return Err(AppError::Asr("Inference cancelled before start".into()));
            }
        }

        let inference_started = std::time::Instant::now();
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE_HZ, audio);
        self.recognizer.decode(&stream);
        let inference_us = elapsed_us(inference_started);

        if let Some(flag) = cancelled.as_ref() {
            if flag.load(Ordering::Acquire) {
                return Err(AppError::Asr("Inference cancelled".into()));
            }
        }

        let extraction_started = std::time::Instant::now();
        let text = stream
            .get_result()
            .map(|result| result.text)
            .unwrap_or_default();
        let text = text.trim().to_string();
        let result_extraction_us = elapsed_us(extraction_started);

        let duration_ms = (audio.len() as f64 / f64::from(SAMPLE_RATE_HZ) * 1000.0) as u64;
        // Parakeet returns one utterance, not Whisper-style timestamped
        // segments. Emit a single segment spanning the clip so callers that
        // persist segment timestamps (Meeting Mode) keep working.
        let segments = if text.is_empty() {
            Vec::new()
        } else {
            vec![TranscriptionSegment {
                start_ms: 0,
                end_ms: duration_ms,
                text: text.clone(),
                confidence: 0.0,
            }]
        };

        Ok(TranscriptionResult {
            text,
            segments,
            duration_ms,
            model_name: self.model_name.clone(),
            timings: TranscriptionTimings {
                // A stream is allocated inside the timed region below; there is
                // no separate decode-state allocation to charge for.
                state_setup_us: 0,
                inference_us,
                result_extraction_us,
            },
        })
    }

    /// Live-preview transcription. Stateless, so it simply re-decodes the
    /// trailing window and returns the text; `is_recording` is checked on both
    /// sides of inference so a tick that lands after stop is discarded.
    pub fn transcribe_preview(
        &self,
        audio: &[f32],
        is_recording: Option<Arc<AtomicBool>>,
    ) -> AppResult<String> {
        if audio.is_empty() {
            return Ok(String::new());
        }
        if let Some(flag) = is_recording.as_ref() {
            if !flag.load(Ordering::Acquire) {
                return Ok(String::new());
            }
        }

        let stream = self.recognizer.create_stream();
        stream.accept_waveform(SAMPLE_RATE_HZ, audio);
        self.recognizer.decode(&stream);

        if let Some(flag) = is_recording.as_ref() {
            if !flag.load(Ordering::Acquire) {
                return Ok(String::new());
            }
        }

        Ok(stream
            .get_result()
            .map(|result| result.text)
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    fn empty_result(&self) -> TranscriptionResult {
        TranscriptionResult {
            text: String::new(),
            segments: vec![],
            duration_ms: 0,
            model_name: self.model_name.clone(),
            timings: TranscriptionTimings::default(),
        }
    }
}

impl AsrEngine for ParakeetEngine {
    fn transcribe(&self, audio: &[f32]) -> AppResult<TranscriptionResult> {
        self.transcribe_cancellable(audio, None)
    }
}

fn elapsed_us(started: std::time::Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}
