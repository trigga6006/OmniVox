use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::VocabularyEntry;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

/// Map a rusqlite row to a VocabularyEntry.
fn row_to_entry(row: &rusqlite::Row) -> rusqlite::Result<VocabularyEntry> {
    let id_str: String = row.get(0)?;
    let word: String = row.get(1)?;
    let is_enabled: bool = row.get(2)?;
    let created_at_str: String = row.get(3)?;
    let mode_id: Option<String> = row.get(4).unwrap_or(None);

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(VocabularyEntry {
        id,
        word,
        is_enabled,
        created_at,
        mode_id,
    })
}

/// Add a new vocabulary entry. Returns the created entry.
pub fn add_entry(
    db: &Database,
    word: &str,
    mode_id: Option<&str>,
) -> AppResult<VocabularyEntry> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO vocabulary_entries (id, word, is_enabled, created_at, mode_id)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            word,
            true,
            now.to_rfc3339(),
            mode_id,
        ],
    )?;

    Ok(VocabularyEntry {
        id,
        word: word.to_string(),
        is_enabled: true,
        created_at: now,
        mode_id: mode_id.map(|s| s.to_string()),
    })
}

/// Update an existing vocabulary entry by ID.
pub fn update_entry(db: &Database, id: &str, word: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "UPDATE vocabulary_entries SET word = ?1 WHERE id = ?2",
        params![word, id],
    )?;
    Ok(())
}

/// Delete a vocabulary entry by ID.
pub fn delete_entry(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "DELETE FROM vocabulary_entries WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// List global vocabulary entries (not tied to any context mode).
pub fn list_entries(db: &Database) -> AppResult<Vec<VocabularyEntry>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, word, is_enabled, created_at, mode_id
         FROM vocabulary_entries
         WHERE mode_id IS NULL
         ORDER BY created_at ASC",
    )?;
    let entries = stmt
        .query_map([], row_to_entry)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}

/// List vocabulary entries belonging to a specific context mode.
pub fn list_entries_for_mode(db: &Database, mode_id: &str) -> AppResult<Vec<VocabularyEntry>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, word, is_enabled, created_at, mode_id
         FROM vocabulary_entries
         WHERE mode_id = ?1
         ORDER BY created_at ASC",
    )?;
    let entries = stmt
        .query_map(params![mode_id], row_to_entry)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(entries)
}
