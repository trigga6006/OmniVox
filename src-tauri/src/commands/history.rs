use crate::state::AppState;
use crate::storage::types::TranscriptionRecord;
use tauri::State;

#[tauri::command]
pub async fn get_dictation_stats(
    state: State<'_, AppState>,
) -> Result<crate::storage::types::DictationStats, String> {
    crate::storage::history::get_dictation_stats(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_analytics_records(
    state: State<'_, AppState>,
) -> Result<Vec<crate::storage::types::AnalyticsRecord>, String> {
    // Full-table read — run on a blocking thread so a large history can't
    // stall the async runtime (which also services the dictation pipeline).
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::storage::history::get_analytics_records(&db))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn search_history(
    query: String,
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptionRecord>, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    // LIKE scan over the whole table — keep it off the async runtime.
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        crate::storage::history::search_history(&db, &query, limit, offset)
    })
    .await
    .map_err(|e| format!("task join: {e}"))?
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn recent_history(
    limit: Option<u32>,
    offset: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptionRecord>, String> {
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    crate::storage::history::recent_history(&state.db, limit, offset).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_record(id: String, state: State<'_, AppState>) -> Result<(), String> {
    crate::storage::history::delete_record(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_history(format: String, state: State<'_, AppState>) -> Result<String, String> {
    // Serializes the entire history — blocking thread, same reasoning as
    // get_analytics_records.
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::storage::history::export_history(&db, &format))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())
}
