use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::{context_mode_seed_data, types::ContextMode};
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

fn row_to_mode(row: &rusqlite::Row) -> rusqlite::Result<ContextMode> {
    let id_str: String = row.get(0)?;
    let name: String = row.get(1)?;
    let description: String = row.get(2)?;
    let icon: String = row.get(3)?;
    let color: String = row.get(4)?;
    let sort_order: i32 = row.get(6)?;
    let is_builtin: bool = row.get(7)?;
    let created_at_str: String = row.get(8)?;
    let updated_at_str: String = row.get(9)?;
    let writing_style: String = row.get(10)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(ContextMode {
        id,
        name,
        description,
        icon,
        color,
        sort_order,
        is_builtin,
        created_at,
        updated_at,
        writing_style,
    })
}

const SELECT_COLS: &str =
    "id, name, description, icon, color, llm_prompt, sort_order, is_builtin, created_at, updated_at, writing_style";

/// Return the ID of the builtin General mode (the fallback for unbound apps).
pub fn get_general_mode_id(db: &Database) -> AppResult<String> {
    let conn = db.conn()?;
    conn.query_row(
        "SELECT id FROM context_modes WHERE is_builtin = 1 LIMIT 1",
        [],
        |row| row.get(0),
    )
    .map_err(|e| crate::error::AppError::Storage(format!("General mode not found: {e}")))
}

/// Ensure the builtin "General" mode exists. Returns its ID.
/// Also refreshes builtin prompts to the latest version on every launch.
pub fn seed_general_mode(db: &Database) -> AppResult<String> {
    let id = {
        let conn = db.conn()?;

        // Check if it already exists
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM context_modes WHERE is_builtin = 1 LIMIT 1",
                [],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            // General mode has no mode-specific additions — clear any stale
            // full prompt left over from earlier versions.
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE context_modes SET llm_prompt = ?1, updated_at = ?2 \
                 WHERE id = ?3 AND is_builtin = 1",
                params!["", now, id],
            )?;

            // General already exists, but still ensure other builtin modes are seeded
            // (they may have been missed due to earlier bugs).
            drop(conn);
            context_mode_seed_data::seed_builtin_context_modes(db)?;

            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        // General mode: empty llm_prompt (no mode-specific additions).
        conn.execute(
            &format!("INSERT INTO context_modes ({SELECT_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"),
            params![id, "General", "Default dictation mode", "mic", "amber", "", 0, true, now, now, "formal"],
        )?;

        // Leave existing entries with mode_id IS NULL — they're global entries
        // that apply in every mode.

        id
    }; // drop conn guard before calling seed_programming_mode which also needs the lock

    // Seed additional builtin modes and default corrections
    context_mode_seed_data::seed_builtin_context_modes(db)?;

    context_mode_seed_data::seed_general_dictionary(db)?;

    Ok(id)
}

pub fn list_modes(db: &Database) -> AppResult<Vec<ContextMode>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM context_modes ORDER BY sort_order ASC, created_at ASC"
    ))?;
    let modes = stmt
        .query_map([], row_to_mode)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(modes)
}

pub fn get_mode(db: &Database, id: &str) -> AppResult<ContextMode> {
    let conn = db.conn()?;
    let mode = conn.query_row(
        &format!("SELECT {SELECT_COLS} FROM context_modes WHERE id = ?1"),
        params![id],
        row_to_mode,
    )?;
    Ok(mode)
}

pub fn create_mode(
    db: &Database,
    name: &str,
    description: &str,
    icon: &str,
    color: &str,
    writing_style: &str,
) -> AppResult<ContextMode> {
    let id = Uuid::new_v4();
    let now = Utc::now();

    let conn = db.conn()?;
    conn.execute(
        &format!(
            "INSERT INTO context_modes ({SELECT_COLS}) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"
        ),
        params![
            id.to_string(),
            name,
            description,
            icon,
            color,
            "",
            0,
            false,
            now.to_rfc3339(),
            now.to_rfc3339(),
            writing_style,
        ],
    )?;

    Ok(ContextMode {
        id,
        name: name.to_string(),
        description: description.to_string(),
        icon: icon.to_string(),
        color: color.to_string(),
        sort_order: 0,
        is_builtin: false,
        created_at: now,
        updated_at: now,
        writing_style: writing_style.to_string(),
    })
}

pub fn update_mode(
    db: &Database,
    id: &str,
    name: &str,
    description: &str,
    icon: &str,
    color: &str,
    writing_style: &str,
) -> AppResult<()> {
    let now = Utc::now().to_rfc3339();
    let conn = db.conn()?;
    conn.execute(
        "UPDATE context_modes SET name=?1, description=?2, icon=?3, color=?4, llm_prompt=?5, updated_at=?6, writing_style=?8 WHERE id=?7",
        params![name, description, icon, color, "", now, id, writing_style],
    )?;
    Ok(())
}

pub fn delete_mode(db: &Database, id: &str) -> AppResult<()> {
    let mut conn = db.conn()?;
    // Prevent deleting builtin modes
    let is_builtin: bool = conn
        .query_row(
            "SELECT is_builtin FROM context_modes WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )
        .unwrap_or(false);

    if is_builtin {
        return Err(crate::error::AppError::Storage(
            "Cannot delete the built-in General mode".into(),
        ));
    }

    // Cascade delete inside one transaction — a crash mid-way must not leave
    // orphaned dictionary entries or snippets pointing at a deleted mode.
    let tx = conn.transaction()?;
    tx.execute(
        "DELETE FROM dictionary_entries WHERE mode_id = ?1",
        params![id],
    )?;
    tx.execute("DELETE FROM snippets WHERE mode_id = ?1", params![id])?;
    tx.execute("DELETE FROM context_modes WHERE id = ?1", params![id])?;
    tx.commit()?;

    Ok(())
}
