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
        let is_new_database = !path.exists();
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
        db.seed_history_retention_default(is_new_database)?;
        Ok(db)
    }

    /// New installations get a useful but finite transcript window. Existing
    /// databases that predate the setting retain the legacy unlimited policy;
    /// an explicitly persisted user choice is never overwritten.
    fn seed_history_retention_default(&self, is_new_database: bool) -> AppResult<()> {
        let days = if is_new_database {
            crate::storage::types::DEFAULT_NEW_INSTALL_HISTORY_RETENTION_DAYS
        } else {
            0
        };
        self.conn()?.execute(
            "INSERT OR IGNORE INTO settings (key, value) VALUES ('history_retention_days', ?1)",
            [days.to_string()],
        )?;
        Ok(())
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
                    created_at TEXT NOT NULL,
                    word_count INTEGER NOT NULL DEFAULT 0
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
                    structured_profile TEXT NOT NULL DEFAULT '',
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

                CREATE TABLE IF NOT EXISTS scratchpad_note (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    content TEXT NOT NULL DEFAULT '',
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS scratchpad_pads (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    position INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE IF NOT EXISTS scratchpad_entries (
                    id TEXT PRIMARY KEY NOT NULL,
                    pad_id TEXT NOT NULL REFERENCES scratchpad_pads(id) ON DELETE CASCADE,
                    content TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_scratchpad_entries_pad
                    ON scratchpad_entries(pad_id, created_at DESC);

                CREATE TABLE IF NOT EXISTS meetings (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    source_app TEXT,
                    started_at TEXT NOT NULL,
                    ended_at TEXT,
                    user_notes TEXT NOT NULL DEFAULT '',
                    summary_markdown TEXT NOT NULL DEFAULT '',
                    summary_json TEXT,
                    summary_provider TEXT,
                    summary_model TEXT,
                    summary_cost REAL,
                    prompt_tokens INTEGER,
                    completion_tokens INTEGER,
                    is_favorite INTEGER NOT NULL DEFAULT 0,
                    tags_json TEXT NOT NULL DEFAULT '[]',
                    completed_actions_json TEXT NOT NULL DEFAULT '[]',
                    summary_stale INTEGER NOT NULL DEFAULT 0,
                    error TEXT,
                    deleted_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    ai_model_override TEXT,
                    summary_preset_override TEXT,
                    summary_instructions TEXT NOT NULL DEFAULT '',
                    agenda TEXT NOT NULL DEFAULT '',
                    participants_json TEXT NOT NULL DEFAULT '[]',
                    previous_meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
                    actions_materialized INTEGER NOT NULL DEFAULT 0
                );

                CREATE INDEX IF NOT EXISTS idx_meetings_started_at
                    ON meetings(started_at DESC);

                CREATE TABLE IF NOT EXISTS meeting_templates (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                    title TEXT NOT NULL,
                    agenda TEXT NOT NULL DEFAULT '',
                    participants_json TEXT NOT NULL DEFAULT '[]',
                    ai_model_override TEXT,
                    summary_preset_override TEXT,
                    summary_instructions TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_templates_name
                    ON meeting_templates(name COLLATE NOCASE);

                CREATE TABLE IF NOT EXISTS meeting_chunks (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    source TEXT NOT NULL,
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL,
                    audio_path TEXT NOT NULL,
                    status TEXT NOT NULL DEFAULT 'pending',
                    error TEXT,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_chunks_pending
                    ON meeting_chunks(meeting_id, status, start_ms);

                CREATE TABLE IF NOT EXISTS meeting_segments (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    chunk_id TEXT REFERENCES meeting_chunks(id) ON DELETE SET NULL,
                    source TEXT NOT NULL,
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    speaker_label TEXT,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_segments_timeline
                    ON meeting_segments(meeting_id, start_ms, source);

                CREATE TABLE IF NOT EXISTS meeting_markers (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    at_ms INTEGER NOT NULL,
                    label TEXT NOT NULL DEFAULT '',
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_markers_timeline
                    ON meeting_markers(meeting_id, at_ms);

                CREATE TABLE IF NOT EXISTS meeting_questions (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    question TEXT NOT NULL,
                    answer TEXT NOT NULL,
                    segment_refs_json TEXT NOT NULL DEFAULT '[]',
                    provider TEXT NOT NULL,
                    model TEXT NOT NULL,
                    cost REAL NOT NULL,
                    prompt_tokens INTEGER NOT NULL DEFAULT 0,
                    completion_tokens INTEGER NOT NULL DEFAULT 0,
                    context_segments INTEGER NOT NULL DEFAULT 0,
                    total_segments INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_questions_timeline
                    ON meeting_questions(meeting_id, created_at, id);

                CREATE TABLE IF NOT EXISTS meeting_summary_versions (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    markdown TEXT NOT NULL,
                    summary_json TEXT,
                    provider TEXT,
                    model TEXT,
                    cost REAL,
                    prompt_tokens INTEGER,
                    completion_tokens INTEGER,
                    origin TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_summary_versions_timeline
                    ON meeting_summary_versions(meeting_id, created_at DESC, id DESC);

                CREATE TABLE IF NOT EXISTS meeting_action_items (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                    task TEXT NOT NULL,
                    owner TEXT,
                    due TEXT,
                    segment_refs_json TEXT NOT NULL DEFAULT '[]',
                    origin TEXT NOT NULL DEFAULT 'manual',
                    source_task TEXT,
                    completed INTEGER NOT NULL DEFAULT 0,
                    dismissed_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_action_items_open
                    ON meeting_action_items(meeting_id, dismissed_at, completed, created_at);

                CREATE TABLE IF NOT EXISTS meeting_provider_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    provider TEXT NOT NULL DEFAULT 'openrouter',
                    model TEXT NOT NULL DEFAULT 'deepseek/deepseek-v4-flash',
                    monthly_budget REAL NOT NULL DEFAULT 2.0,
                    per_meeting_budget REAL NOT NULL DEFAULT 0.05,
                    max_prompt_price REAL NOT NULL DEFAULT 0.5,
                    max_completion_price REAL NOT NULL DEFAULT 3.0,
                    zdr_only INTEGER NOT NULL DEFAULT 1,
                    deny_data_collection INTEGER NOT NULL DEFAULT 1,
                    auto_suggest INTEGER NOT NULL DEFAULT 1,
                    auto_summarize INTEGER NOT NULL DEFAULT 0,
                    summary_preset TEXT NOT NULL DEFAULT 'general',
                    custom_instructions TEXT NOT NULL DEFAULT '',
                    transcription_mode TEXT NOT NULL DEFAULT 'after_meeting',
                    routing_preference TEXT NOT NULL DEFAULT 'price',
                    updated_at TEXT NOT NULL
                );

                INSERT OR IGNORE INTO meeting_provider_settings
                    (id, updated_at) VALUES (1, datetime('now'));

                CREATE TABLE IF NOT EXISTS meeting_ai_usage (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL,
                    provider TEXT NOT NULL,
                    model TEXT NOT NULL,
                    cost REAL NOT NULL,
                    prompt_tokens INTEGER NOT NULL DEFAULT 0,
                    completion_tokens INTEGER NOT NULL DEFAULT 0,
                    request_kind TEXT NOT NULL DEFAULT 'legacy',
                    created_at TEXT NOT NULL
                );

                CREATE INDEX IF NOT EXISTS idx_meeting_ai_usage_created_at
                    ON meeting_ai_usage(created_at DESC);

            ",
            )?;
        } // drop conn guard before calling migrate which also needs the lock

        // Migration: add mode_id columns if they don't exist (safe to re-run)
        self.migrate_add_mode_id()?;

        // Migration: add writing_style column to context_modes if missing
        self.migrate_add_writing_style()?;

        // Migration: repurpose the vestigial llm_prompt column as the
        // Structured Mode profile id
        self.migrate_llm_prompt_to_structured_profile()?;

        // Migration: add raw_transcript column to transcriptions if missing
        self.migrate_add_raw_transcript()?;

        // Migration v4: persist exact per-row word counts and maintain an O(1)
        // aggregate row for the completion-time stats refresh.
        self.migrate_history_stats_v4()?;

        // Meeting Mode evolves independently of the older global schema
        // version. Repair shipped databases column-by-column so development
        // builds and future upgrades remain idempotent.
        self.migrate_meeting_mode_columns()?;

        // Convert structured-summary action items into durable local records.
        // The migration flag keeps user edits and dismissals authoritative on
        // every subsequent launch.
        crate::storage::meetings::backfill_generated_action_items(self)?;

        // Seed the immutable notes ledger for meetings created before summary
        // versioning shipped. The backfill is existence-guarded per meeting.
        crate::storage::meetings::backfill_summary_versions(self)?;

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

        // Seed the built-in voice commands on first run (empty-table guarded),
        // then backfill any built-ins added by app updates since the first
        // seed. Called after the conn guard above is dropped since both take
        // the lock.
        crate::storage::voice_commands::seed_defaults(self)?;
        crate::storage::voice_commands::seed_missing_builtins(self)?;

        // Seed the always-present default scratchpad pad (idempotent).
        crate::storage::scratchpad::ensure_default_pad(self)?;

        Ok(())
    }

    fn migrate_meeting_mode_columns(&self) -> AppResult<()> {
        let conn = self.conn()?;
        let has_col = |table: &str, name: &str| -> bool {
            conn.prepare(&format!("PRAGMA table_info({table})"))
                .and_then(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(1))
                        .map(|rows| rows.filter_map(Result::ok).any(|column| column == name))
                })
                .unwrap_or(false)
        };

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS meeting_summary_versions (
                id TEXT PRIMARY KEY NOT NULL,
                meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
                markdown TEXT NOT NULL,
                summary_json TEXT,
                provider TEXT,
                model TEXT,
                cost REAL,
                prompt_tokens INTEGER,
                completion_tokens INTEGER,
                origin TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_meeting_summary_versions_timeline
                ON meeting_summary_versions(meeting_id, created_at DESC, id DESC);",
        )?;

        if !has_col("meetings", "is_favorite") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN is_favorite INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !has_col("meetings", "tags_json") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        if !has_col("meetings", "summary_stale") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN summary_stale INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !has_col("meetings", "completed_actions_json") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN completed_actions_json TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        if !has_col("meetings", "deleted_at") {
            conn.execute_batch("ALTER TABLE meetings ADD COLUMN deleted_at TEXT;")?;
        }
        if !has_col("meetings", "ai_model_override") {
            conn.execute_batch("ALTER TABLE meetings ADD COLUMN ai_model_override TEXT;")?;
        }
        if !has_col("meetings", "summary_preset_override") {
            conn.execute_batch("ALTER TABLE meetings ADD COLUMN summary_preset_override TEXT;")?;
        }
        if !has_col("meetings", "summary_instructions") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN summary_instructions TEXT NOT NULL DEFAULT '';",
            )?;
        }
        if !has_col("meetings", "agenda") {
            conn.execute_batch("ALTER TABLE meetings ADD COLUMN agenda TEXT NOT NULL DEFAULT '';")?;
        }
        if !has_col("meetings", "participants_json") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN participants_json TEXT NOT NULL DEFAULT '[]';",
            )?;
        }
        if !has_col("meetings", "previous_meeting_id") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN previous_meeting_id TEXT REFERENCES meetings(id) ON DELETE SET NULL;",
            )?;
        }
        if !has_col("meetings", "actions_materialized") {
            conn.execute_batch(
                "ALTER TABLE meetings ADD COLUMN actions_materialized INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !has_col("meeting_action_items", "source_task") {
            conn.execute_batch("ALTER TABLE meeting_action_items ADD COLUMN source_task TEXT;")?;
        }
        conn.execute_batch(
            "UPDATE meeting_action_items SET source_task=task WHERE origin='ai' AND source_task IS NULL;",
        )?;
        if !has_col("meeting_ai_usage", "request_kind") {
            conn.execute_batch(
                "ALTER TABLE meeting_ai_usage ADD COLUMN request_kind TEXT NOT NULL DEFAULT 'legacy';",
            )?;
        }
        if !has_col("meeting_provider_settings", "auto_summarize") {
            // A database from before this column never recorded a choice, so it
            // must land on the same default a fresh install gets: off, with
            // notes generated from the drawer's CTA.
            conn.execute_batch(
                "ALTER TABLE meeting_provider_settings ADD COLUMN auto_summarize INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !has_col("meeting_provider_settings", "summary_preset") {
            conn.execute_batch(
                "ALTER TABLE meeting_provider_settings ADD COLUMN summary_preset TEXT NOT NULL DEFAULT 'general';",
            )?;
        }
        if !has_col("meeting_provider_settings", "custom_instructions") {
            conn.execute_batch(
                "ALTER TABLE meeting_provider_settings ADD COLUMN custom_instructions TEXT NOT NULL DEFAULT '';",
            )?;
        }
        if !has_col("meeting_provider_settings", "transcription_mode") {
            conn.execute_batch(
                "ALTER TABLE meeting_provider_settings ADD COLUMN transcription_mode TEXT NOT NULL DEFAULT 'after_meeting';",
            )?;
        }
        if !has_col("meeting_provider_settings", "routing_preference") {
            conn.execute_batch(
                "ALTER TABLE meeting_provider_settings ADD COLUMN routing_preference TEXT NOT NULL DEFAULT 'price';",
            )?;
        }
        if !has_col("meeting_questions", "context_segments") {
            conn.execute_batch(
                "ALTER TABLE meeting_questions ADD COLUMN context_segments INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !has_col("meeting_questions", "total_segments") {
            conn.execute_batch(
                "ALTER TABLE meeting_questions ADD COLUMN total_segments INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        if !has_col("meeting_segments", "speaker_label") {
            conn.execute_batch("ALTER TABLE meeting_segments ADD COLUMN speaker_label TEXT;")?;
        }
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_meetings_deleted_started
             ON meetings(deleted_at, started_at DESC);",
        )?;
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

    /// Rename `context_modes.llm_prompt` to `structured_profile` if the old
    /// column is still present.
    ///
    /// `llm_prompt` was vestigial — written by the builtin-mode seeds but
    /// never read by extraction — and now holds the Structured Mode profile
    /// id ("agent-prompt" / "email" / "notes-outline"; empty = default).
    /// Existing rows carry stale seed text, so the migration clears the
    /// column after renaming; unknown values would degrade to the default
    /// profile anyway, but starting clean keeps the DB inspectable.
    fn migrate_llm_prompt_to_structured_profile(&self) -> AppResult<()> {
        let conn = self.conn()?;

        let has_col = |name: &str| -> bool {
            conn.prepare("PRAGMA table_info(context_modes)")
                .and_then(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(1))
                        .map(|rows| rows.filter_map(|r| r.ok()).any(|col| col == name))
                })
                .unwrap_or(false)
        };

        if has_col("llm_prompt") && !has_col("structured_profile") {
            conn.execute_batch(
                "ALTER TABLE context_modes RENAME COLUMN llm_prompt TO structured_profile;
                 UPDATE context_modes SET structured_profile = '';",
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

    /// Version 4: add exact word counts to history rows and seed the singleton
    /// aggregate used by `get_dictation_stats`.
    ///
    /// The column add, Rust `split_whitespace` backfill, aggregate rebuild, and
    /// version bump are one transaction.  That makes an interrupted migration
    /// all-or-nothing.  Schema checks remain in place so a partially-created or
    /// manually repaired database is recovered even if its `user_version` is
    /// already 4.
    fn migrate_history_stats_v4(&self) -> AppResult<()> {
        let mut conn = self.conn()?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        let has_word_count = conn
            .prepare("PRAGMA table_info(transcriptions)")
            .and_then(|mut stmt| {
                stmt.query_map([], |row| row.get::<_, String>(1))
                    .map(|rows| rows.filter_map(|r| r.ok()).any(|name| name == "word_count"))
            })
            .unwrap_or(false);
        let has_stats_table: bool = conn.query_row(
            "SELECT EXISTS(
                SELECT 1 FROM sqlite_master
                WHERE type = 'table' AND name = 'dictation_stats'
            )",
            [],
            |row| row.get(0),
        )?;
        let has_stats_row = has_stats_table
            && conn
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM dictation_stats WHERE singleton = 1)",
                    [],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(false);

        if version >= 4 && has_word_count && has_stats_table && has_stats_row {
            return Ok(());
        }

        let tx = conn.transaction()?;
        if !has_word_count {
            tx.execute_batch(
                "ALTER TABLE transcriptions
                 ADD COLUMN word_count INTEGER NOT NULL DEFAULT 0;",
            )?;
        }
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS dictation_stats (
                singleton INTEGER PRIMARY KEY NOT NULL CHECK (singleton = 1),
                total_words INTEGER NOT NULL DEFAULT 0,
                total_transcriptions INTEGER NOT NULL DEFAULT 0,
                total_duration_ms INTEGER NOT NULL DEFAULT 0
            );",
        )?;

        // Recompute every row whenever v4 is incomplete.  This repairs the
        // realistic interrupted/manual state where the column exists with its
        // default zeros but the aggregate row or version bump is missing.
        let rows: Vec<(String, String)> = {
            let mut stmt = tx.prepare("SELECT id, text FROM transcriptions")?;
            let rows = stmt
                .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        for (id, text) in rows {
            let words = text.split_whitespace().count() as i64;
            tx.execute(
                "UPDATE transcriptions SET word_count = ?1 WHERE id = ?2",
                rusqlite::params![words, id],
            )?;
        }

        tx.execute(
            "INSERT INTO dictation_stats
                (singleton, total_words, total_transcriptions, total_duration_ms)
             SELECT 1,
                    COALESCE(SUM(word_count), 0),
                    COUNT(*) FILTER (WHERE word_count > 0),
                    COALESCE(SUM(duration_ms) FILTER (WHERE word_count > 0), 0)
             FROM transcriptions
             WHERE true
             ON CONFLICT(singleton) DO UPDATE SET
                total_words = excluded.total_words,
                total_transcriptions = excluded.total_transcriptions,
                total_duration_ms = excluded.total_duration_ms",
            [],
        )?;
        // Never downgrade a database produced by a newer build while
        // repairing a missing v4 artifact.
        tx.pragma_update(None, "user_version", version.max(4))?;
        tx.commit()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_database_path(label: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("omnivox-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.db");
        (dir, path)
    }

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

    /// A database from before the profile column carries `llm_prompt` with
    /// stale seed text.  Init must rename it to `structured_profile` and
    /// clear the stale values so they can't be misread as profile ids.
    #[test]
    fn init_renames_llm_prompt_to_structured_profile_and_clears_it() {
        let dir = std::env::temp_dir().join(format!("omnivox-db-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("legacy.db");

        // Simulate the pre-profile schema with one mode carrying seed text.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE context_modes (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL UNIQUE,
                    description TEXT NOT NULL DEFAULT '',
                    icon TEXT NOT NULL DEFAULT 'mic',
                    color TEXT NOT NULL DEFAULT 'amber',
                    llm_prompt TEXT NOT NULL,
                    sort_order INTEGER NOT NULL DEFAULT 0,
                    is_builtin INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    writing_style TEXT NOT NULL DEFAULT 'formal'
                );
                INSERT INTO context_modes
                    (id, name, description, icon, color, llm_prompt, sort_order,
                     is_builtin, created_at, updated_at, writing_style)
                VALUES
                    ('m1', 'Programming', '', 'code', 'blue',
                     '- Recognize programming terms…', 1, 1, '2025', '2025', 'formal');",
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("init must migrate the legacy schema");

        let conn = db.conn().unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(context_modes)")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(1))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(
            cols.iter().any(|c| c == "structured_profile"),
            "structured_profile column should exist after migration"
        );
        assert!(
            !cols.iter().any(|c| c == "llm_prompt"),
            "llm_prompt column should be gone after migration"
        );

        let value: String = conn
            .query_row(
                "SELECT structured_profile FROM context_modes WHERE id = 'm1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(value, "", "stale seed text must be cleared");

        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v4_migrates_historical_rows_with_exact_whitespace_counts() {
        let (dir, path) = temp_database_path("history-v4-legacy");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE transcriptions (
                    id TEXT PRIMARY KEY NOT NULL,
                    text TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL,
                    model_name TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );
                PRAGMA user_version = 3;",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transcriptions
                    (id, text, duration_ms, model_name, created_at)
                 VALUES (?1, ?2, ?3, 'legacy', '2026-01-01T00:00:00Z')",
                rusqlite::params!["one", "alpha  beta\ngamma\t delta", 125_i64],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO transcriptions
                    (id, text, duration_ms, model_name, created_at)
                 VALUES (?1, ?2, ?3, 'legacy', '2026-01-02T00:00:00Z')",
                rusqlite::params!["empty", "  \n\t ", 999_i64],
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("legacy history must migrate");
        let conn = db.conn().unwrap();
        let counts: Vec<(String, i64)> = {
            let mut stmt = conn
                .prepare("SELECT id, word_count FROM transcriptions ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        assert_eq!(counts, vec![("empty".into(), 0), ("one".into(), 4)]);
        let totals: (i64, i64, i64) = conn
            .query_row(
                "SELECT total_words, total_transcriptions, total_duration_ms
                 FROM dictation_stats WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(totals, (4, 1, 125));
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 4);

        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v4_repairs_partial_schema_even_when_version_was_already_bumped() {
        let (dir, path) = temp_database_path("history-v4-partial");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE transcriptions (
                    id TEXT PRIMARY KEY NOT NULL,
                    text TEXT NOT NULL,
                    duration_ms INTEGER NOT NULL,
                    model_name TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    raw_transcript TEXT,
                    word_count INTEGER NOT NULL DEFAULT 0
                );
                INSERT INTO transcriptions
                    (id, text, duration_ms, model_name, created_at, word_count)
                VALUES
                    ('partial', 'one   two', 80, 'legacy',
                     '2026-01-01T00:00:00Z', 0);
                PRAGMA user_version = 4;",
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("partial v4 state must be repaired");
        let conn = db.conn().unwrap();
        let word_count: i64 = conn
            .query_row(
                "SELECT word_count FROM transcriptions WHERE id = 'partial'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let totals: (i64, i64, i64) = conn
            .query_row(
                "SELECT total_words, total_transcriptions, total_duration_ms
                 FROM dictation_stats WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(word_count, 2);
        assert_eq!(totals, (2, 1, 80));

        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v4_migration_is_idempotent_across_reopen() {
        let (dir, path) = temp_database_path("history-v4-idempotent");
        {
            let db = Database::init(&path).unwrap();
            let conn = db.conn().unwrap();
            conn.execute(
                "INSERT INTO transcriptions
                    (id, text, duration_ms, model_name, created_at,
                     raw_transcript, word_count)
                 VALUES ('stable', 'three exact words', 42, 'test',
                         '2026-01-01T00:00:00Z', NULL, 3)",
                [],
            )
            .unwrap();
            conn.execute(
                "UPDATE dictation_stats SET
                    total_words = 3,
                    total_transcriptions = 1,
                    total_duration_ms = 42
                 WHERE singleton = 1",
                [],
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("completed v4 migration must be repeatable");
        let conn = db.conn().unwrap();
        let totals: (i64, i64, i64) = conn
            .query_row(
                "SELECT total_words, total_transcriptions, total_duration_ms
                 FROM dictation_stats WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(totals, (3, 1, 42));

        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn meeting_question_context_columns_repair_existing_databases() {
        let (dir, path) = temp_database_path("meeting-question-context");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meetings (
                    id TEXT PRIMARY KEY NOT NULL,
                    title TEXT NOT NULL,
                    status TEXT NOT NULL,
                    source_app TEXT,
                    started_at TEXT NOT NULL,
                    ended_at TEXT,
                    user_notes TEXT NOT NULL DEFAULT '',
                    summary_markdown TEXT NOT NULL DEFAULT '',
                    summary_json TEXT,
                    summary_provider TEXT,
                    summary_model TEXT,
                    summary_cost REAL,
                    prompt_tokens INTEGER,
                    completion_tokens INTEGER,
                    is_favorite INTEGER NOT NULL DEFAULT 0,
                    tags_json TEXT NOT NULL DEFAULT '[]',
                    completed_actions_json TEXT NOT NULL DEFAULT '[]',
                    summary_stale INTEGER NOT NULL DEFAULT 0,
                    error TEXT,
                    deleted_at TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );
                CREATE TABLE meeting_questions (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL,
                    question TEXT NOT NULL,
                    answer TEXT NOT NULL,
                    segment_refs_json TEXT NOT NULL DEFAULT '[]',
                    provider TEXT NOT NULL,
                    model TEXT NOT NULL,
                    cost REAL NOT NULL,
                    prompt_tokens INTEGER NOT NULL DEFAULT 0,
                    completion_tokens INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL
                );
                CREATE TABLE meeting_segments (
                    id TEXT PRIMARY KEY NOT NULL,
                    meeting_id TEXT NOT NULL,
                    chunk_id TEXT,
                    source TEXT NOT NULL,
                    start_ms INTEGER NOT NULL,
                    end_ms INTEGER NOT NULL,
                    text TEXT NOT NULL,
                    created_at TEXT NOT NULL
                );",
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("meeting Q&A schema must be repaired");
        let conn = db.conn().unwrap();
        let columns = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(meeting_questions)")
                .unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(columns.iter().any(|column| column == "context_segments"));
        assert!(columns.iter().any(|column| column == "total_segments"));
        let segment_columns = {
            let mut stmt = conn.prepare("PRAGMA table_info(meeting_segments)").unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(segment_columns
            .iter()
            .any(|column| column == "speaker_label"));
        let meeting_columns = {
            let mut stmt = conn.prepare("PRAGMA table_info(meetings)").unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        for expected in [
            "ai_model_override",
            "summary_preset_override",
            "summary_instructions",
            "agenda",
            "participants_json",
            "previous_meeting_id",
        ] {
            assert!(meeting_columns.iter().any(|column| column == expected));
        }
        let usage_columns = {
            let mut stmt = conn.prepare("PRAGMA table_info(meeting_ai_usage)").unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(usage_columns.iter().any(|column| column == "request_kind"));
        let provider_columns = {
            let mut stmt = conn
                .prepare("PRAGMA table_info(meeting_provider_settings)")
                .unwrap();
            stmt.query_map([], |row| row.get::<_, String>(1))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };
        assert!(provider_columns
            .iter()
            .any(|column| column == "routing_preference"));

        drop(conn);
        drop(db);
        Database::init(&path).expect("meeting Q&A repair must be idempotent");
        std::fs::remove_dir_all(&dir).ok();
    }

    /// A database that predates the `auto_summarize` column belongs to a user
    /// who never opted into automatic notes, so the backfill must match the
    /// fresh-install default (off) rather than silently enabling paid calls.
    #[test]
    fn legacy_provider_settings_backfill_leaves_auto_summarize_off() {
        let (dir, path) = temp_database_path("provider-auto-summarize");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meeting_provider_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    provider TEXT NOT NULL DEFAULT 'openrouter',
                    model TEXT NOT NULL DEFAULT 'deepseek/deepseek-v4-flash',
                    monthly_budget REAL NOT NULL DEFAULT 2.0,
                    per_meeting_budget REAL NOT NULL DEFAULT 0.05,
                    max_prompt_price REAL NOT NULL DEFAULT 0.5,
                    max_completion_price REAL NOT NULL DEFAULT 3.0,
                    zdr_only INTEGER NOT NULL DEFAULT 1,
                    deny_data_collection INTEGER NOT NULL DEFAULT 1,
                    auto_suggest INTEGER NOT NULL DEFAULT 1,
                    updated_at TEXT NOT NULL
                );
                INSERT INTO meeting_provider_settings (id, updated_at)
                VALUES (1, '2026-01-01T00:00:00Z');",
            )
            .unwrap();
        }

        let db = Database::init(&path).expect("legacy provider settings must migrate");
        let conn = db.conn().unwrap();
        let auto_summarize: i64 = conn
            .query_row(
                "SELECT auto_summarize FROM meeting_provider_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(auto_summarize, 0);

        drop(conn);
        drop(db);
        std::fs::remove_dir_all(&dir).ok();
    }
}
