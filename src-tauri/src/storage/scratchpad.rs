//! Scratchpad storage — a quick voice-capture surface separate from Notes.
//!
//! Supports all three UX variants behind one live flag (`scratchpad_variant`):
//! - `note`    → one editable text blob (`scratchpad_note`, single row id=1)
//! - `entries` → timestamped cards in the default pad
//! - `pads`    → multiple named pads, each a list of entries
//!
//! `entries` and `pads` share `scratchpad_pads` + `scratchpad_entries`
//! (entries FK-cascade on pad delete). The variant flag and active-pad id are
//! stored as dedicated `settings` keys written atomically (single-key upsert),
//! NOT through the whole-`AppSettings` patch — so a scratchpad write can't
//! clobber a concurrent settings change from another window.

use crate::error::{AppError, AppResult};
use crate::storage::database::Database;
use chrono::Utc;
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use uuid::Uuid;

/// The always-present pad used by the `entries` variant and as the initial
/// active pad for the `pads` variant.
pub const DEFAULT_PAD_ID: &str = "default";
const VARIANT_KEY: &str = "scratchpad_variant";
const ACTIVE_PAD_KEY: &str = "scratchpad_active_pad_id";
const DEFAULT_VARIANT: &str = "entries";

#[derive(Debug, Clone, Serialize)]
pub struct ScratchpadEntry {
    pub id: String,
    pub pad_id: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScratchpadPad {
    pub id: String,
    pub name: String,
    pub position: i64,
    /// Newest-first.
    pub entries: Vec<ScratchpadEntry>,
}

/// Everything the scratchpad window needs in one round-trip, so it can render
/// any variant after a single `scratchpad_get` on mount (creation-time events
/// aren't replayed to a late-created window).
#[derive(Debug, Clone, Serialize)]
pub struct ScratchpadData {
    pub note: String,
    pub pads: Vec<ScratchpadPad>,
    pub active_pad_id: String,
    /// Current UX variant ("note" | "entries" | "pads") — folded in so the
    /// window loads everything it renders in a single round-trip.
    pub variant: String,
}

// ── settings k/v helpers (atomic single-key upserts) ─────────────────────────

fn get_setting(db: &Database, key: &str) -> AppResult<Option<String>> {
    let conn = db.conn()?;
    Ok(conn
        .query_row("SELECT value FROM settings WHERE key = ?1", params![key], |r| r.get(0))
        .optional()?)
}

fn set_setting(db: &Database, key: &str, value: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

pub fn get_variant(db: &Database) -> AppResult<String> {
    // "pads" was removed — migrate any legacy value to Cards ("entries").
    Ok(match get_setting(db, VARIANT_KEY)?.as_deref() {
        Some("note") => "note".to_string(),
        _ => DEFAULT_VARIANT.to_string(),
    })
}

pub fn set_variant(db: &Database, variant: &str) -> AppResult<()> {
    if !matches!(variant, "note" | "entries") {
        return Err(AppError::Storage(format!("unknown scratchpad variant: {variant}")));
    }
    set_setting(db, VARIANT_KEY, variant)
}

pub fn get_active_pad_id(db: &Database) -> AppResult<String> {
    Ok(get_setting(db, ACTIVE_PAD_KEY)?
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_PAD_ID.to_string()))
}

/// Saved window position in PHYSICAL pixels (so restore needs no scale math),
/// or None if the window has never been moved.
pub fn get_position(db: &Database) -> AppResult<Option<(f64, f64)>> {
    let x = get_setting(db, "scratchpad_pos_x")?.and_then(|v| v.parse::<f64>().ok());
    let y = get_setting(db, "scratchpad_pos_y")?.and_then(|v| v.parse::<f64>().ok());
    Ok(match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    })
}

pub fn set_position(db: &Database, x: f64, y: f64) -> AppResult<()> {
    // Windows reports (-32000,-32000) for minimized windows — persisting that
    // sentinel would restore the pad permanently off-screen.  Drop it no matter
    // which caller (close path, onMoved) let it through.
    if x <= -30000.0 || y <= -30000.0 {
        return Ok(());
    }
    set_setting(db, "scratchpad_pos_x", &x.to_string())?;
    set_setting(db, "scratchpad_pos_y", &y.to_string())?;
    Ok(())
}

