use tauri::{Emitter, State};

use crate::cleanup::registry;
use crate::cleanup::service;
use crate::cleanup::types::*;
use crate::state::AppState;

/// List all supported cleanup models with their installation status.
#[tauri::command]
pub async fn list_cleanup_models(_state: State<'_, AppState>) -> Result<Vec<CleanupModelInfo>, String> {
    let mut models = registry::supported_models();

    // Check installation status for each model
    for model in &mut models {
        model.is_installed = service::check_model_availability(&model.id)
            .await
            .unwrap_or(false);
    }

    Ok(models)
}

/// Check if the local model server (Ollama) is running.
#[tauri::command]
pub async fn check_cleanup_server() -> Result<bool, String> {
    Ok(service::is_server_running().await)
}

/// Run cleanup on the given text using current settings.
/// Returns the cleanup result with both raw and cleaned text.
#[tauri::command]
pub async fn run_cleanup(
    app: tauri::AppHandle,
    text: String,
    state: State<'_, AppState>,
) -> Result<CleanupResult, String> {
    let settings = crate::storage::settings::get_settings(&state.db)
        .map_err(|e| e.to_string())?;

    if !settings.cleanup_enabled {
        return Err("Cleanup is not enabled".to_string());
    }

    let model_id = CleanupModelId(settings.cleanup_model_id.clone());
    let mode = CleanupMode::from_str(&settings.cleanup_mode);
    let strength = RewriteStrength::from_str(&settings.cleanup_strength);

    let request = CleanupRequest {
        raw_text: text.clone(),
        mode,
        strength,
        model_id,
    };

    // Emit status
    let _ = app.emit("cleanup-status", "running");

    match service::run_cleanup(&request).await {
        Ok(result) => {
            let _ = app.emit("cleanup-status", "success");
            let _ = app.emit("cleanup-result", &result);
            Ok(result)
        }
        Err(e) => {
            let _ = app.emit("cleanup-status", "failed");
            let _ = app.emit("cleanup-error", e.to_string());
            Err(e.to_string())
        }
    }
}

/// Run cleanup with explicit mode/strength parameters (for preview/testing).
#[tauri::command]
pub async fn run_cleanup_with_options(
    text: String,
    model_id: String,
    mode: String,
    strength: String,
) -> Result<CleanupResult, String> {
    let request = CleanupRequest {
        raw_text: text,
        mode: CleanupMode::from_str(&mode),
        strength: RewriteStrength::from_str(&strength),
        model_id: CleanupModelId(model_id),
    };

    service::run_cleanup(&request)
        .await
        .map_err(|e| e.to_string())
}

/// Get the current cleanup settings.
#[tauri::command]
pub async fn get_cleanup_settings(state: State<'_, AppState>) -> Result<CleanupSettings, String> {
    let settings = crate::storage::settings::get_settings(&state.db)
        .map_err(|e| e.to_string())?;

    Ok(CleanupSettings {
        enabled: settings.cleanup_enabled,
        model_id: settings.cleanup_model_id,
        mode: CleanupMode::from_str(&settings.cleanup_mode),
        strength: RewriteStrength::from_str(&settings.cleanup_strength),
        use_cleaned_by_default: settings.cleanup_use_cleaned_by_default,
    })
}
