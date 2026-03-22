use tauri::State;
use crate::state::AppState;
use crate::storage::types::Note;

#[tauri::command]
pub async fn add_note(
    title: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<Note, String> {
    crate::storage::notes::add_note(&state.db, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_note(
    id: String,
    title: String,
    content: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::notes::update_note(&state.db, &id, &title, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_note(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::notes::delete_note(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_notes(
    state: State<'_, AppState>,
) -> Result<Vec<Note>, String> {
    crate::storage::notes::list_notes(&state.db).map_err(|e| e.to_string())
}
