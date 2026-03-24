use crate::error::AppResult;
use crate::storage::database::Database;
use crate::storage::types::TranscriptionRecord;
use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

/// Map a rusqlite row to a TranscriptionRecord.
fn row_to_record(row: &rusqlite::Row) -> rusqlite::Result<TranscriptionRecord> {
    let id_str: String = row.get(0)?;
    let text: String = row.get(1)?;
    let duration_ms: u64 = row.get(2)?;
    let model_name: String = row.get(3)?;
    let created_at_str: String = row.get(4)?;

    let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    Ok(TranscriptionRecord {
        id,
        text,
        duration_ms,
        model_name,
        created_at,
    })
}

/// Save (insert or replace) a transcription record.
pub fn save_transcription(db: &Database, record: &TranscriptionRecord) -> AppResult<()> {
    let conn = db.conn()?;
    conn.execute(
        "INSERT OR REPLACE INTO transcriptions (id, text, duration_ms, model_name, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            record.id.to_string(),
            record.text,
            record.duration_ms,
            record.model_name,
            record.created_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

/// Get aggregate dictation statistics (total words, transcriptions, duration).
pub fn get_dictation_stats(db: &Database) -> AppResult<crate::storage::types::DictationStats> {
    let conn = db.conn()?;
    let stats = conn.query_row(
        "SELECT
            COALESCE(SUM(LENGTH(TRIM(text)) - LENGTH(REPLACE(TRIM(text), ' ', '')) + 1), 0),
            COUNT(*),
            COALESCE(SUM(duration_ms), 0)
         FROM transcriptions
         WHERE TRIM(text) != ''",
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
        "SELECT id, text, duration_ms, model_name, created_at
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
        "SELECT id, text, duration_ms, model_name, created_at
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
    let conn = db.conn()?;
    conn.execute(
        "DELETE FROM transcriptions WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

/// Export transcription history in the given format ("json" or "csv").
pub fn export_history(db: &Database, format: &str) -> AppResult<String> {
    let conn = db.conn()?;
    let mut stmt = conn.prepare(
        "SELECT id, text, duration_ms, model_name, created_at
         FROM transcriptions
         ORDER BY created_at DESC",
    )?;
    let records: Vec<TranscriptionRecord> = stmt
        .query_map([], row_to_record)?
        .collect::<Result<Vec<_>, _>>()?;

    match format {
        "json" => {
            let json = serde_json::to_string_pretty(&records)
                .map_err(|e| crate::error::AppError::Storage(e.to_string()))?;
            Ok(json)
        }
        "csv" => {
            let mut csv = String::from("id,text,duration_ms,model_name,created_at\n");
            for record in &records {
                // Escape double quotes in text by doubling them, and wrap in quotes
                let escaped_text = record.text.replace('"', "\"\"");
                csv.push_str(&format!(
                    "{},\"{}\",{},{},{}\n",
                    record.id,
                    escaped_text,
                    record.duration_ms,
                    record.model_name,
                    record.created_at.to_rfc3339(),
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
