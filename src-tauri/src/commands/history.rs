use crate::state::AppState;
use crate::storage::types::TranscriptionRecord;
use tauri::State;

fn require_main_window(caller: &tauri::WebviewWindow) -> Result<(), String> {
    require_main_label(caller.label())
}

fn require_main_label(label: &str) -> Result<(), String> {
    if label == "main" {
        Ok(())
    } else {
        Err("History is only available from the main window".into())
    }
}

#[tauri::command]
pub async fn get_dictation_stats(
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<crate::storage::types::DictationStats, String> {
    require_main_window(&caller)?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::storage::history::get_dictation_stats(&db))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_analytics_records(
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<crate::storage::types::AnalyticsRecord>, String> {
    require_main_window(&caller)?;
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
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptionRecord>, String> {
    require_main_window(&caller)?;
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
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<TranscriptionRecord>, String> {
    require_main_window(&caller)?;
    let limit = limit.unwrap_or(50);
    let offset = offset.unwrap_or(0);
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::storage::history::recent_history(&db, limit, offset))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_history_record(
    id: String,
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_main_window(&caller)?;
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::storage::history::delete_record(&db, &id))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_history(
    format: String,
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<String, String> {
    require_main_window(&caller)?;
    // Serializes the entire history — blocking thread, same reasoning as
    // get_analytics_records.
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || crate::storage::history::export_history(&db, &format))
        .await
        .map_err(|e| format!("task join: {e}"))?
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::require_main_label;

    #[test]
    fn history_ipc_is_main_window_only() {
        assert!(require_main_label("main").is_ok());
        assert!(require_main_label("overlay").is_err());
        assert!(require_main_label("scratchpad").is_err());
    }
}
