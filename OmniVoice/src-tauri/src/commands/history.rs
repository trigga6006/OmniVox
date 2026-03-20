use crate::storage::types::TranscriptionRecord;

#[tauri::command]
pub async fn search_history(query: String) -> Result<Vec<TranscriptionRecord>, String> {
    crate::storage::history::search_history(&query).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn recent_history(limit: Option<u32>) -> Result<Vec<TranscriptionRecord>, String> {
    let limit = limit.unwrap_or(50);
    crate::storage::history::recent_history(limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_record(id: String) -> Result<(), String> {
    crate::storage::history::delete_record(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_history(format: String) -> Result<String, String> {
    crate::storage::history::export_history(&format).map_err(|e| e.to_string())
}
