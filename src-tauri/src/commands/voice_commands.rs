use super::auth::{require_caller, WindowPolicy};
use crate::state::AppState;
use crate::storage::types::CustomVoiceCommand;
use tauri::State;

#[tauri::command]
pub async fn list_voice_commands(
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<CustomVoiceCommand>, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::voice_commands::list(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_voice_command(
    caller: tauri::WebviewWindow,
    phrase: String,
    action: String,
    trigger_scope: String,
    state: State<'_, AppState>,
) -> Result<CustomVoiceCommand, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::voice_commands::add(&state.db, &phrase, &action, &trigger_scope)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_voice_command(
    caller: tauri::WebviewWindow,
    id: String,
    phrase: String,
    action: String,
    trigger_scope: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::voice_commands::update(
        &state.db,
        &id,
        &phrase,
        &action,
        &trigger_scope,
        enabled,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_voice_command(
    caller: tauri::WebviewWindow,
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::voice_commands::delete(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn reset_voice_commands(
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Vec<CustomVoiceCommand>, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::storage::voice_commands::reset_to_defaults(&state.db).map_err(|e| e.to_string())?;
    crate::storage::voice_commands::list(&state.db).map_err(|e| e.to_string())
}
