use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::TranscriptionRecord;
use chrono::{DateTime, Utc};
use rusqlite::{params, types::Type, OptionalExtension};
use uuid::Uuid;

/// Map a rusqlite row to a TranscriptionRecord.
///
/// SELECT column order: id, text, duration_ms, model_name, created_at, raw_transcript
/// (the raw_transcript column was added in a later migration and may be NULL
/// for pre-migration rows).
fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<TranscriptionRecord> {
    let id_str: String = row.get(0)?;
    let text: String = row.get(1)?;
    let duration_ms: u64 = row.get(2)?;
    let model_name: String = row.get(3)?;
    let created_at_str: String = row.get(4)?;
    let raw_transcript: Option<String> = row.get(5)?;

    let id = Uuid::parse_str(&id_str)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(0, Type::Text, Box::new(e)))?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(e)))?;

    Ok(TranscriptionRecord {
        id,
        text,
        duration_ms,
        model_name,
        created_at,
        raw_transcript,
    })
}

/// Save (insert or replace) a transcription record.
pub fn save_transcription(db: &Database, record: &TranscriptionRecord) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let id = record.id.to_string();
    let old: Option<(i64, i64)> = tx
        .query_row(
            "SELECT word_count, duration_ms FROM transcriptions WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let word_count = record.text.split_whitespace().count() as i64;
    let counted_duration = if word_count > 0 {
        record.duration_ms as i64
    } else {
        0
    };

    tx.execute(
        "INSERT INTO transcriptions
            (id, text, duration_ms, model_name, created_at, raw_transcript, word_count)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(id) DO UPDATE SET
            text = excluded.text,
            duration_ms = excluded.duration_ms,
            model_name = excluded.model_name,
            created_at = excluded.created_at,
            raw_transcript = excluded.raw_transcript,
            word_count = excluded.word_count",
        params![
            id,
            record.text,
            record.duration_ms,
            record.model_name,
            record.created_at.to_rfc3339(),
            record.raw_transcript,
            word_count,
        ],
    )?;
    let (old_words, old_duration, transcription_delta) = match old {
        Some((words, duration)) => (words, if words > 0 { duration } else { 0 }, 0),
        None => (0, 0, if word_count > 0 { 1 } else { 0 }),
    };
    // Replacing an empty row with a real transcription (or vice versa) changes
    // the counted-row total even though the primary key already existed.
    let old_counted = i64::from(old_words > 0);
    let new_counted = i64::from(word_count > 0);
    let transcription_delta = if old.is_some() {
        new_counted - old_counted
    } else {
        transcription_delta
    };
    tx.execute(
        "UPDATE dictation_stats SET
            total_words = MAX(0, total_words + ?1),
            total_transcriptions = MAX(0, total_transcriptions + ?2),
            total_duration_ms = MAX(0, total_duration_ms + ?3)
         WHERE singleton = 1",
        params![
            word_count - old_words,
            transcription_delta,
            counted_duration - old_duration,
        ],
    )?;
    tx.commit()?;
    Ok(())
}

/// Get aggregate dictation statistics (total words, transcriptions, duration).
///
/// Exact counts are maintained transactionally when rows are inserted,
/// replaced, or deleted, so this completion-time query is O(1) regardless of
/// history size.
pub fn get_dictation_stats(db: &Database) -> AppResult<crate::storage::types::DictationStats> {
    let conn = db.conn()?;
    let stats = conn.query_row(
        "SELECT total_words, total_transcriptions, total_duration_ms
         FROM dictation_stats WHERE singleton = 1",
        [],
        |row| {
            Ok(crate::storage::types::DictationStats {
                total_words: row.get::<_, i64>(0)? as u64,
                total_transcriptions: row.get::<_, i64>(1)? as u64,
                total_duration_ms: row.get::<_, i64>(2)? as u64,
            })
        },
    )?;
    Ok(stats)
}

