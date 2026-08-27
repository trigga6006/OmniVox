//! The loaded-engine slot shared by every ASR caller.
//!
//! One model is active at a time and it may belong to either engine family, so
//! the pipeline, Meeting Mode, and the model commands hold a
//! [`LoadedAsrEngine`] rather than a concrete engine. Cloning is cheap — each
//! variant is an `Arc` — which keeps the existing "snapshot the engine, then
//! release the mutex" pattern intact.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use crate::asr::engine::{WhisperEngine, WhisperSession};
use crate::asr::parakeet::ParakeetEngine;
use crate::asr::types::{TranscriptionOptions, TranscriptionResult};
use crate::error::{AppError, AppResult};

#[derive(Clone)]
pub enum LoadedAsrEngine {
    Whisper(Arc<WhisperEngine>),
    Parakeet(Arc<ParakeetEngine>),
}

/// A reusable final-transcription session.
///
/// Whisper sessions own multi-hundred-megabyte decode buffers worth caching
/// across requests. Parakeet has no such state, so its variant is zero-sized
/// and every request behaves like a fresh one.
pub enum AsrSession {
    Whisper(WhisperSession),
    Parakeet,
}

/// Decode state owned by the live-preview worker for a whole recording.
pub enum AsrPreviewState {
    Whisper(whisper_rs::WhisperState),
    Parakeet,
}

/// A session or preview state can only be used with the engine that created it.
/// The workers rebuild both whenever the engine identity changes, so reaching
/// this is a programming error rather than a runtime condition.
fn mismatched_session() -> AppError {
    AppError::Asr("ASR session does not match the loaded engine".into())
}

impl LoadedAsrEngine {
    /// Identity comparison for the session-reuse caches. Comparing the inner
    /// `Arc` (not the enum wrapper, which is cloned per request) is what tells a
    /// worker whether its cached session still belongs to the live engine.
    pub fn ptr_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Whisper(a), Self::Whisper(b)) => Arc::ptr_eq(a, b),
            (Self::Parakeet(a), Self::Parakeet(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Backend preference used to build this engine. Parakeet runs on the ONNX
    /// Runtime CPU provider, so it is always false.
    pub fn uses_gpu(&self) -> bool {
        match self {
            Self::Whisper(engine) => engine.uses_gpu(),
            Self::Parakeet(_) => false,
        }
    }

    /// Effective vocabulary prompt. Parakeet has no prompt conditioning, so
    /// screen-context callers see "nothing to merge against".
    pub fn get_initial_prompt(&self) -> Option<String> {
        match self {
            Self::Whisper(engine) => engine.get_initial_prompt(),
            Self::Parakeet(_) => None,
        }
    }

    /// Hot-swap the vocabulary prompt. A no-op for Parakeet.
    pub fn set_initial_prompt(&self, prompt: Option<String>) {
        match self {
            Self::Whisper(engine) => engine.set_initial_prompt(prompt),
            Self::Parakeet(_) => {}
        }
    }

    pub fn create_session(&self) -> AppResult<AsrSession> {
        match self {
            Self::Whisper(engine) => engine.create_session().map(AsrSession::Whisper),
            Self::Parakeet(_) => Ok(AsrSession::Parakeet),
        }
    }

    pub fn create_preview_state(&self) -> AppResult<AsrPreviewState> {
        match self {
            Self::Whisper(engine) => engine.create_preview_state().map(AsrPreviewState::Whisper),
            Self::Parakeet(_) => Ok(AsrPreviewState::Parakeet),
        }
    }

    /// Final transcription with cooperative cancellation.
    ///
    /// Parakeet ignores the Whisper-specific parts of `options` (decode policy,
    /// prompt override, single-segment): its transducer has no beam/temperature
    /// ladder, no prompt conditioning, and always yields one utterance.
    pub fn transcribe_with_session_cancellable(
        &self,
        session: &mut AsrSession,
        audio: &[f32],
        options: &TranscriptionOptions,
        cancelled: Option<Arc<AtomicBool>>,
    ) -> AppResult<TranscriptionResult> {
        match (self, session) {
            (Self::Whisper(engine), AsrSession::Whisper(session)) => {
                engine.transcribe_with_session_cancellable(session, audio, options, cancelled)
            }
            (Self::Parakeet(engine), AsrSession::Parakeet) => {
                engine.transcribe_cancellable(audio, cancelled)
            }
            _ => Err(mismatched_session()),
        }
    }

    pub fn transcribe_with_session(
        &self,
        session: &mut AsrSession,
        audio: &[f32],
        options: &TranscriptionOptions,
    ) -> AppResult<TranscriptionResult> {
        self.transcribe_with_session_cancellable(session, audio, options, None)
    }

    pub fn transcribe_preview_with_state(
        &self,
        state: &mut AsrPreviewState,
        audio: &[f32],
        is_recording: Option<Arc<AtomicBool>>,
    ) -> AppResult<String> {
        match (self, state) {
            (Self::Whisper(engine), AsrPreviewState::Whisper(state)) => {
                engine.transcribe_preview_with_state(state, audio, is_recording)
            }
            (Self::Parakeet(engine), AsrPreviewState::Parakeet) => {
                engine.transcribe_preview(audio, is_recording)
            }
            _ => Err(mismatched_session()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sessions_and_engines_travel_across_worker_threads() {
        fn assert_send<T: Send>() {}
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send::<AsrSession>();
        assert_send::<AsrPreviewState>();
        assert_send_sync::<LoadedAsrEngine>();
    }

    #[test]
    fn cross_family_sessions_are_rejected_rather_than_silently_reused() {
        assert!(mismatched_session().to_string().contains("does not match"));
    }
}
