pub mod audio;
pub mod asr;
pub mod commands;
pub mod error;
pub mod hotkey;
pub mod llm;
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
/// always-on-top, positioned just above the Windows taskbar at screen center.
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

    let _overlay = WebviewWindowBuilder::new(app, "overlay", tauri::WebviewUrl::App("/".into()))
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
fn copy_bundled_resources(app: &tauri::App, state: &state::AppState) {
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

    // Check if the bundled model exists on disk
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
}

/// On first launch, copy the bundled LLM model from app resources into
/// the user's LLM models directory. The model is NOT loaded here — users
/// enable AI cleanup from the Settings UI which triggers the load.
fn setup_bundled_llm_model(app: &tauri::App, state: &state::AppState) {
    const LLM_MODEL_FILENAME: &str = "Qwen3-0.6B-Q4_K_M.gguf";
    let target = state.llm_models_dir.join(LLM_MODEL_FILENAME);

    if target.exists() {
        return;
    }

    let resource_path = app
        .path()
        .resolve(format!("resources/{LLM_MODEL_FILENAME}"), tauri::path::BaseDirectory::Resource);

    if let Ok(source) = resource_path {
        if source.exists() {
            let _ = std::fs::create_dir_all(&state.llm_models_dir);
            if std::fs::copy(&source, &target).is_ok() {
                eprintln!("Bundled LLM model installed: {LLM_MODEL_FILENAME}");
            }
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

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(state::AppState::new())
        .setup(|app| {
            setup_tray(app)?;

            // Ensure data directories exist
            let state = app.state::<state::AppState>();
            let _ = std::fs::create_dir_all(&state.models_dir);
            let _ = std::fs::create_dir_all(&state.llm_models_dir);
            let _ = std::fs::create_dir_all(&state.data_dir);

            // Load persisted settings (output mode, etc.) into in-memory state
            apply_persisted_settings(&state);

            // First-launch: copy bundled models from app resources to user dirs.
            // Model loading is deferred to a background task so that a slow or
            // crashing model load never prevents the window from appearing.
            copy_bundled_resources(app, &state);
            setup_bundled_llm_model(app, &state);
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Let the window render first
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                let st = handle.state::<state::AppState>();
                load_default_model_deferred(&handle, &st);
            });

            // Create the floating overlay pill — always-on-top, transparent,
            // positioned just above the Windows taskbar.
            setup_overlay_window(app)?;

            // Install the hotkey via a low-level keyboard hook.
            hotkey::install(app.handle().clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Audio commands (5)
            commands::start_recording,
            commands::stop_recording,
            commands::cancel_recording,
            commands::get_audio_devices,
            commands::set_audio_device,
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
            // History commands (4)
            commands::search_history,
            commands::recent_history,
            commands::delete_history_record,
            commands::export_history,
            // Settings & hotkey commands (5)
            commands::get_settings,
            commands::update_settings,
            commands::suspend_hotkey,
            commands::update_hotkey,
            commands::resize_overlay,
            // Notes commands (4)
            commands::add_note,
            commands::update_note,
            commands::delete_note,
            commands::list_notes,
            // LLM / AI cleanup commands (4)
            commands::get_ai_cleanup_status,
            commands::download_llm_model,
            commands::enable_ai_cleanup,
            commands::disable_ai_cleanup,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OmniVox application");
}
