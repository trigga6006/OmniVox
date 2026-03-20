pub mod audio;
pub mod asr;
pub mod commands;
pub mod error;
pub mod models;
pub mod output;
pub mod pipeline;
pub mod postprocess;
pub mod state;
pub mod storage;

use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = tauri::menu::MenuItem::with_id(app, "show", "Show OmniVox")?;
    let quit_item = tauri::menu::MenuItem::with_id(app, "quit", "Quit")?;
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

/// On first launch, copy the bundled whisper-base.en model from app resources
/// into the user's models directory and auto-activate it.
///
/// The model file (`ggml-base.en.bin`) is placed in `src-tauri/resources/` at
/// build time and bundled into the installer by Tauri's resource system.
fn setup_bundled_model(app: &tauri::App, state: &state::AppState) {
    use models::downloader::model_filename;
    use models::manager::BUNDLED_MODEL_ID;

    let filename = model_filename(BUNDLED_MODEL_ID);
    let target = state.models_dir.join(&filename);

    // Already exists — just make sure it's activated
    if target.exists() {
        if state.active_model_id.lock().unwrap().is_none() {
            let _ = commands::models::load_and_activate_model(BUNDLED_MODEL_ID, state);
        }
        return;
    }

    // Copy from Tauri's bundled resources
    let resource_path = app
        .path()
        .resolve(format!("resources/{filename}"), tauri::path::BaseDirectory::Resource);

    if let Ok(source) = resource_path {
        if source.exists() {
            if let Ok(()) = std::fs::copy(&source, &target).map(|_| ()) {
                let _ = commands::models::load_and_activate_model(BUNDLED_MODEL_ID, state);
                eprintln!("Bundled model installed and activated: {BUNDLED_MODEL_ID}");
            }
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == ShortcutState::Pressed {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(async move {
                            pipeline::toggle_recording(&handle).await;
                        });
                    }
                })
                .build(),
        )
        .manage(state::AppState::new())
        .setup(|app| {
            setup_tray(app)?;

            // Register default hotkey: Ctrl+Win to toggle recording
            use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifier, Shortcut};
            let shortcut =
                Shortcut::new(Some(Modifier::CONTROL), Code::MetaLeft);
            app.global_shortcut().register(shortcut)?;

            // Ensure data directories exist
            let state = app.state::<state::AppState>();
            let _ = std::fs::create_dir_all(&state.models_dir);
            let _ = std::fs::create_dir_all(&state.data_dir);

            // First-launch: copy bundled model from app resources to models dir
            // and auto-activate it so the user can dictate immediately.
            setup_bundled_model(app, &state);

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
            // Settings commands (2)
            commands::get_settings,
            commands::update_settings,
        ])
        .run(tauri::generate_context!())
        .expect("error while running OmniVox application");
}
