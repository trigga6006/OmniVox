use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::DictionaryEntry;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

/// Map a rusqlite row to a DictionaryEntry.
fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<DictionaryEntry> {
    let id_str: String = row.get(0)?;
    let phrase: String = row.get(1)?;
    let replacement: String = row.get(2)?;
    let is_enabled: bool = row.get(3)?;
    let created_at_str: String = row.get(4)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(DictionaryEntry {
        id,
        phrase,
        replacement,
        is_enabled,
        created_at,
    })
}

/// Add a new dictionary entry. Returns the created entry.
pub fn add_entry(db: &Database, phrase: &str, replacement: &str) -> AppResult<DictionaryEntry> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO dictionary_entries (id, phrase, replacement, is_enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            phrase,
            replacement,
            true,
            now.to_rfc3339(),
        ],
    )?;

    Ok(DictionaryEntry {
        id,
        phrase: phrase.to_string(),
        replacement: replacement.to_string(),
        is_enabled: true,
        created_at: now,
    })
}

/// Update an existing dictionary entry by ID.
pub fn update_entry(db: &Database, id: &str, phrase: &str, replacement: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "UPDATE dictionary_entries SET phrase = ?1, replacement = ?2 WHERE id = ?3",
        params![phrase, replacement, id],
    )?;
    Ok(())
}

/// Delete a dictionary entry by ID.
pub fn delete_entry(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "DELETE FROM dictionary_entries WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// List all dictionary entries, ordered by creation time ascending.
pub fn list_entries(db: &Database) -> AppResult<Vec<DictionaryEntry>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, phrase, replacement, is_enabled, created_at
         FROM dictionary_entries
         ORDER BY created_at ASC",
    )?;
    let entries = stmt
        .query_map([], row_to_entry)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}
