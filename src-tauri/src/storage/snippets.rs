use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::Snippet;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

/// Map a rusqlite row to a Snippet.
/// Note: The SQL column is `trigger_text` (reserved word), but the Rust struct field is `trigger`.
fn row_to_snippet(row: &rusqlite::Row) -> rusqlite::Result<Snippet> {
    let id_str: String = row.get(0)?;
    let trigger: String = row.get(1)?;
    let content: String = row.get(2)?;
    let description: Option<String> = row.get(3)?;
    let is_enabled: bool = row.get(4)?;
    let created_at_str: String = row.get(5)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(Snippet {
        id,
        trigger,
        content,
        description,
        is_enabled,
        created_at,
    })
}

/// Add a new snippet. Returns the created snippet.
pub fn add_snippet(
    db: &Database,
    trigger: &str,
    content: &str,
    description: Option<&str>,
) -> AppResult<Snippet> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO snippets (id, trigger_text, content, description, is_enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id.to_string(),
            trigger,
            content,
            description,
            true,
            now.to_rfc3339(),
        ],
    )?;

    Ok(Snippet {
        id,
        trigger: trigger.to_string(),
        content: content.to_string(),
        description: description.map(|d| d.to_string()),
        is_enabled: true,
        created_at: now,
    })
}

/// Update an existing snippet by ID.
pub fn update_snippet(
    db: &Database,
    id: &str,
    trigger: &str,
    content: &str,
    description: Option<&str>,
) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "UPDATE snippets SET trigger_text = ?1, content = ?2, description = ?3 WHERE id = ?4",
        params![trigger, content, description, id],
    )?;
    Ok(())
}

/// Delete a snippet by ID.
pub fn delete_snippet(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("DELETE FROM snippets WHERE id = ?1", params![id])?;
    Ok(())
}

/// List all snippets, ordered by creation time ascending.
pub fn list_snippets(db: &Database) -> AppResult<Vec<Snippet>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, trigger_text, content, description, is_enabled, created_at
         FROM snippets
         ORDER BY created_at ASC",
    )?;
    let snippets = stmt
        .query_map([], row_to_snippet)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(snippets)
}
