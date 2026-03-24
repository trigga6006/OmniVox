use tauri::{Emitter, Manager, State};
use crate::hotkey::HotkeyConfig;
use crate::output::types::OutputMode;
use crate::postprocess::types::WritingStyle;
use crate::state::AppState;
use crate::storage::types::AppSettings;

const TASKBAR_H: f64 = 48.0;
const MARGIN: f64 = 12.0;

/// Find which monitor currently contains the mouse cursor.
/// The cursor tracks the user's active text input, so the overlay pill
/// follows them across monitors automatically.
#[cfg(target_os = "windows")]
fn cursor_monitor(app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let mut pt = POINT { x: 0, y: 0 };
    if unsafe { GetCursorPos(&mut pt) } == 0 {
        return None;
    }

    let monitors = app.available_monitors().ok()?;
    monitors.into_iter().find(|mon| {
        let pos = mon.position();
        let size = mon.size();
        pt.x >= pos.x
            && pt.x < pos.x + size.width as i32
            && pt.y >= pos.y
            && pt.y < pos.y + size.height as i32
    })
}

#[cfg(not(target_os = "windows"))]
fn cursor_monitor(_app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    None
}

/// Resize and reposition the overlay pill window from the frontend.
/// Automatically moves the pill to whichever monitor has the cursor,
/// so it follows the user across multi-monitor setups.
#[tauri::command]
pub async fn resize_overlay(
    app: tauri::AppHandle,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found")?;

    // Prefer the monitor with the cursor; fall back to the overlay's current monitor.
    let target = cursor_monitor(&app)
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or("no monitor")?;

    let scale = target.scale_factor();
    let mon_pos = target.position();
    let mon_size = target.size();

    // Calculate position in physical pixels, centered at the bottom of the target monitor
    let phys_w = width * scale;
    let phys_h = height * scale;
    let taskbar_phys = TASKBAR_H * scale;
    let margin_phys = MARGIN * scale;

    let x = mon_pos.x as f64 + (mon_size.width as f64 - phys_w) / 2.0;
    let y = mon_pos.y as f64 + mon_size.height as f64 - taskbar_phys - phys_h - margin_phys;

    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(x as i32, y.max(0.0) as i32))
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
    app: tauri::AppHandle,
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
        cfg.ship_mode = settings.ship_mode;
    }

    // Sync writing style to the processor chain
    if let Ok(mut proc) = state.processor.lock() {
        proc.set_style(WritingStyle::from_str(&settings.writing_style));
    }

    // Sync hotkey to the live hook
    if let Some(ref hk) = settings.hotkey {
        let key1 = hk.keys.first().copied().unwrap_or(0);
        let key2 = hk.keys.get(1).copied().unwrap_or(0);
        crate::hotkey::update_hotkey_keys(key1, key2);
    }

    // Broadcast to all windows so the overlay and main window stay in sync
    let _ = app.emit("settings-changed", &settings);

    Ok(())
}

/// Suspend or resume the hotkey hook.
/// Called by the frontend before entering "listening" mode for key recording.
#[tauri::command]
pub async fn suspend_hotkey(suspended: bool) -> Result<(), String> {
    crate::hotkey::set_suspended(suspended);
    Ok(())
}

/// Show and focus the main application window (used by the overlay pill).
#[tauri::command]
pub async fn show_main_window(app: tauri::AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or("main window not found")?;
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())?;
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
