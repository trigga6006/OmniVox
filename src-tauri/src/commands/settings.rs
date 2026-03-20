use crate::storage::types::AppSettings;

#[tauri::command]
pub async fn get_settings() -> Result<AppSettings, String> {
    crate::storage::settings::get_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_settings(settings: AppSettings) -> Result<(), String> {
    crate::storage::settings::update_settings(settings).map_err(|e| e.to_string())
}
