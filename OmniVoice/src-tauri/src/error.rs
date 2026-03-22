use serde::Serialize;

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
    #[error("LLM error: {0}")]
    Llm(String),
    #[error("Storage error: {0}")]
    Storage(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Internal error: {0}")]
    Internal(String),
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
