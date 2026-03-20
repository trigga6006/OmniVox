use crate::error::AppResult;
use crate::storage::types::DictionaryEntry;
use chrono::Utc;
use uuid::Uuid;

/// Add a new dictionary entry.
pub fn add_entry(phrase: &str, replacement: &str) -> AppResult<DictionaryEntry> {
    Ok(DictionaryEntry {
        id: Uuid::new_v4(),
        phrase: phrase.to_string(),
        replacement: replacement.to_string(),
        is_enabled: true,
        created_at: Utc::now(),
    })
}

/// Update an existing dictionary entry.
pub fn update_entry(_id: &str, _phrase: &str, _replacement: &str) -> AppResult<()> {
    Ok(())
}

/// Delete a dictionary entry by ID.
pub fn delete_entry(_id: &str) -> AppResult<()> {
    Ok(())
}

/// List all dictionary entries.
pub fn list_entries() -> AppResult<Vec<DictionaryEntry>> {
    Ok(vec![])
}
