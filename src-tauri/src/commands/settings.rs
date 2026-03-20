use tauri::State;
use crate::state::AppState;
use crate::storage::types::AppSettings;

#[tauri::command]
pub async fn get_settings(
    state: State<'_, AppState>,
) -> Result<AppSettings, String> {
    crate::storage::settings::get_settings(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_settings(
    settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::settings::update_settings(&state.db, &settings).map_err(|e| e.to_string())
}
