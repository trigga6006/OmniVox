pub mod audio;
pub mod asr;
pub mod commands;
pub mod error;
pub mod hotkey;
pub mod models;
pub mod output;
pub mod pipeline;
pub mod postprocess;
pub mod state;
pub mod storage;

use tauri::Manager;

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = tauri::menu::MenuItem::with_id(app, "show", "Show OmniVox", true, None::<&str>)?;
    let quit_item = tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&show_item, &quit_item])?;

    tauri::tray::TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("OmniVox")
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

/// Create the floating overlay pill window — transparent, borderless,
/// always-on-top, positioned just above the taskbar/dock at screen center.
fn setup_overlay_window(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::WebviewWindowBuilder;

    // Start at active size — the frontend shrinks it to idle once mounted.
    let pill_width = 210.0_f64;
    let pill_height = 34.0_f64;
    let taskbar_height = 48.0_f64;
    let margin = 12.0_f64;

    let (x, y) = if let Some(monitor) = app.primary_monitor()? {
        let size = monitor.size();
        let scale = monitor.scale_factor();
        let screen_w = size.width as f64 / scale;
        let screen_h = size.height as f64 / scale;
        (
            (screen_w - pill_width) / 2.0,
            screen_h - taskbar_height - pill_height - margin,
        )
    } else {
        (400.0, 800.0) // fallback
    };

    let _overlay = WebviewWindowBuilder::new(app, "overlay", tauri::WebviewUrl::App("/overlay.html".into()))
        .title("")
        .inner_size(pill_width, pill_height)
        .min_inner_size(1.0, 1.0)
        .position(x, y)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .focused(false)
        .visible(true)
        .build()?;

    Ok(())
}

/// Copy bundled model files from Tauri resources to user directories on first launch.
/// Does NOT load any models — that's handled by `load_default_model_deferred`.
///
/// Called from the deferred background task (not `setup()` directly) so that
/// a 1.5 GB `fs::copy` of the bundled medium-en model doesn't block the
/// window from appearing on first launch.  Takes `AppHandle` instead of
/// `&App` so it can run off the main thread.
fn copy_bundled_resources(app: &tauri::AppHandle, state: &state::AppState) {
    use models::downloader::model_filename;
    use models::manager::BUNDLED_MODEL_ID;

    let filename = model_filename(BUNDLED_MODEL_ID);
    let target = state.models_dir.join(&filename);

    if target.exists() {
        return;
    }

    let resource_path = app
        .path()
        .resolve(format!("resources/{filename}"), tauri::path::BaseDirectory::Resource);

    if let Ok(source) = resource_path {
        if source.exists() {
            if std::fs::copy(&source, &target).is_ok() {
                eprintln!("Bundled Whisper model installed: {BUNDLED_MODEL_ID}");
                // The cached model list in ModelManager was built before the
                // copy completed, so it still says the bundled model isn't
                // downloaded.  Invalidate so the UI reflects reality.
                state.model_manager.invalidate_cache();
            }
        }
    }
}

/// Load the default Whisper model on a background thread after the window is up.
/// Failures are logged but never crash the app.
fn load_default_model_deferred(app_handle: &tauri::AppHandle, state: &state::AppState) {
    use models::manager::BUNDLED_MODEL_ID;
    use tauri::Emitter;

    if state.active_model_id.lock().unwrap().is_some() {
        return; // already loaded
    }

    // Prefer the persisted model choice; fall back to the bundled default.
    let model_id = crate::storage::settings::get_settings(&state.db)
        .ok()
        .and_then(|s| s.active_model_id)
        .unwrap_or_else(|| BUNDLED_MODEL_ID.to_string());

    // Check if the chosen model exists on disk
    if state.model_manager.model_path(&model_id).is_none() {
        // Fall back to bundled model if persisted choice is missing
        if model_id != BUNDLED_MODEL_ID {
            eprintln!("Persisted model '{model_id}' not found, falling back to bundled");
            if state.model_manager.model_path(BUNDLED_MODEL_ID).is_none() {
                eprintln!("No bundled model found on disk — skipping auto-load");
                return;
            }
            eprintln!("Loading Whisper model in background...");
            match commands::models::load_and_activate_model(BUNDLED_MODEL_ID, state) {
                Ok(()) => {
                    eprintln!("Whisper model loaded successfully");
                    let _ = app_handle.emit("model-loaded", BUNDLED_MODEL_ID);
                }
                Err(e) => {
                    eprintln!("Failed to load Whisper model (app still usable): {e}");
                    let _ = app_handle.emit("model-load-error", e);
                }
            }
            return;
        }
        eprintln!("No bundled model found on disk — skipping auto-load");
        return;
    }

    eprintln!("Loading Whisper model in background...");
    match commands::models::load_and_activate_model(&model_id, state) {
        Ok(()) => {
            eprintln!("Whisper model loaded successfully");
            let _ = app_handle.emit("model-loaded", model_id);
        }
        Err(e) => {
            eprintln!("Failed to load Whisper model (app still usable): {e}");
            let _ = app_handle.emit("model-load-error", e);
        }
    }
}

