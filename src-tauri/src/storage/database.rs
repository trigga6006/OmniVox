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

            PRAGMA user_version = 1;
        ",
        )?;
        Ok(())
    }
}