/// Saved window inner size in PHYSICAL pixels (so restore needs no scale math),
/// or None if the window has never been resized.
pub fn get_size(db: &Database) -> AppResult<Option<(f64, f64)>> {
    let w = get_setting(db, "scratchpad_size_w")?.and_then(|v| v.parse::<f64>().ok());
    let h = get_setting(db, "scratchpad_size_h")?.and_then(|v| v.parse::<f64>().ok());
    Ok(match (w, h) {
        (Some(w), Some(h)) => Some((w, h)),
        _ => None,
    })
}

pub fn set_size(db: &Database, w: f64, h: f64) -> AppResult<()> {
    set_setting(db, "scratchpad_size_w", &w.to_string())?;
    set_setting(db, "scratchpad_size_h", &h.to_string())?;
    Ok(())
}


// ── note variant ─────────────────────────────────────────────────────────────

pub fn get_note(db: &Database) -> AppResult<String> {
    let conn = db.conn()?;
    Ok(conn
        .query_row("SELECT content FROM scratchpad_note WHERE id = 1", [], |r| r.get(0))
        .optional()?
        .unwrap_or_default())
}

pub fn set_note(db: &Database, content: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO scratchpad_note (id, content, updated_at) VALUES (1, ?1, ?2)
         ON CONFLICT(id) DO UPDATE SET content = ?1, updated_at = ?2",
        params![content, Utc::now().to_rfc3339()],
    )?;
    Ok(())
}

// ── pads + entries ───────────────────────────────────────────────────────────

/// Seed the always-present default pad. Called at DB init and defensively
/// before any entry insert (entries FK-reference a pad).
pub fn ensure_default_pad(db: &Database) -> AppResult<()> {
    let conn = db.conn()?;
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO scratchpad_pads (id, name, position, created_at, updated_at)
         VALUES (?1, 'Scratchpad', 0, ?2, ?2)",
        params![DEFAULT_PAD_ID, now],
    )?;
    Ok(())
}

pub fn list_pads(db: &Database) -> AppResult<Vec<ScratchpadPad>> {
    let conn = db.conn()?;

    let mut pad_stmt =
        conn.prepare("SELECT id, name, position FROM scratchpad_pads ORDER BY position, created_at")?;
    let mut pads: Vec<ScratchpadPad> = pad_stmt
        .query_map([], |row| {
            Ok(ScratchpadPad {
                id: row.get(0)?,
                name: row.get(1)?,
                position: row.get(2)?,
                entries: Vec::new(),
            })
        })?
        .collect::<Result<_, _>>()?;

    let mut entry_stmt = conn.prepare(
        "SELECT id, pad_id, content, created_at FROM scratchpad_entries ORDER BY created_at DESC",
    )?;
    let entries = entry_stmt.query_map([], |row| {
        Ok(ScratchpadEntry {
            id: row.get(0)?,
            pad_id: row.get(1)?,
            content: row.get(2)?,
            created_at: row.get(3)?,
        })
    })?;
    for e in entries {
        let e = e?;
        if let Some(pad) = pads.iter_mut().find(|p| p.id == e.pad_id) {
            pad.entries.push(e);
        }
    }
    Ok(pads)
}

/// Resolve `None`/empty to the default pad, and ensure the target pad exists so
/// the FK insert can't fail — a client holding a since-deleted active id falls
/// back to the default rather than erroring on append.
fn resolve_pad(db: &Database, pad_id: Option<&str>) -> AppResult<String> {
    if let Some(id) = pad_id.filter(|s| !s.is_empty()) {
        let exists = {
            let conn = db.conn()?;
            conn.query_row("SELECT 1 FROM scratchpad_pads WHERE id = ?1", params![id], |_| Ok(()))
                .optional()?
                .is_some()
        };
        if exists {
            return Ok(id.to_string());
        }
    }
    ensure_default_pad(db)?;
    Ok(DEFAULT_PAD_ID.to_string())
}

pub fn add_entry(db: &Database, pad_id: Option<&str>, content: &str) -> AppResult<ScratchpadEntry> {
    let pad_id = resolve_pad(db, pad_id)?;
    let id = Uuid::new_v4().to_string();
    let created_at = Utc::now().to_rfc3339();
    let conn = db.conn()?;
    conn.execute(
        "INSERT INTO scratchpad_entries (id, pad_id, content, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, pad_id, content, created_at],
    )?;
    Ok(ScratchpadEntry { id, pad_id, content: content.to_string(), created_at })
}