/// Get lean per-transcription rows for the analytics page.
///
/// Returns every non-empty transcription in chronological order. Word counts
/// are persisted with the row and SQLite computes character length, so the
/// full text never leaves the storage layer for analytics.
pub fn get_analytics_records(
    db: &Database,
) -> AppResult<Vec<crate::storage::types::AnalyticsRecord>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT word_count, LENGTH(text), duration_ms, model_name, created_at
         FROM transcriptions
         WHERE word_count > 0
         ORDER BY created_at ASC",
    )?;
    let records = stmt
        .query_map([], |row| {
            let word_count: u64 = row.get(0)?;
            let char_count: u64 = row.get(1)?;
            let duration_ms: u64 = row.get(2)?;
            let model_name: String = row.get(3)?;
            let created_at_str: String = row.get(4)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(4, Type::Text, Box::new(e))
                })?;
            Ok(crate::storage::types::AnalyticsRecord {
                created_at,
                word_count,
                char_count,
                duration_ms,
                model_name,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

/// Search transcription history by query string (case-insensitive substring match).
pub fn search_history(
    db: &Database,
    query: &str,
    limit: u32,
    offset: u32,
) -> AppResult<Vec<TranscriptionRecord>> {
    let conn = db.conn()?;
    let like_pattern = format!("%{}%", query);
    let mut stmt = conn.prepare(
        "SELECT id, text, duration_ms, model_name, created_at, raw_transcript
         FROM transcriptions
         WHERE text LIKE ?1
         ORDER BY created_at DESC
         LIMIT ?2 OFFSET ?3",
    )?;
    let records = stmt
        .query_map(params![like_pattern, limit, offset], row_to_record)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

/// Get the most recent transcription records with pagination.
pub fn recent_history(
    db: &Database,
    limit: u32,
    offset: u32,
) -> AppResult<Vec<TranscriptionRecord>> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, text, duration_ms, model_name, created_at, raw_transcript
         FROM transcriptions
         ORDER BY created_at DESC
         LIMIT ?1 OFFSET ?2",
    )?;
    let records = stmt
        .query_map(params![limit, offset], row_to_record)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(records)
}

/// Delete a single transcription record by ID.
pub fn delete_record(db: &Database, id: &str) -> AppResult<()> {
    let mut conn = db.conn()?;
    let tx = conn.transaction()?;
    let old: Option<(i64, i64)> = tx
        .query_row(
            "SELECT word_count, duration_ms FROM transcriptions WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    tx.execute("DELETE FROM transcriptions WHERE id = ?1", params![id])?;
    if let Some((words, duration)) = old {
        let counted = words > 0;
        tx.execute(
            "UPDATE dictation_stats SET
                total_words = MAX(0, total_words - ?1),
                total_transcriptions = MAX(0, total_transcriptions - ?2),
                total_duration_ms = MAX(0, total_duration_ms - ?3)
             WHERE singleton = 1",
            params![
                words,
                i64::from(counted),
                if counted { duration } else { 0 }
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// Export transcription history in the given format ("json" or "csv").
pub fn export_history(db: &Database, format: &str) -> AppResult<String> {
    // Keep database ownership scoped to the query. Pretty-JSON and CSV
    // formatting can be expensive for a large history and must not hold the
    // application's one connection mutex while doing CPU-only work.
    let records: Vec<TranscriptionRecord> = {
        let conn = db.conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, text, duration_ms, model_name, created_at, raw_transcript
             FROM transcriptions
             ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], row_to_record)?
            .collect::<Result<Vec<_>, _>>()?;
        rows
    };

    format_export(&records, format)
}

fn format_export(records: &[TranscriptionRecord], format: &str) -> AppResult<String> {
    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&records)
                .map_err(|e| crate::error::AppError::Storage(e.to_string()))?;
            Ok(json)
        }
        "csv" => {
            let mut csv = String::from("id,text,duration_ms,model_name,created_at\n");
            for record in records {
                csv.push_str(&format!(
                    "{},{},{},{},{}\n",
                    record.id,
                    csv_field(&record.text),
                    record.duration_ms,
                    csv_field(&record.model_name),
                    csv_field(&record.created_at.to_rfc3339()),
                ));
            }
            Ok(csv)
        }
        _ => Err(crate::error::AppError::Storage(format!(
            "Unsupported export format: {}",
            format
        ))),
    }
}

fn csv_field(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_database(label: &str) -> (std::path::PathBuf, Database) {
        let dir =
            std::env::temp_dir().join(format!("omnivox-history-{label}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Database::init(&dir.join("history.db")).unwrap();
        (dir, db)
    }

    fn record(id: Uuid, text: &str, duration_ms: u64) -> TranscriptionRecord {
        TranscriptionRecord {
            id,
            text: text.into(),
            duration_ms,
            model_name: "test-model".into(),
            created_at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                .unwrap()
                .with_timezone(&Utc),
            raw_transcript: None,
        }
    }

    #[test]
    fn stats_are_exact_and_incremental_for_insert_replace_and_delete() {
        let (dir, db) = test_database("stats");
        let first_id = Uuid::new_v4();
        let second_id = Uuid::new_v4();

        save_transcription(&db, &record(first_id, "one  two\nthree\tfour", 100)).unwrap();
        save_transcription(&db, &record(second_id, "  \n\t ", 999)).unwrap();
        let stats = get_dictation_stats(&db).unwrap();
        assert_eq!(stats.total_words, 4);
        assert_eq!(stats.total_transcriptions, 1);
        assert_eq!(stats.total_duration_ms, 100);

        save_transcription(&db, &record(first_id, "only two", 250)).unwrap();
        save_transcription(&db, &record(second_id, "now counted", 40)).unwrap();
        let stats = get_dictation_stats(&db).unwrap();
        assert_eq!(stats.total_words, 4);
        assert_eq!(stats.total_transcriptions, 2);
        assert_eq!(stats.total_duration_ms, 290);

        save_transcription(&db, &record(first_id, "", 500)).unwrap();
        let stats = get_dictation_stats(&db).unwrap();
        assert_eq!(stats.total_words, 2);
        assert_eq!(stats.total_transcriptions, 1);
        assert_eq!(stats.total_duration_ms, 40);

        delete_record(&db, &second_id.to_string()).unwrap();
        delete_record(&db, &Uuid::new_v4().to_string()).unwrap();
        let stats = get_dictation_stats(&db).unwrap();
        assert_eq!(stats.total_words, 0);
        assert_eq!(stats.total_transcriptions, 0);
        assert_eq!(stats.total_duration_ms, 0);

        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn analytics_uses_persisted_word_count_without_loading_full_text() {
        let (dir, db) = test_database("analytics");
        let text = "é  hi\nthere";
        save_transcription(&db, &record(Uuid::new_v4(), text, 77)).unwrap();

        let rows = get_analytics_records(&db).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].word_count, 3);
        assert_eq!(rows[0].char_count, text.chars().count() as u64);
        assert_eq!(rows[0].duration_ms, 77);

        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn malformed_persisted_identity_is_reported_instead_of_fabricated() {
        let (dir, db) = test_database("malformed");
        {
            let conn = db.conn().unwrap();
            conn.execute(
                "INSERT INTO transcriptions
                    (id, text, duration_ms, model_name, created_at,
                     raw_transcript, word_count)
                 VALUES ('not-a-uuid', 'hello', 1, 'test',
                         '2026-01-01T00:00:00Z', NULL, 1)",
                [],
            )
            .unwrap();
        }
        assert!(recent_history(&db, 10, 0).is_err());

        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn csv_export_quotes_all_string_fields() {
        let mut sample = record(Uuid::nil(), "hello, \"world\"", 12);
        sample.model_name = "model,one".into();
        let csv = format_export(&[sample], "csv").unwrap();
        assert!(csv.contains("\"hello, \"\"world\"\"\""));
        assert!(csv.contains("\"model,one\""));
        assert!(format_export(&[], "xml").is_err());
    }
}
