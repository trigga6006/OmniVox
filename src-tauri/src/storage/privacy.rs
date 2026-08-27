//! Privacy-focused transcript retention and bounded cleanup scheduling.

use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Transaction};
use std::sync::atomic::Ordering;
use tauri::{Emitter, Manager};

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::storage::database::Database;

const CLEANUP_INTERVAL_SECONDS: u64 = 24 * 60 * 60;

/// Apply the complete history policy. Disabled history securely clears all
/// rows; zero retention days means history is enabled and kept forever.
pub fn enforce_transcript_policy(
    db: &Database,
    history_enabled: bool,
    retention_days: u32,
    now: DateTime<Utc>,
) -> AppResult<usize> {
    if !history_enabled {
        purge_all_transcripts(db)
    } else if retention_days == 0 {
        Ok(0)
    } else {
        purge_transcripts_older_than(db, retention_days, now)
    }
}

/// Permanently remove every transcript and truncate recoverable WAL content.
pub fn purge_all_transcripts(db: &Database) -> AppResult<usize> {
    purge_where(db, None)
}

/// Remove transcripts older than `retention_days` relative to `now`.
pub fn purge_transcripts_older_than(
    db: &Database,
    retention_days: u32,
    now: DateTime<Utc>,
) -> AppResult<usize> {
    if retention_days == 0 {
        return Err(AppError::Storage(
            "Transcript retention must be at least one day".into(),
        ));
    }
    if retention_days > crate::storage::types::MAX_HISTORY_RETENTION_DAYS {
        return Err(AppError::Storage(format!(
            "Transcript retention cannot exceed {} days",
            crate::storage::types::MAX_HISTORY_RETENTION_DAYS
        )));
    }
    let duration = Duration::try_days(retention_days as i64)
        .ok_or_else(|| AppError::Storage("Transcript retention is too large".into()))?;
    purge_where(db, Some(now - duration))
}

/// Schedule cleanup without delaying dictation or startup. Normal requests run
/// at most daily; forced requests (startup or a policy change) are coalesced
/// and retried once an in-flight cleanup finishes.
pub fn schedule_retention_cleanup(app: &tauri::AppHandle, force: bool) {
    let state = app.state::<AppState>();
    let now_epoch = Utc::now().timestamp().max(0) as u64;
    let last = state.history_last_purge_epoch.load(Ordering::Acquire);
    if !force && last != 0 && now_epoch.saturating_sub(last) < CLEANUP_INTERVAL_SECONDS {
        return;
    }

    state.history_purge_requested.store(true, Ordering::Release);
    if state
        .history_purge_in_progress
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }

    let handle = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        let (result, now_epoch) = {
            let state = handle.state::<AppState>();
            state
                .history_purge_requested
                .store(false, Ordering::Release);
            let now = Utc::now();
            // Keep the settings read lock through the DB transaction. This
            // serializes enable/disable changes with cleanup: enabling history
            // cannot write a new row while an older disabled-policy purge is
            // still clearing the database, and disabling cannot race a final
            // pre-disable save after the purge.
            let result = match state.settings.read() {
                Ok(settings) => enforce_transcript_policy(
                    &state.db,
                    settings.values().history_enabled,
                    settings.values().history_retention_days,
                    now,
                ),
                Err(_) => Err(AppError::Storage(
                    "Settings lock poisoned; transcript cleanup deferred".into(),
                )),
            };
            (result, now.timestamp().max(0) as u64)
        };

        match result {
            Ok(deleted) => {
                let state = handle.state::<AppState>();
                state
                    .history_last_purge_epoch
                    .store(now_epoch, Ordering::Release);
                if deleted > 0 {
                    let _ = handle.emit("history-changed", deleted);
                }
            }
            Err(error) => {
                eprintln!("Transcript retention cleanup failed: {error}");
                let _ = handle.emit("history-cleanup-error", error.to_string());
            }
        }

        let rerun = {
            let state = handle.state::<AppState>();
            state
                .history_purge_in_progress
                .store(false, Ordering::Release);
            state.history_purge_requested.load(Ordering::Acquire)
        };
        if rerun {
            schedule_retention_cleanup(&handle, true);
        }
    }));
}

fn purge_where(db: &Database, cutoff: Option<DateTime<Utc>>) -> AppResult<usize> {
    let purge_all = cutoff.is_none();
    let mut conn = db.conn()?;
    // Overwrite deleted payload bytes in ordinary database pages instead of
    // leaving transcript text on SQLite's freelist.
    conn.pragma_update(None, "secure_delete", "ON")?;
    let tx = conn.transaction()?;
    let (words, counted, duration) = removed_totals(&tx, cutoff)?;
    let deleted = match cutoff {
        Some(cutoff) => tx.execute(
            "DELETE FROM transcriptions WHERE created_at < ?1",
            [cutoff.to_rfc3339()],
        )?,
        None => tx.execute("DELETE FROM transcriptions", [])?,
    };
    if deleted > 0 {
        tx.execute(
            "UPDATE dictation_stats SET
                total_words = MAX(0, total_words - ?1),
                total_transcriptions = MAX(0, total_transcriptions - ?2),
                total_duration_ms = MAX(0, total_duration_ms - ?3)
             WHERE singleton = 1",
            params![words, counted, duration],
        )?;
    }
    tx.commit()?;
    // WAL can retain older page images even after secure_delete updates the
    // main database. A disabled-history purge always truncates it, even when
    // the table is already empty: the previous process may have crashed after
    // committing its DELETE but before reaching this checkpoint.
    if deleted > 0 || purge_all {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    }
    Ok(deleted)
}

