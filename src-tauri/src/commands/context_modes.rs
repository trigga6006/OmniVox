use tauri::{Emitter, State};

use crate::state::AppState;
use crate::storage::types::ContextMode;

#[tauri::command]
pub async fn list_context_modes(state: State<'_, AppState>) -> Result<Vec<ContextMode>, String> {
    crate::storage::context_modes::list_modes(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_context_mode(
    id: String,
    state: State<'_, AppState>,
) -> Result<ContextMode, String> {
    crate::storage::context_modes::get_mode(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_context_mode(
    name: String,
    description: String,
    icon: String,
    color: String,
    llm_prompt: String,
    state: State<'_, AppState>,
) -> Result<ContextMode, String> {
    crate::storage::context_modes::create_mode(&state.db, &name, &description, &icon, &color, &llm_prompt)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_context_mode(
    id: String,
    name: String,
    description: String,
    icon: String,
    color: String,
    llm_prompt: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::storage::context_modes::update_mode(&state.db, &id, &name, &description, &icon, &color, &llm_prompt)
        .map_err(|e| e.to_string())?;

    // If this is the active mode, update the in-memory prompt
    let active_id = state.active_context_mode_id.lock().unwrap().clone();
    if active_id.as_deref() == Some(&id) {
        *state.active_llm_prompt.lock().unwrap() = Some(llm_prompt);
    }

    Ok(())
}

#[tauri::command]
pub async fn delete_context_mode(
    id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // If deleting the active mode, switch back to General
    let active_id = state.active_context_mode_id.lock().unwrap().clone();
    if active_id.as_deref() == Some(&id) {
        // Find the builtin General mode
        let modes = crate::storage::context_modes::list_modes(&state.db)
            .map_err(|e| e.to_string())?;
        if let Some(general) = modes.iter().find(|m| m.is_builtin) {
            activate_mode_internal(&state, &general.id.to_string())?;
        }
    }

    crate::storage::context_modes::delete_mode(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_context_mode(
    state: State<'_, AppState>,
) -> Result<Option<ContextMode>, String> {
    let active_id = state.active_context_mode_id.lock().unwrap().clone();
    match active_id {
        Some(id) => {
            let mode = crate::storage::context_modes::get_mode(&state.db, &id)
                .map_err(|e| e.to_string())?;
            Ok(Some(mode))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn set_active_context_mode(
    id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    activate_mode_internal(&state, &id)?;

    // Emit event so the overlay can update
    let mode = crate::storage::context_modes::get_mode(&state.db, &id)
        .map_err(|e| e.to_string())?;
    let _ = app_handle.emit("context-mode-changed", serde_json::json!({
        "id": mode.id.to_string(),
        "name": mode.name,
        "icon": mode.icon,
        "color": mode.color,
    }));

    Ok(())
}

/// Internal helper to switch the active context mode:
/// 1. Update settings
/// 2. Load global + mode's dictionary/snippets into ProcessorChain
/// 3. Update the in-memory LLM prompt
fn activate_mode_internal(state: &AppState, mode_id: &str) -> Result<(), String> {
    let mode = crate::storage::context_modes::get_mode(&state.db, mode_id)
        .map_err(|e| e.to_string())?;

    // Persist active mode choice
    crate::storage::settings::set_setting(&state.db, "active_context_mode_id", mode_id)
        .map_err(|e| e.to_string())?;

    // Update in-memory state
    *state.active_context_mode_id.lock().unwrap() = Some(mode_id.to_string());
    *state.active_llm_prompt.lock().unwrap() = Some(mode.llm_prompt);

    // Load global + mode-scoped entries into the processor
    super::dictionary::sync_processor(state);

    Ok(())
}
