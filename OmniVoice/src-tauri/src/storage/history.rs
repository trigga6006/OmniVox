use crate::error::AppResult;
use crate::storage::types::TranscriptionRecord;

/// Search transcription history by query string.
pub fn search_history(_query: &str) -> AppResult<Vec<TranscriptionRecord>> {
    Ok(vec![])
}

/// Get the most recent transcription records, up to `limit`.
pub fn recent_history(_limit: u32) -> AppResult<Vec<TranscriptionRecord>> {
    Ok(vec![])
}

/// Delete a single transcription record by ID.
pub fn delete_record(_id: &str) -> AppResult<()> {
    Ok(())
}

/// Export transcription history in the given format (e.g., "json", "csv").
pub fn export_history(_format: &str) -> AppResult<String> {
    Ok("Export stub".to_string())
}
