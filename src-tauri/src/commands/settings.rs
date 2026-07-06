use crate::hotkey::HotkeyConfig;
use crate::output::types::OutputMode;
use crate::postprocess::types::WritingStyle;
use crate::state::AppState;
use crate::storage::types::AppSettings;
use tauri::{Emitter, Manager, State};

// Gap between the pill and the bottom of the monitor's WORK AREA (the
// screen minus taskbar/appbars, from the OS) — no taskbar-height guessing.
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

/// Find which monitor currently contains the mouse cursor on macOS.
/// Uses CoreGraphics CGEvent API to get the cursor position.
#[cfg(target_os = "macos")]
fn cursor_monitor(app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    use core_graphics::event::CGEvent;
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    let source = CGEventSource::new(CGEventSourceStateID::CombinedSessionState).ok()?;
    let event = CGEvent::new(source).ok()?;
    let cursor_pos = event.location();

    // CoreGraphics uses a coordinate system where Y=0 is at the top of the
    // primary display. Tauri monitor positions use the same convention.
    let cx = cursor_pos.x as i32;
    let cy = cursor_pos.y as i32;

    let monitors = app.available_monitors().ok()?;
    monitors.into_iter().find(|mon| {
        let pos = mon.position();
        let size = mon.size();
        cx >= pos.x
            && cx < pos.x + size.width as i32
            && cy >= pos.y
            && cy < pos.y + size.height as i32
    })
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn cursor_monitor(_app: &tauri::AppHandle) -> Option<tauri::Monitor> {
    None
}

/// Resize and reposition the overlay pill window from the frontend.
/// Automatically moves the pill to whichever monitor has the cursor,
/// so it follows the user across multi-monitor setups.
///
/// On Windows, the size + position are applied atomically via a single
/// `SetWindowPos` call.  Using Tauri's `set_size` followed by
/// `set_position` produced a visible flicker on the primary monitor
/// when opening the right-click menu: between the two IPC calls the
/// window briefly exists at the OLD position with the NEW (much larger)
/// size, so the pill appeared to jump toward the top-left before
/// settling.  A single `SetWindowPos` skips that intermediate state.
#[tauri::command]
pub async fn resize_overlay(app: tauri::AppHandle, width: f64, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found")?;

    // Prefer the monitor with the cursor; fall back to the overlay's current monitor.
    let target = cursor_monitor(&app)
        .or_else(|| window.current_monitor().ok().flatten())
        .ok_or("no monitor")?;

    let scale = target.scale_factor();
    let wa = target.work_area();

    // Calculate position in physical pixels, centered at the bottom of the
    // target monitor's work area (excludes the taskbar wherever it is —
    // bottom, side, scaled, or auto-hidden).
    let phys_w = width * scale;
    let phys_h = height * scale;
    let margin_phys = MARGIN * scale;

    let x = wa.position.x as f64 + (wa.size.width as f64 - phys_w) / 2.0;
    let y = wa.position.y as f64 + wa.size.height as f64 - phys_h - margin_phys;
    let xi = x as i32;
    let yi = y.max(0.0) as i32;

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
        };

        if let Ok(hwnd_raw) = window.hwnd() {
            // Convert Tauri's HWND to windows-sys HWND (both are `isize`
            // pointers wrapping the same OS handle).
            let hwnd: HWND = hwnd_raw.0 as HWND;
            let w_phys = phys_w.round() as i32;
            let h_phys = phys_h.round() as i32;
            // SAFETY: hwnd is valid (just retrieved from Tauri), flags
            // are well-formed, no z-order or activation change.
            let ok = unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    xi,
                    yi,
                    w_phys,
                    h_phys,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            if ok != 0 {
                return Ok(());
            }
            // If SetWindowPos failed for some reason, fall through to
            // the cross-platform path so resize still happens.
        }
    }

    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(xi, yi))
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> Result<AppSettings, String> {
    crate::storage::settings::get_settings(&state.db).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_settings(
    app: tauri::AppHandle,
    mut settings: AppSettings,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Snapshot previous structured_mode before we overwrite so we can tell
    // if the user just turned it off.  Structured Mode off → drop the loaded
    // LLM to reclaim ~180 MB cleanly (the plan's "users who disable should
    // reclaim RAM cleanly" rule).
    let prev_structured = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.structured_mode)
        .unwrap_or(false);
    let prev_command_mode = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.command_mode)
        .unwrap_or(false);
    let prev_auto_start = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.auto_start)
        .unwrap_or(false);

    // If Structured Mode is being enabled without an explicit active model,
    // auto-pick the best downloaded one so the app never enters a misleading
    // "structured on, but guaranteed to degrade" state.
    if settings.structured_mode
        && settings
            .active_llm_model_id
            .as_deref()
            .unwrap_or("")
            .is_empty()
    {
        settings.active_llm_model_id =
            crate::commands::llm::preferred_downloaded_llm_id(state.inner());
    }

    // Persist to SQLite
    crate::storage::settings::update_settings(&state.db, &settings).map_err(|e| e.to_string())?;

    if prev_structured && !settings.structured_mode {
        if let Ok(mut guard) = state.llm_runner.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = state.active_llm_model_id.lock() {
            *guard = None;
        }
    }

    // Structured Mode just turned on AND a model is chosen but not loaded →
    // load it eagerly so the first dictation doesn't eat the load time.
    // The multi-second GGUF load runs on a blocking thread so the settings
    // command (and the toggle in the UI) returns immediately.
    if !prev_structured && settings.structured_mode {
        if let Some(model_id) = settings.active_llm_model_id.clone() {
            if let Ok(mut guard) = state.active_llm_model_id.lock() {
                *guard = Some(model_id.clone());
            }
            let runner_loaded = state
                .llm_runner
                .lock()
                .ok()
                .map(|g| g.is_some())
                .unwrap_or(false);
            if !runner_loaded {
                let app_for_load = app.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    let st = app_for_load.state::<AppState>();
                    if let Err(e) = crate::commands::llm::load_and_activate_llm(&model_id, &st) {
                        eprintln!("Eager LLM load on toggle failed: {e}");
                    }
                });
            }
        }
    }

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

    // Sync writing style + filler removal to the processor chain
    if let Ok(mut proc) = state.processor.lock() {
        proc.set_style(WritingStyle::from_str(&settings.writing_style));
        proc.set_filler_removal(settings.filler_removal);
    }

    // Sync hotkey to the live hook
    if let Some(ref hk) = settings.hotkey {
        let key1 = hk.keys.first().copied().unwrap_or(0);
        let key2 = hk.keys.get(1).copied().unwrap_or(0);
        crate::hotkey::update_hotkey_keys(key1, key2);
    }

    // Sync launch-at-startup with the OS (registry Run key on Windows).
    // Only touch the registry when the setting actually changed.
    if settings.auto_start != prev_auto_start {
        use tauri_plugin_autostart::ManagerExt;
        let autolaunch = app.autolaunch();
        let result = if settings.auto_start {
            autolaunch.enable()
        } else {
            autolaunch.disable()
        };
        if let Err(e) = result {
            eprintln!("Failed to update launch-at-startup: {e}");
        }
    }

    // Sync Command-Mode hotkey activation; warm the app index the first time
    // Command Mode is switched on so the first command isn't slowed by the
    // PowerShell enumeration.
    crate::hotkey::set_command_mode_enabled(settings.command_mode);
    if settings.command_mode && !prev_command_mode {
        let _ = tokio::task::spawn_blocking(crate::actions::app_index::refresh);
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

/// Forward a modifier key event from a focused OmniVox window into the hotkey
/// state machine.  The global OS keyboard hook gets nothing while our own
/// WebView has focus, so the frontend bridges key down/up events here.
#[tauri::command]
pub async fn feed_hotkey_event(vk: u16, down: bool) -> Result<(), String> {
    crate::hotkey::feed_key_event(vk, down);
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

/// Recover the floating overlay pill from any "lost" state — invisible due
/// to ghost mode, parked on a disconnected monitor, hidden by another
/// always-on-top app, etc.  Invoked from the tray menu's "Reset Pill" item.
///
/// What it does, in order:
///   1. Force `ghost_mode = false` in the DB and broadcast a settings-changed
///      event so the FloatingPill un-ghosts.
///   2. Show the overlay window, re-assert always-on-top, and unminimize.
///   3. Reposition to the primary monitor's center-bottom via the same
///      `SetWindowPos` path `resize_overlay` uses — survives monitor
///      changes that left the pill parked off-screen.
///
/// If the overlay window has been destroyed entirely (rare — WebView2
/// process kill), this returns an error and the user has to fully restart
/// the app.  Rebuilding a Tauri window from an AppHandle mid-runtime is
/// possible but pulls in enough complexity that it's not worth it for the
/// once-in-a-blue-moon case.
#[tauri::command]
pub async fn recover_overlay(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // 1. Force ghost mode off so the pill isn't invisible after we show it.
    let mut settings =
        crate::storage::settings::get_settings(&state.db).map_err(|e| e.to_string())?;
    if settings.ghost_mode {
        settings.ghost_mode = false;
        crate::storage::settings::update_settings(&state.db, &settings)
            .map_err(|e| e.to_string())?;
        let _ = app.emit("settings-changed", &settings);
    }

    let window = app
        .get_webview_window("overlay")
        .ok_or("overlay window not found — fully quit and relaunch OmniVox to recreate it")?;

    // 2. Recover from hidden / z-order-lost states.  set_always_on_top is
    //    idempotent; calling it again forces the OS to re-evaluate z-order
    //    in case a fullscreen app stole the spot.
    let _ = window.unminimize();
    window.show().map_err(|e| e.to_string())?;
    let _ = window.set_always_on_top(true);

    // 3. Reposition to primary monitor center-bottom.  We deliberately use
    //    the primary monitor (not the cursor monitor) for recovery because
    //    the cursor may itself be on a disconnected monitor in the bad
    //    state we're trying to escape.
    let target = app
        .primary_monitor()
        .map_err(|e| e.to_string())?
        .ok_or("no primary monitor")?;

    let scale = target.scale_factor();
    let wa = target.work_area();

    // Idle window size — shared const, matches useOverlaySizing.ts IDLE_WIN_W/H.
    let pill_w_logical = crate::OVERLAY_IDLE_WIN_W;
    let pill_h_logical = crate::OVERLAY_IDLE_WIN_H;
    let phys_w = pill_w_logical * scale;
    let phys_h = pill_h_logical * scale;
    let margin_phys = MARGIN * scale;

    let x = wa.position.x as f64 + (wa.size.width as f64 - phys_w) / 2.0;
    let y = wa.position.y as f64 + wa.size.height as f64 - phys_h - margin_phys;
    let xi = x as i32;
    let yi = y.max(0.0) as i32;

    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
        };

        if let Ok(hwnd_raw) = window.hwnd() {
            let hwnd: HWND = hwnd_raw.0 as HWND;
            let w_phys = phys_w.round() as i32;
            let h_phys = phys_h.round() as i32;
            // SAFETY: hwnd is valid (just retrieved from Tauri), flags are
            // well-formed, no z-order or activation change beyond what
            // set_always_on_top above already requested.
            let ok = unsafe {
                SetWindowPos(
                    hwnd,
                    std::ptr::null_mut(),
                    xi,
                    yi,
                    w_phys,
                    h_phys,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
            };
            if ok != 0 {
                return Ok(());
            }
        }
    }

    window
        .set_size(tauri::LogicalSize::new(pill_w_logical, pill_h_logical))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(xi, yi))
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Persist a new hotkey config and activate it immediately.
#[tauri::command]
pub async fn update_hotkey(config: HotkeyConfig, state: State<'_, AppState>) -> Result<(), String> {
    if config.keys.is_empty() || config.keys.len() > 2 {
        return Err("Hotkey must be 1 or 2 keys".into());
    }

    // Persist to SQLite
    let mut settings =
        crate::storage::settings::get_settings(&state.db).map_err(|e| e.to_string())?;
    settings.hotkey = Some(config.clone());
    crate::storage::settings::update_settings(&state.db, &settings).map_err(|e| e.to_string())?;

    // Live-update the hook
    let key1 = config.keys[0];
    let key2 = config.keys.get(1).copied().unwrap_or(0);
    crate::hotkey::update_hotkey_keys(key1, key2);

    // Un-suspend in case we were in listening mode
    crate::hotkey::set_suspended(false);

    Ok(())
}