fn removed_totals(
    tx: &Transaction<'_>,
    cutoff: Option<DateTime<Utc>>,
) -> AppResult<(i64, i64, i64)> {
    let sql = |where_clause: &str| {
        format!(
            "SELECT
                COALESCE(SUM(word_count), 0),
                COALESCE(SUM(CASE WHEN word_count > 0 THEN 1 ELSE 0 END), 0),
                COALESCE(SUM(CASE WHEN word_count > 0 THEN duration_ms ELSE 0 END), 0)
             FROM transcriptions {where_clause}"
        )
    };
    match cutoff {
        Some(cutoff) => Ok(tx.query_row(
            &sql("WHERE created_at < ?1"),
            [cutoff.to_rfc3339()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?),
        None => Ok(tx.query_row(&sql(""), [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> (Database, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("omnivox-retention-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::init(&dir.join("omnivox.db")).unwrap();
        (db, dir)
    }

    fn insert(db: &Database, id: &str, created_at: DateTime<Utc>, words: i64, duration: i64) {
        let conn = db.conn().unwrap();
        conn.execute(
            "INSERT INTO transcriptions
             (id, text, duration_ms, model_name, created_at, word_count)
             VALUES (?1, ?2, ?3, 'test', ?4, ?5)",
            params![
                id,
                "sensitive test text",
                duration,
                created_at.to_rfc3339(),
                words
            ],
        )
        .unwrap();
        if words > 0 {
            conn.execute(
                "UPDATE dictation_stats SET
                    total_words = total_words + ?1,
                    total_transcriptions = total_transcriptions + 1,
                    total_duration_ms = total_duration_ms + ?2
                 WHERE singleton = 1",
                params![words, duration],
            )
            .unwrap();
        }
    }

    fn stats(db: &Database) -> (i64, i64, i64) {
        db.conn()
            .unwrap()
            .query_row(
                "SELECT total_words, total_transcriptions, total_duration_ms
                 FROM dictation_stats WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap()
    }

    #[test]
    fn retention_removes_only_expired_rows_and_updates_stats() {
        let (db, dir) = test_db();
        let now = Utc::now();
        insert(&db, "old", now - Duration::days(31), 3, 100);
        insert(&db, "boundary", now - Duration::days(30), 5, 200);
        insert(&db, "recent", now - Duration::days(2), 7, 300);

        assert_eq!(purge_transcripts_older_than(&db, 30, now).unwrap(), 1);
        assert_eq!(stats(&db), (12, 2, 500));
        let remaining: i64 = db
            .conn()
            .unwrap()
            .query_row("SELECT COUNT(*) FROM transcriptions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(remaining, 2);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn disabled_policy_clears_rows_and_zeros_stats() {
        let (db, dir) = test_db();
        insert(&db, "one", Utc::now(), 4, 150);

        assert_eq!(
            enforce_transcript_policy(&db, false, 0, Utc::now()).unwrap(),
            1
        );
        assert_eq!(stats(&db), (0, 0, 0));
        let secure_delete: i64 = db
            .conn()
            .unwrap()
            .query_row("PRAGMA secure_delete", [], |row| row.get(0))
            .unwrap();
        assert_eq!(secure_delete, 1);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn disabled_policy_truncates_wal_even_when_table_is_already_empty() {
        let (db, dir) = test_db();
        let wal_path = dir.join("omnivox.db-wal");
        {
            let conn = db.conn().unwrap();
            conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
            conn.execute(
                "INSERT INTO transcriptions
                 (id, text, duration_ms, model_name, created_at, word_count)
                 VALUES ('crash-window', 'sensitive crash residue', 1, 'test', ?1, 3)",
                [Utc::now().to_rfc3339()],
            )
            .unwrap();
            // Simulate a prior process reaching its DELETE commit and then
            // crashing before the WAL checkpoint. The logical table is empty,
            // but committed page frames still occupy the WAL file.
            conn.execute("DELETE FROM transcriptions", []).unwrap();
        }
        assert!(std::fs::metadata(&wal_path).unwrap().len() > 0);

        assert_eq!(purge_all_transcripts(&db).unwrap(), 0);
        assert_eq!(std::fs::metadata(&wal_path).unwrap().len(), 0);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn forever_policy_is_a_no_op_and_invalid_windows_are_rejected() {
        let (db, dir) = test_db();
        insert(&db, "one", Utc::now(), 1, 10);
        assert_eq!(
            enforce_transcript_policy(&db, true, 0, Utc::now()).unwrap(),
            0
        );
        assert!(purge_transcripts_older_than(
            &db,
            crate::storage::types::MAX_HISTORY_RETENTION_DAYS + 1,
            Utc::now()
        )
        .is_err());
        assert_eq!(stats(&db), (1, 1, 10));
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
