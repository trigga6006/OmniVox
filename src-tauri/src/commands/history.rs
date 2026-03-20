use tauri::State;
use crate::state::AppState;
use crate::storage::types::TranscriptionRecord;

#[tauri::command]
pub async fn search_history(
    query: String,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptionRecord>, String> {
    crate::storage::history::search_history(&state.db, &query).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn recent_history(
    limit: Option<u32>,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptionRecord>, String> {
    let limit = limit.unwrap_or(50);
    crate::storage::history::recent_history(&state.db, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_record(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::history::delete_record(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_history(
    format: String,
    state: State<'_, AppState>,
) -> Result<String, String> {
    crate::storage::history::export_history(&state.db, &format).map_err(|e| e.to_string())
}