/// Load persisted settings from SQLite and apply them to in-memory state.
fn apply_persisted_settings(state: &state::AppState) {
    if let Ok(settings) = crate::storage::settings::get_settings(&state.db) {
        let mode = match settings.output_mode.as_str() {
            "type_simulation" => crate::output::types::OutputMode::TypeSimulation,
            "both" => crate::output::types::OutputMode::Both,
            _ => crate::output::types::OutputMode::Clipboard,
        };
        if let Ok(mut cfg) = state.output_config.lock() {
            cfg.mode = mode;
            cfg.ship_mode = settings.ship_mode;
        }

        // Load hotkey config into the hook (before hook thread starts).
        if let Some(ref hk) = settings.hotkey {
            let key1 = hk.keys.first().copied().unwrap_or(0);
            let key2 = hk.keys.get(1).copied().unwrap_or(0);
            hotkey::update_hotkey_keys(key1, key2);
        }
    }

    // Load dictionary entries and snippets into the in-memory ProcessorChain
    if let Ok(mut processor) = state.processor.lock() {
        if let Ok(entries) = crate::storage::dictionary::list_entries(&state.db) {
            processor.set_dictionary(entries);
        }
        if let Ok(snippets) = crate::storage::snippets::list_snippets(&state.db) {
            processor.set_snippets(snippets);
        }
    }
}

/// Suppress MSVC debug CRT assertion dialogs that appear in Windows GUI apps.
///
/// The debug UCRT (`ucrtbased.dll`) contains `_ASSERTE` checks that show modal
/// dialogs when C code hits invalid file handles (e.g., writing to stderr in a
/// no-console GUI app). This function dynamically hooks into the debug CRT's
/// report system to silently suppress these dialogs. In release builds (which
/// use `ucrtbase.dll`), this is a no-op — `GetModuleHandleA` returns null.
#[cfg(windows)]
unsafe fn suppress_crt_asserts() {
    extern "system" {
        fn GetModuleHandleA(name: *const u8) -> *mut std::ffi::c_void;
        fn GetProcAddress(
            module: *mut std::ffi::c_void,
            name: *const u8,
        ) -> *mut std::ffi::c_void;
    }

    // Only act if the debug UCRT is loaded (debug builds only).
    let ucrtd = GetModuleHandleA(b"ucrtbased.dll\0".as_ptr());
    if ucrtd.is_null() {
        return;
    }

    // Hook _CrtSetReportHook2 to intercept assertion reports before the dialog.
    type ReportHookFn = unsafe extern "C" fn(i32, *const i8, *mut i32) -> i32;
    type SetReportHook2 =
        unsafe extern "C" fn(i32, Option<ReportHookFn>) -> i32;
    type SetReportMode = unsafe extern "C" fn(i32, i32) -> i32;

    // Our hook: suppress all reports (return 1 = handled, don't show dialog).
    unsafe extern "C" fn suppress_hook(
        _report_type: i32,
        _message: *const i8,
        return_value: *mut i32,
    ) -> i32 {
        if !return_value.is_null() {
            *return_value = 0; // don't break into debugger
        }
        1 // report handled — suppress dialog
    }

    // Try _CrtSetReportHook2 first (most reliable)
    let proc = GetProcAddress(ucrtd, b"_CrtSetReportHook2\0".as_ptr());
    if !proc.is_null() {
        let set_hook: SetReportHook2 = std::mem::transmute(proc);
        set_hook(0, Some(suppress_hook)); // 0 = _CRT_RPTHK_INSTALL
    }

    // Also disable assertion dialog via _CrtSetReportMode as belt-and-suspenders.
    let proc = GetProcAddress(ucrtd, b"_CrtSetReportMode\0".as_ptr());
    if !proc.is_null() {
        let set_mode: SetReportMode = std::mem::transmute(proc);
        set_mode(2, 0); // _CRT_ASSERT = 2, disable all output
    }
}

/// Remove `.part` files left behind by interrupted model downloads.
/// Only deletes files older than 1 hour to avoid racing with an active download.
fn cleanup_part_files(models_dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(models_dir) else { return };
    let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("part") {
            let is_stale = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| t < cutoff)
                .unwrap_or(false);
            if is_stale {
                eprintln!("Removing orphaned download: {}", path.display());
                let _ = std::fs::remove_file(&path);
            }
        }
    }
}

