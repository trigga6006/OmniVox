use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::AppBinding;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

/// List all app bindings for a given context mode.
pub fn list_bindings_for_mode(db: &Database, mode_id: &str) -> AppResult<Vec<AppBinding>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, mode_id, process_name, created_at FROM mode_app_bindings WHERE mode_id = ?1 ORDER BY created_at",
    )?;
    let rows = stmt.query_map(params![mode_id], |row| {
        let id_str: String = row.get(0)?;
        let mode_id: String = row.get(1)?;
        let process_name: String = row.get(2)?;
        let created_at_str: String = row.get(3)?;

        let id = id_str.parse().unwrap_or_default();
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        Ok(AppBinding {
            id,
            mode_id,
            process_name,
            created_at,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Create a new app binding associating a process name with a context mode.
pub fn add_binding(db: &Database, mode_id: &str, process_name: &str) -> AppResult<AppBinding> {
    let binding = AppBinding {
        id: Uuid::new_v4(),
        mode_id: mode_id.to_string(),
        process_name: process_name.to_string(),
        created_at: Utc::now(),
    };
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO mode_app_bindings (id, mode_id, process_name, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![
            binding.id.to_string(),
            binding.mode_id,
            binding.process_name,
            binding.created_at.to_rfc3339(),
        ],
    )?;
    Ok(binding)
}

/// Delete an app binding by ID.
pub fn delete_binding(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("DELETE FROM mode_app_bindings WHERE id = ?1", params![id])?;
    Ok(())
}

/// Find the context mode ID associated with a process name (case-insensitive).
/// Returns `None` if no binding matches.
pub fn find_mode_for_process(db: &Database, process_name: &str) -> AppResult<Option<String>> {
    let conn = db.conn()?;
    let result = conn.query_row(
        "SELECT mode_id FROM mode_app_bindings WHERE LOWER(process_name) = LOWER(?1) LIMIT 1",
        params![process_name],
        |row| row.get(0),
    );
    match result {
        Ok(id) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
