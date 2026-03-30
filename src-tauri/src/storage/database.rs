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

                PRAGMA user_version = 3;
            ",
            )?;
        } // drop conn guard before calling migrate which also needs the lock

        // Migration: add mode_id columns if they don't exist (safe to re-run)
        self.migrate_add_mode_id()?;

        // Migration: add writing_style column to context_modes if missing
        self.migrate_add_writing_style()?;

        Ok(())
    }

    /// Add `mode_id` column to dictionary_entries and snippets if missing.
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
                "ALTER TABLE snippets ADD COLUMN mode_id TEXT REFERENCES context_modes(id);"
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
}