pub fn run() {
    // In debug builds on Windows, the debug UCRT (ucrtbased.dll) has _ASSERTE
    // checks that show modal dialogs when C code touches invalid file handles
    // (e.g. fprintf(stderr) in a GUI app with no console). We can't disable
    // these at compile time because they're in the system DLL. Instead, hook
    // into the debug CRT's report system to silently suppress them.
    #[cfg(windows)]
    unsafe {
        // 1. Suppress debug CRT assertion dialogs (hooks into ucrtbased.dll).
        suppress_crt_asserts();

        // 2. Create a hidden console so stderr/stdout are valid file handles.
        //    Without this, any C fprintf(stderr, ...) hits an invalid fd.
        extern "system" {
            fn AllocConsole() -> i32;
            fn GetConsoleWindow() -> isize;
            fn ShowWindow(hwnd: isize, cmd: i32) -> i32;
        }
        AllocConsole();
        let console = GetConsoleWindow();
        if console != 0 {
            ShowWindow(console, 0); // SW_HIDE
        }
    }

    let builder = tauri::Builder::default().plugin(tauri_plugin_shell::init());

    #[cfg(not(debug_assertions))]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Another instance was launched — focus the existing main window.
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
        }));

    builder.manage(state::AppState::new())
        .setup(|app| {
            setup_tray(app)?;

            // Ensure data directories exist
            let state = app.state::<state::AppState>();
            let _ = std::fs::create_dir_all(&state.models_dir);
            let _ = std::fs::create_dir_all(&state.data_dir);

            // Clean up orphaned .part files from interrupted downloads
            cleanup_part_files(&state.models_dir);

            // Load persisted settings (output mode, etc.) into in-memory state
            apply_persisted_settings(&state);

            // Seed the default "General" context mode and load the active mode
            if let Ok(general_id) = crate::storage::context_modes::seed_general_mode(&state.db) {
                // Load the persisted active mode, or default to General
                let active_id = crate::storage::settings::get_settings(&state.db)
                    .ok()
                    .and_then(|s| s.active_context_mode_id)
                    .unwrap_or(general_id.clone());

                *state.active_context_mode_id.lock().unwrap() = Some(active_id.clone());

                // Load global + mode-scoped dictionary/snippets into processor
                commands::dictionary::sync_processor(&state);
            }

            // First-launch: copy bundled models from app resources to user
            // dirs.  We do this in the same deferred task as model loading so
            // the ~1.5 GB fs::copy of the bundled medium-en model doesn't
            // block setup() from returning — previously this delayed the
            // window appearing by 3–5 s on first launch.  The copy runs
            // inside spawn_blocking so the async runtime stays responsive
            // for tray + window events.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Let the window render first
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;

                // Copy bundled resources on a blocking thread (fs::copy on a
                // multi-GB file blocks for seconds; keep it off the async pool).
                {
                    let h = handle.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let st = h.state::<state::AppState>();
                        copy_bundled_resources(&h, &st);
                    })
                    .await;
                }

                let st = handle.state::<state::AppState>();
                load_default_model_deferred(&handle, &st);
            });

            // Create the floating overlay pill — always-on-top, transparent,
            // positioned just above the taskbar/dock.
            setup_overlay_window(app)?;

            // Install the hotkey via a low-level keyboard hook.
            hotkey::install(app.handle().clone());

            Ok(())
        })
        .on_window_event(|window, event| {
            // Hide the main window on close instead of destroying it.
            // The overlay pill stays visible and the app keeps running in the tray.
            // Users restore it via the tray icon "Show" option or by re-launching
            // (which the single-instance plugin redirects to show the existing window).
            if window.label() == "main" {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // Audio commands (8)
            commands::start_recording,
            commands::stop_recording,
            commands::cancel_recording,
            commands::get_audio_devices,
            commands::set_audio_device,
            commands::open_mic_settings,
            commands::open_accessibility_settings,
            commands::get_platform_info,
            // Model commands (6)
            commands::list_models,
            commands::download_model,
            commands::delete_model,
            commands::get_active_model,
            commands::set_active_model,
            commands::get_hardware_info,
            commands::get_gpu_support,
            // Dictionary & snippet commands (8)
            commands::add_dictionary_entry,
            commands::update_dictionary_entry,
            commands::delete_dictionary_entry,
            commands::list_dictionary_entries,
            commands::add_snippet,
            commands::update_snippet,
            commands::delete_snippet,
            commands::list_snippets,
            // History commands (5)
            commands::search_history,
            commands::recent_history,
            commands::delete_history_record,
            commands::export_history,
            commands::get_dictation_stats,
            // Settings & hotkey commands (5)
            commands::get_settings,
            commands::update_settings,
            commands::suspend_hotkey,
            commands::update_hotkey,
            commands::resize_overlay,
            commands::show_main_window,
            // Notes commands (4)
            commands::add_note,
            commands::update_note,
            commands::delete_note,
            commands::list_notes,
            // Mode-scoped dictionary/snippet commands (6)
            commands::list_mode_dictionary_entries,
            commands::add_mode_dictionary_entry,
            commands::delete_mode_dictionary_entry,
            commands::list_mode_snippets,
            commands::add_mode_snippet,
            commands::delete_mode_snippet,
            // Vocabulary commands (7)
            commands::add_vocabulary_entry,
            commands::update_vocabulary_entry,
            commands::delete_vocabulary_entry,
            commands::list_vocabulary_entries,
            commands::list_mode_vocabulary_entries,
            commands::add_mode_vocabulary_entry,
            commands::delete_mode_vocabulary_entry,
            // Context modes (7)
            commands::list_context_modes,
            commands::get_context_mode,
            commands::create_context_mode,
            commands::update_context_mode,
            commands::delete_context_mode,
            commands::get_active_context_mode,
            commands::set_active_context_mode,
            // App binding commands (3)
            commands::list_app_bindings,
            commands::add_app_binding,
            commands::delete_app_binding,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OmniVox application");
}
