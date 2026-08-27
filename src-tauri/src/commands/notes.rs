use super::auth::{require_caller, WindowPolicy};
use crate::state::AppState;
use crate::storage::types::Note;
use tauri::State;

#[tauri::command]
pub async fn add_note(
    caller: tauri::WebviewWindow,
    title: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<Note, String> {
    require_caller(&caller, WindowPolicy::MainScratchpad)?;
    crate::storage::notes::add_note(&state.db, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_note(
    caller: tauri::WebviewWindow,
    id: String,
    title: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::notes::update_note(&state.db, &id, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_note(
    caller: tauri::WebviewWindow,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::notes::delete_note(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_notes(
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<Note>, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::notes::list_notes(&state.db).map_err(|e| e.to_string())
}
