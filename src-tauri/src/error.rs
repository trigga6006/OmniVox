use serde::Serialize;

/// Typed error codes for granular frontend error display.
///
/// Each variant maps to a user-actionable category so the frontend can
/// show specific guidance instead of a generic "Something went wrong."
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    // Audio
    AudioDeviceNotFound,
    AudioDeviceBusy,
    AudioCaptureError,
    MicPermissionDenied,
    // ASR / Model
    NoModelLoaded,
    ModelLoadFailed,
    ModelCorrupted,
    TranscriptionFailed,
    TranscriptionPanicked,
    // Output
    ClipboardError,
    KeystrokeError,
    // Storage
    DatabaseError,
    // LLM (Structured Mode)
    LlmUnavailable,
    LlmTimeout,
    LlmInferenceFailed,
    // General
    InternalError,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Audio error: {0}")]
    Audio(String),
    #[error("ASR error: {0}")]
    Asr(String),
    #[error("Model error: {0}")]
    Model(String),
    #[error("Output error: {0}")]
    Output(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl AppError {
    /// Map error variant to a typed error code for the frontend.
    pub fn code(&self) -> ErrorCode {
        match self {
            AppError::Audio(msg) => {
                // Case-insensitive substring check without allocating a new String.
                let bytes = msg.as_bytes();
                let contains = |needle: &str| -> bool {
                    bytes.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
                };
                if contains("permission")
                    || contains("denied")
                    || contains("not allowed")
                    || contains("no default input device")
                {
                    ErrorCode::MicPermissionDenied
                } else if contains("not found") {
                    ErrorCode::AudioDeviceNotFound
                } else if contains("busy") || contains("exclusive") {
                    ErrorCode::AudioDeviceBusy
                } else {
                    ErrorCode::AudioCaptureError
                }
            }
            AppError::Asr(_) => ErrorCode::TranscriptionFailed,
            AppError::Model(_) => ErrorCode::ModelLoadFailed,
            AppError::Output(_) => ErrorCode::KeystrokeError,
            AppError::Storage(_) => ErrorCode::DatabaseError,
            AppError::Llm(msg) => {
                let bytes = msg.as_bytes();
                let contains = |needle: &str| -> bool {
                    bytes.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
                };
                if contains("timed out") || contains("timeout") {
                    ErrorCode::LlmTimeout
                } else if contains("not found") || contains("no model") || contains("worker has stopped") {
                    ErrorCode::LlmUnavailable
                } else {
                    ErrorCode::LlmInferenceFailed
                }
            }
            AppError::Io(_) => ErrorCode::InternalError,
            AppError::Internal(_) => ErrorCode::InternalError,
        }
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        AppError::Storage(e.to_string())
    }
}
