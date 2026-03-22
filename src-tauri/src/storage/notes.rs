use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::Note;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

fn row_to_note(row: &rusqlite::Row) -> rusqlite::Result<Note> {
    let id_str: String = row.get(0)?;
    let title: String = row.get(1)?;
    let content: String = row.get(2)?;
    let created_at_str: String = row.get(3)?;
    let updated_at_str: String = row.get(4)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(Note {
        id,
        title,
        content,
        created_at,
        updated_at,
    })
}

pub fn add_note(db: &Database, title: &str, content: &str) -> AppResult<Note> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO notes (id, title, content, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            id.to_string(),
            title,
            content,
            now.to_rfc3339(),
            now.to_rfc3339(),
        ],
    )?;

    Ok(Note {
        id,
        title: title.to_string(),
        content: content.to_string(),
        created_at: now,
        updated_at: now,
    })
}

pub fn update_note(db: &Database, id: &str, title: &str, content: &str) -> AppResult<()> {
    let now = Utc::now();
    let conn = db.conn()?;
    conn.execute(
        "UPDATE notes SET title = ?1, content = ?2, updated_at = ?3 WHERE id = ?4",
        params![title, content, now.to_rfc3339(), id],
    )?;
    Ok(())
}

pub fn delete_note(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("DELETE FROM notes WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn list_notes(db: &Database) -> AppResult<Vec<Note>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, title, content, created_at, updated_at
         FROM notes
         ORDER BY updated_at DESC",
    )?;
    let notes = stmt
        .query_map([], row_to_note)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(notes)
}
