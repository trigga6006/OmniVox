use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptionResult {
    pub text: String,
    pub segments: Vec<TranscriptionSegment>,
    pub duration_ms: u64,
    pub model_name: String,
    /// Wall-clock timings for the local ASR stages. These intentionally contain
    /// no transcript or prompt content, so callers can retain them in bounded
    /// diagnostics without recording dictated text.
    #[serde(default)]
    pub timings: TranscriptionTimings,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TranscriptionTimings {
    /// Time spent allocating a fresh Whisper decode state. Zero when a caller
    /// supplies a reusable [`crate::asr::engine::WhisperSession`].
    pub state_setup_us: u64,
    /// Time spent inside whisper.cpp's native `whisper_full` call.
    pub inference_us: u64,
    /// Time spent copying segment text/timestamps out of the native state.
    pub result_extraction_us: u64,
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

/// Decode behavior selected independently for each transcription request.
///
/// Keeping this value request-local avoids changing the loaded model when a
/// latency-sensitive command and an accuracy-oriented dictation overlap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DecodePolicy {
    /// Use the model's configured beam size and temperature fallback settings.
    Configured,
    /// Deterministic greedy decoding. `temperature_increment == 0.0` disables
    /// Whisper's low-confidence retry loop.
    Greedy {
        best_of: u32,
        temperature: f32,
        temperature_increment: f32,
    },
}

/// Immutable, per-call Whisper options.
///
/// Builder methods consume and return `Self`; an in-flight request therefore
/// observes one stable option set and never consults mutable global decode
/// policy. `initial_prompt_override` distinguishes "use the engine prompt"
/// from an explicit per-request prompt (including explicitly no prompt).
#[derive(Debug, Clone, PartialEq)]
pub struct TranscriptionOptions {
    decode_policy: DecodePolicy,
    single_segment: bool,
    initial_prompt_override: Option<Option<String>>,
}

/// Whether a serialized final-ASR worker may safely carry native decode state
/// into the next request. Temperature sampling uses RNG owned by WhisperState,
/// so any stochastic policy must preserve fresh-per-utterance semantics.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub(crate) enum SessionPolicy {
    Fresh,
    Reused,
}

impl TranscriptionOptions {
    /// Accuracy-oriented behavior from [`AsrConfig`].
    pub const fn configured() -> Self {
        Self {
            decode_policy: DecodePolicy::Configured,
            single_segment: false,
            initial_prompt_override: None,
        }
    }

    /// Latency-oriented deterministic decoding for short command utterances.
    pub const fn latency_first(single_segment: bool) -> Self {
        Self {
            decode_policy: DecodePolicy::Greedy {
                best_of: 1,
                temperature: 0.0,
                temperature_increment: 0.0,
            },
            single_segment,
            initial_prompt_override: None,
        }
    }

    pub const fn decode_policy(&self) -> DecodePolicy {
        self.decode_policy
    }

    pub const fn single_segment(&self) -> bool {
        self.single_segment
    }

    pub(crate) fn session_policy(&self) -> SessionPolicy {
        match self.decode_policy {
            DecodePolicy::Greedy {
                temperature,
                temperature_increment,
                ..
            } if temperature == 0.0 && temperature_increment == 0.0 => SessionPolicy::Reused,
            DecodePolicy::Configured | DecodePolicy::Greedy { .. } => SessionPolicy::Fresh,
        }
    }

    pub const fn with_decode_policy(mut self, policy: DecodePolicy) -> Self {
        self.decode_policy = policy;
        self
    }

    pub const fn with_single_segment(mut self, enabled: bool) -> Self {
        self.single_segment = enabled;
        self
    }

    /// Override the prompt for only this request. Passing `None` explicitly
    /// disables the engine's configured vocabulary prompt for this call.
    pub fn with_initial_prompt(mut self, prompt: Option<String>) -> Self {
        self.initial_prompt_override = Some(prompt);
        self
    }

    pub(crate) fn initial_prompt_override(&self) -> Option<Option<&str>> {
        self.initial_prompt_override
            .as_ref()
            .map(|prompt| prompt.as_deref())
    }
}

impl Default for TranscriptionOptions {
    fn default() -> Self {
        Self::configured()
    }
}

impl Default for AsrConfig {
    fn default() -> Self {
        // Reserve 2 logical cores for the OS, audio capture, and UI.
        let n_threads = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).clamp(2, 8) as u32)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_first_is_greedy_without_temperature_fallback() {
        let options = TranscriptionOptions::latency_first(true);
        assert_eq!(
            options.decode_policy(),
            DecodePolicy::Greedy {
                best_of: 1,
                temperature: 0.0,
                temperature_increment: 0.0,
            }
        );
        assert!(options.single_segment());
        assert_eq!(options.session_policy(), SessionPolicy::Reused);
    }

    #[test]
    fn configured_and_stochastic_decode_require_fresh_sessions() {
        assert_eq!(
            TranscriptionOptions::configured().session_policy(),
            SessionPolicy::Fresh
        );
        let stochastic =
            TranscriptionOptions::configured().with_decode_policy(DecodePolicy::Greedy {
                best_of: 1,
                temperature: 0.1,
                temperature_increment: 0.0,
            });
        assert_eq!(stochastic.session_policy(), SessionPolicy::Fresh);
    }

    #[test]
    fn per_call_prompt_distinguishes_inherit_from_explicit_none() {
        let inherited = TranscriptionOptions::configured();
        assert_eq!(inherited.initial_prompt_override(), None);

        let disabled = inherited.with_initial_prompt(None);
        assert_eq!(disabled.initial_prompt_override(), Some(None));
    }

    #[test]
    fn single_segment_builder_does_not_mutate_the_source_options() {
        let source = TranscriptionOptions::configured();
        let changed = source.clone().with_single_segment(true);
        assert!(!source.single_segment());
        assert!(changed.single_segment());
    }

    #[test]
    fn decode_policy_builder_does_not_mutate_the_source_options() {
        let source = TranscriptionOptions::configured();
        let policy = DecodePolicy::Greedy {
            best_of: 2,
            temperature: 0.1,
            temperature_increment: 0.0,
        };
        let changed = source.clone().with_decode_policy(policy);
        assert_eq!(source.decode_policy(), DecodePolicy::Configured);
        assert_eq!(changed.decode_policy(), policy);
    }
}
