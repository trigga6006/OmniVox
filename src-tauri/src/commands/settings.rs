use tauri::{Manager, State};
use crate::hotkey::HotkeyConfig;
use crate::output::types::OutputMode;
use crate::state::AppState;
use crate::storage::types::AppSettings;

const TASKBAR_H: f64 = 48.0;
const MARGIN: f64 = 12.0;

/// Resize and reposition the overlay pill window from the frontend.
#[tauri::command]
pub async fn resize_overlay(
    app: tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found")?;

    let monitor = window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no monitor")?;

    let scale = monitor.scale_factor();
    let screen_w = monitor.size().width as f64 / scale;
    let screen_h = monitor.size().height as f64 / scale;

    let x = (screen_w - width) / 2.0;
    let y = (screen_h - TASKBAR_H - height - MARGIN).max(0.0);

    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::LogicalPosition::new(x, y))
        .map_err(|e| e.to_string())?;

    Ok(())
}

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
    // Persist to SQLite
    crate::storage::settings::update_settings(&state.db, &settings).map_err(|e| e.to_string())?;

    // Sync output mode to in-memory state so the pipeline uses it immediately
    let mode = match settings.output_mode.as_str() {
        "type_simulation" => OutputMode::TypeSimulation,
        "both" => OutputMode::Both,
        _ => OutputMode::Clipboard,
    };
    if let Ok(mut cfg) = state.output_config.lock() {
        cfg.mode = mode;
    }

    // Sync hotkey to the live hook
    if let Some(ref hk) = settings.hotkey {
        let key1 = hk.keys.first().copied().unwrap_or(0);
        let key2 = hk.keys.get(1).copied().unwrap_or(0);
        crate::hotkey::update_hotkey_keys(key1, key2);
    }

    Ok(())
}

/// Suspend or resume the hotkey hook.
/// Called by the frontend before entering "listening" mode for key recording.
#[tauri::command]
pub async fn suspend_hotkey(suspended: bool) -> Result<(), String> {
    crate::hotkey::set_suspended(suspended);
    Ok(())
}

/// Persist a new hotkey config and activate it immediately.
#[tauri::command]
pub async fn update_hotkey(
    config: HotkeyConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if config.keys.is_empty() || config.keys.len() > 2 {
        return Err("Hotkey must be 1 or 2 keys".into());
    }

    // Persist to SQLite
    let mut settings = crate::storage::settings::get_settings(&state.db)
        .map_err(|e| e.to_string())?;
    settings.hotkey = Some(config.clone());
    crate::storage::settings::update_settings(&state.db, &settings)
        .map_err(|e| e.to_string())?;

    // Live-update the hook
    let key1 = config.keys[0];
    let key2 = config.keys.get(1).copied().unwrap_or(0);
    crate::hotkey::update_hotkey_keys(key1, key2);

    // Un-suspend in case we were in listening mode
    crate::hotkey::set_suspended(false);

    Ok(())
}
