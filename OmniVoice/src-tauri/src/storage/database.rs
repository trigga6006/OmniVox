use std::path::Path;

use crate::error::{AppError, AppResult};

/// SQLite database wrapper for persistent storage.
///
/// TODO: Replace stub with actual rusqlite connection management.
pub struct Database {
    _path: String,
}

impl Database {
    /// Initialize the database, creating tables if they don't exist.
    pub fn init(path: &Path) -> AppResult<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let path_str = path.to_str().unwrap_or("omnivox.db").to_string();

        let db = Self { _path: path_str };
        db.create_tables()?;

        Ok(db)
    }

    /// Create all required tables if they don't already exist.
    fn create_tables(&self) -> AppResult<()> {
        // TODO: Execute these via rusqlite::Connection
        let _statements = vec![
            "CREATE TABLE IF NOT EXISTS transcriptions (
                id TEXT PRIMARY KEY,
                text TEXT NOT NULL,
                duration_ms INTEGER NOT NULL,
                model_name TEXT NOT NULL,
                created_at TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS dictionary_entries (
                id TEXT PRIMARY KEY,
                phrase TEXT NOT NULL,
                replacement TEXT NOT NULL,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS snippets (
                id TEXT PRIMARY KEY,
                trigger TEXT NOT NULL,
                content TEXT NOT NULL,
                description TEXT,
                is_enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL
            )",
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        ];

        // TODO: Execute statements via rusqlite::Connection
        Ok(())
    }
}