pub fn delete_entry(db: &Database, id: &str) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute("DELETE FROM scratchpad_entries WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn clear_pad(db: &Database, pad_id: Option<&str>) -> AppResult<()> {
    let pad_id = resolve_pad(db, pad_id)?;
    let conn = db.conn()?;
    conn.execute("DELETE FROM scratchpad_entries WHERE pad_id = ?1", params![pad_id])?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::database::Database;

    fn temp_db() -> (Database, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("omnivox-scratch-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::init(&dir.join("t.db")).unwrap();
        (db, dir)
    }

    #[test]
    fn default_pad_seeded() {
        let (db, _d) = temp_db();
        let data = get_data(&db).unwrap();
        assert!(data.pads.iter().any(|p| p.id == DEFAULT_PAD_ID));
    }

    #[test]
    fn get_data_repairs_dangling_active() {
        let (db, _d) = temp_db();
        // Point the active-pad setting at a non-existent pad directly.
        db.conn()
            .unwrap()
            .execute(
                "INSERT INTO settings (key, value) VALUES ('scratchpad_active_pad_id', 'ghost')
                 ON CONFLICT(key) DO UPDATE SET value = 'ghost'",
                [],
            )
            .unwrap();
        let data = get_data(&db).unwrap();
        assert_eq!(data.active_pad_id, DEFAULT_PAD_ID);
    }

    #[test]
    fn add_entry_falls_back_when_pad_missing() {
        let (db, _d) = temp_db();
        let entry = add_entry(&db, Some("ghost"), "hi").unwrap();
        assert_eq!(entry.pad_id, DEFAULT_PAD_ID);
    }

    #[test]
    fn variant_validated_migrated_and_persisted() {
        let (db, _d) = temp_db();
        assert_eq!(get_variant(&db).unwrap(), "entries"); // default
        assert!(set_variant(&db, "pads").is_err()); // removed variant rejected
        set_variant(&db, "note").unwrap();
        assert_eq!(get_variant(&db).unwrap(), "note");
        // A legacy stored "pads" migrates to Cards ("entries") on read.
        db.conn()
            .unwrap()
            .execute(
                "INSERT INTO settings (key, value) VALUES ('scratchpad_variant', 'pads')
                 ON CONFLICT(key) DO UPDATE SET value = 'pads'",
                [],
            )
            .unwrap();
        assert_eq!(get_variant(&db).unwrap(), "entries");
    }

    #[test]
    fn note_roundtrip() {
        let (db, _d) = temp_db();
        assert_eq!(get_note(&db).unwrap(), "");
        set_note(&db, "hello").unwrap();
        assert_eq!(get_note(&db).unwrap(), "hello");
        set_note(&db, "world").unwrap();
        assert_eq!(get_note(&db).unwrap(), "world");
    }

    #[test]
    fn entries_land_in_default_pad_and_delete() {
        let (db, _d) = temp_db();
        let e = add_entry(&db, None, "one").unwrap();
        assert_eq!(e.pad_id, DEFAULT_PAD_ID);
        let data = get_data(&db).unwrap();
        let def = data.pads.iter().find(|p| p.id == DEFAULT_PAD_ID).unwrap();
        assert_eq!(def.entries.len(), 1);
        delete_entry(&db, &e.id).unwrap();
        let data = get_data(&db).unwrap();
        let def = data.pads.iter().find(|p| p.id == DEFAULT_PAD_ID).unwrap();
        assert!(def.entries.is_empty());
    }
}

/// One-shot fetch of everything the window renders.
pub fn get_data(db: &Database) -> AppResult<ScratchpadData> {
    ensure_default_pad(db)?;
    let pads = list_pads(db)?;
    let mut active = get_active_pad_id(db)?;
    // Repair a dangling active pad (e.g. it was deleted while the setting still
    // pointed at it) so the client never holds an id that would FK-fail.
    if !pads.iter().any(|p| p.id == active) {
        active = DEFAULT_PAD_ID.to_string();
        set_setting(db, ACTIVE_PAD_KEY, &active)?;
    }
    Ok(ScratchpadData {
        note: get_note(db)?,
        pads,
        active_pad_id: active,
        variant: get_variant(db)?,
    })
}
