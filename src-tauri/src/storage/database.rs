use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::{AppError, AppResult};

/// Thread-safe SQLite database wrapper for persistent storage.
pub struct Database {
    conn: Mutex<Connection>,
}

impl Database {
    /// Initialize the database at `path`, creating parent directories and tables as needed.
    pub fn init(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;

        // Performance: WAL mode for concurrent reads, good for a desktop app
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Busy timeout: wait up to 5s if DB is locked
        conn.pragma_update(None, "busy_timeout", 5000)?;

        let db = Self {
            conn: Mutex::new(conn),
        };
        db.create_tables()?;
        Ok(db)
    }

    /// Get a reference to the connection, handling mutex poisoning gracefully.
    pub fn conn(&self) -> AppResult<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::Storage("Database lock poisoned".into()))
    }

    /// Create all required tables if they don't already exist.
    fn create_tables(&self) -> AppResult<()> {
        {
            let conn = self.conn()?;
            conn.execute_batch(
                "
                CREATE TABLE IF NOT EXISTS transcriptions (
                    id TEXT PRIMARY KEY NOT NULL,
                    text TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL,
                    model_name TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_transcriptions_created_at
                    ON transcriptions(created_at DESC);

                CREATE TABLE IF NOT EXISTS dictionary_entries (
                    id TEXT PRIMARY KEY NOT NULL,
                    phrase TEXT NOT NULL,
                    replacement TEXT NOT NULL,
                    is_enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS snippets (
                    id TEXT PRIMARY KEY NOT NULL,
                    trigger_text TEXT NOT NULL,
                    content TEXT NOT NULL,
                    description TEXT,
                    is_enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS vocabulary_entries (
                    id TEXT PRIMARY KEY NOT NULL,
                    word TEXT NOT NULL,
                    is_enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL,
                    mode_id TEXT REFERENCES context_modes(id)
                );

                CREATE TABLE IF NOT EXISTS context_modes (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL UNIQUE,
                    description TEXT NOT NULL DEFAULT '',
                    icon TEXT NOT NULL DEFAULT 'mic',
                    color TEXT NOT NULL DEFAULT 'amber',
                    llm_prompt TEXT NOT NULL,
                    sort_order INTEGER NOT NULL DEFAULT 0,
                    is_builtin INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS notes (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_notes_updated_at
                    ON notes(updated_at DESC);

                CREATE TABLE IF NOT EXISTS mode_app_bindings (
                    id TEXT PRIMARY KEY NOT NULL,
                    mode_id TEXT NOT NULL REFERENCES context_modes(id) ON DELETE CASCADE,
                    process_name TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_mode_app_bindings_mode_id
                    ON mode_app_bindings(mode_id);

                CREATE TABLE IF NOT EXISTS custom_voice_commands (
                    id TEXT PRIMARY KEY NOT NULL,
                    phrase TEXT NOT NULL UNIQUE,
                    action TEXT NOT NULL,
                    trigger_scope TEXT NOT NULL DEFAULT 'anywhere',
                    enabled INTEGER NOT NULL DEFAULT 1,
                    built_in INTEGER NOT NULL DEFAULT 0,
                    sort_order INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );

                PRAGMA user_version = 3;
            ",
            )?;
        } // drop conn guard before calling migrate which also needs the lock

        // Migration: add mode_id columns if they don't exist (safe to re-run)
        self.migrate_add_mode_id()?;

        // Migration: add writing_style column to context_modes if missing
        self.migrate_add_writing_style()?;

        // Migration: add raw_transcript column to transcriptions if missing
        self.migrate_add_raw_transcript()?;

        // Per-mode lookup indexes.  Created after migrate_add_mode_id because
        // older databases only gain the mode_id columns through that migration.
        {
            let conn = self.conn()?;
            conn.execute_batch(
                "
                CREATE INDEX IF NOT EXISTS idx_dictionary_mode_id
                    ON dictionary_entries(mode_id);
                CREATE INDEX IF NOT EXISTS idx_snippets_mode_id
                    ON snippets(mode_id);
                CREATE INDEX IF NOT EXISTS idx_vocabulary_mode_id
                    ON vocabulary_entries(mode_id);
            ",
            )?;
        }

        // Seed the built-in voice commands on first run (empty-table guarded).
        // Called after the conn guard above is dropped since it takes the lock.
        crate::storage::voice_commands::seed_defaults(self)?;

        Ok(())
    }

    /// Add `mode_id` column to dictionary_entries, snippets, and
    /// vocabulary_entries if missing.
    ///
    /// vocabulary_entries has shipped with `mode_id` inline since the table
    /// was introduced (v0.1.8), so its branch here is defensive — but the
    /// per-mode index created after the migrations references the column
    /// unconditionally, and a schema variant without it would otherwise
    /// fail `Database::init()` and brick startup.
    fn migrate_add_mode_id(&self) -> AppResult<()> {
        let conn = self.conn()?;

        // Check if column exists by querying table_info
        let has_mode_id = |table: &str| -> bool {
            conn.prepare(&format!("PRAGMA table_info({table})"))
                .and_then(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(1))
                        .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == "mode_id"))
                })
                .unwrap_or(false)
        };

        if !has_mode_id("dictionary_entries") {
            conn.execute_batch(
                "ALTER TABLE dictionary_entries ADD COLUMN mode_id TEXT REFERENCES context_modes(id);"
            )?;
        }
        if !has_mode_id("snippets") {
            conn.execute_batch(
                "ALTER TABLE snippets ADD COLUMN mode_id TEXT REFERENCES context_modes(id);",
            )?;
        }
        if !has_mode_id("vocabulary_entries") {
            conn.execute_batch(
                "ALTER TABLE vocabulary_entries ADD COLUMN mode_id TEXT REFERENCES context_modes(id);",
            )?;
        }

        Ok(())
    }

    /// Add `writing_style` column to context_modes if missing.
    fn migrate_add_writing_style(&self) -> AppResult<()> {
        let conn = self.conn()?;

        let has_col: bool = conn
            .prepare("PRAGMA table_info(context_modes)")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .map(|rows| {
                        rows.filter_map(|r| r.ok())
                            .any(|name| name == "writing_style")
                    })
            })
            .unwrap_or(false);

        if !has_col {
            conn.execute_batch(
                "ALTER TABLE context_modes ADD COLUMN writing_style TEXT NOT NULL DEFAULT 'formal';"
            )?;
        }

        Ok(())
    }

    /// Add `raw_transcript` column to transcriptions if missing.
    ///
    /// Stored nullable — pre-migration rows stay NULL and the read path
    /// treats NULL as "same as `text`".  Keeps the migration safe on
    /// upgrade: no rewrite of existing history, no UI surprises.
    fn migrate_add_raw_transcript(&self) -> AppResult<()> {
        let conn = self.conn()?;
        let has_col: bool = conn
            .prepare("PRAGMA table_info(transcriptions)")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .map(|rows| {
                        rows.filter_map(|r| r.ok())
                            .any(|name| name == "raw_transcript")
                    })
            })
            .unwrap_or(false);

        if !has_col {
            conn.execute_batch("ALTER TABLE transcriptions ADD COLUMN raw_transcript TEXT;")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A database whose vocabulary_entries table predates mode scoping must
    /// be migrated before the per-mode index is created — otherwise
    /// `Database::init()` fails with "no such column: mode_id" and the app
    /// never starts.
    #[test]
    fn init_migrates_legacy_vocabulary_table_before_indexing() {
        let dir = std::env::temp_dir().join(format!("omnivox-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.db");

        // Simulate the legacy schema: vocabulary_entries without mode_id.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE vocabulary_entries (
                    id TEXT PRIMARY KEY NOT NULL,
                    word TEXT NOT NULL,
                    is_enabled INTEGER NOT NULL DEFAULT 1,
                    created_at TEXT NOT NULL
                );",
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("init must migrate the legacy schema");

        let conn = db.conn().unwrap();
        let has_mode_id: bool = conn
            .prepare("PRAGMA table_info(vocabulary_entries)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .any(|name| name == "mode_id");
        assert!(has_mode_id, "mode_id column should be added by migration");

        let has_index: bool = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name='idx_vocabulary_mode_id'")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .next()
            .is_some();
        assert!(has_index, "per-mode vocabulary index should exist");

        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }
}
