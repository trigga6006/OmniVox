//! Scratchpad window lifecycle + CRUD commands.

use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::state::AppState;
use crate::storage::scratchpad::{self, ScratchpadData, ScratchpadEntry};

const SCRATCHPAD_W: f64 = 340.0;
const SCRATCHPAD_H: f64 = 440.0;
const SCRATCHPAD_MARGIN: f64 = 16.0;

/// Bottom-right of the primary monitor's work area (above the taskbar), for a
/// window of the given logical size.
fn scratchpad_position(app: &AppHandle, w: f64, h: f64) -> (f64, f64) {
    if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let wa = monitor.work_area();
        let wa_x = wa.position.x as f64 / scale;
        let wa_y = wa.position.y as f64 / scale;
        let wa_w = wa.size.width as f64 / scale;
        let wa_h = wa.size.height as f64 / scale;
        (
            wa_x + wa_w - w - SCRATCHPAD_MARGIN,
            wa_y + wa_h - h - SCRATCHPAD_MARGIN,
        )
    } else {
        (600.0, 300.0)
    }
}

/// Show the scratchpad window, creating it on first use.
///
/// MUST stay on the async runtime: building a `WebviewWindow` inside a
/// synchronous command deadlocks WebView2 on Windows. On reopen we show/focus
/// the existing window (it is hidden, not destroyed, on close) so content +
/// listeners survive.  Shared by the Tauri command and voice Command Mode.
pub async fn open_scratchpad_impl(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(win) = app.get_webview_window("scratchpad") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
        return Ok(());
    }

    // Restore the saved size (physical px → logical) so the pad reopens at the
    // size you closed it, falling back to the default on first use.
    let (w, h) = match scratchpad::get_size(&state.db) {
        Ok(Some((pw, ph))) => {
            let scale = app
                .primary_monitor()
                .ok()
                .flatten()
                .map(|m| m.scale_factor())
                .unwrap_or(1.0);
            (pw / scale, ph / scale)
        }
        _ => (SCRATCHPAD_W, SCRATCHPAD_H),
    };
    let (x, y) = scratchpad_position(&app, w, h);

    let win = WebviewWindowBuilder::new(app, "scratchpad", WebviewUrl::App("/scratchpad.html".into()))
        .title("Scratchpad")
        .inner_size(w, h)
        .min_inner_size(260.0, 300.0)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .focused(true)
        .visible(true)
        .build()
        .map_err(|e| e.to_string())?;

    // Restore the saved position (physical px) so it reopens where you left it,
    // not back in the default corner.
    if let Ok(Some((px, py))) = crate::storage::scratchpad::get_position(&state.db) {
        let _ = win.set_position(tauri::PhysicalPosition::new(px, py));
    }

    // Hide (don't destroy) on close so content, scroll position, unsaved edits,
    // and listeners survive for an instant reopen. Save geometry first.
    let app_for_close = (*app).clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if let Some(w) = app_for_close.get_webview_window("scratchpad") {
                save_scratchpad_geometry(&app_for_close, &w);
                let _ = w.hide();
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub async fn open_scratchpad(app: AppHandle) -> Result<(), String> {
    open_scratchpad_impl(&app).await
}

/// Hide the scratchpad window (saving geometry first).  Returns whether a
/// visible window was actually closed — voice feedback distinguishes "closed"
/// from "wasn't open".  Shared by the Tauri command and voice Command Mode.
pub fn close_scratchpad_impl(app: &AppHandle) -> bool {
    if let Some(win) = app.get_webview_window("scratchpad") {
        if win.is_visible().unwrap_or(false) {
            save_scratchpad_geometry(app, &win);
            let _ = win.hide();
            return true;
        }
    }
    false
}

/// Wipe the scratchpad's saved cards + note (voice "clear scratchpad" —
/// confirm-gated in the pipeline).  Emits `scratchpad-refresh` so an open pad
/// reloads its now-empty state.
pub fn clear_scratchpad_impl(app: &AppHandle) -> Result<(), String> {
    let st = app.state::<AppState>();
    scratchpad::clear_pad(&st.db, None).map_err(|e| e.to_string())?;
    scratchpad::set_note(&st.db, "").map_err(|e| e.to_string())?;
    let _ = app.emit("scratchpad-refresh", ());
    Ok(())
}

fn save_scratchpad_geometry(app: &AppHandle, win: &tauri::WebviewWindow) {
    // A minimized window reports the (-32000,-32000) position sentinel and a
    // junk size — persisting either would reopen the pad off-screen/broken.
    // (Reachable via voice: "minimize everything" then "close the scratchpad".)
    if win.is_minimized().unwrap_or(false) {
        return;
    }
    let st = app.state::<AppState>();
    if let Ok(pos) = win.outer_position() {
        let _ = crate::storage::scratchpad::set_position(&st.db, pos.x as f64, pos.y as f64);
    }
    if let Ok(size) = win.inner_size() {
        let _ = crate::storage::scratchpad::set_size(&st.db, size.width as f64, size.height as f64);
    }
}

#[tauri::command]
pub async fn close_scratchpad(app: AppHandle) -> Result<(), String> {
    close_scratchpad_impl(&app);
    Ok(())
}

#[tauri::command]
pub async fn save_scratchpad_position(x: f64, y: f64, state: State<'_, AppState>) -> Result<(), String> {
    crate::storage::scratchpad::set_position(&state.db, x, y).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_scratchpad_size(w: f64, h: f64, state: State<'_, AppState>) -> Result<(), String> {
    crate::storage::scratchpad::set_size(&state.db, w, h).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_scratchpad_capture(on: bool, state: State<'_, AppState>) -> Result<(), String> {
    state
        .scratchpad_capture
        .store(on, std::sync::atomic::Ordering::Release);
    Ok(())
}

#[tauri::command]
pub async fn scratchpad_get_capture(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(state
        .scratchpad_capture
        .load(std::sync::atomic::Ordering::Acquire))
}

#[tauri::command]
pub async fn scratchpad_get(state: State<'_, AppState>) -> Result<ScratchpadData, String> {
    scratchpad::get_data(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scratchpad_set_note(content: String, state: State<'_, AppState>) -> Result<(), String> {
    scratchpad::set_note(&state.db, &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scratchpad_add_entry(
    pad_id: Option<String>,
    content: String,
    state: State<'_, AppState>,
) -> Result<ScratchpadEntry, String> {
    scratchpad::add_entry(&state.db, pad_id.as_deref(), &content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scratchpad_delete_entry(id: String, state: State<'_, AppState>) -> Result<(), String> {
    scratchpad::delete_entry(&state.db, &id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scratchpad_clear_pad(
    pad_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    scratchpad::clear_pad(&state.db, pad_id.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scratchpad_set_variant(variant: String, state: State<'_, AppState>) -> Result<(), String> {
    scratchpad::set_variant(&state.db, &variant).map_err(|e| e.to_string())
}
