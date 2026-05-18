use std::sync::Arc;

use tauri::{Emitter, State};

use crate::llm::engine::LlamaEngine;
use crate::llm::runner::LlmRunner;
use crate::llm::schema::SlotExtraction;
use crate::llm::template::render_markdown;
use crate::llm::types::LlmConfig;
use crate::llm_models::types::LlmModelInfo;
use crate::state::AppState;

/// Pick the best downloaded LLM when Structured Mode is enabled without an
/// explicit active model selection.
pub fn preferred_downloaded_llm_id(state: &AppState) -> Option<String> {
    let models = state.llm_model_manager.list_available();
    models
        .iter()
        .find(|m| m.is_downloaded && m.is_default)
        .or_else(|| models.iter().find(|m| m.is_downloaded))
        .map(|m| m.id.clone())
}

#[tauri::command]
pub async fn list_llm_models(state: State<'_, AppState>) -> Result<Vec<LlmModelInfo>, String> {
    Ok(state.llm_model_manager.list_available())
}

#[tauri::command]
pub async fn download_llm_model(
    model_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .llm_downloader
        .download(&state.llm_model_manager, &model_id, &app_handle)
        .await
        .map_err(|e| e.to_string())?;
    state.llm_model_manager.invalidate_cache();
    Ok(())
}

#[tauri::command]
pub async fn delete_llm_model(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // If this is the active model, unload the runner first so the worker
    // drops its LlamaEngine before we delete the on-disk weights, AND clear
    // the persisted active_llm_model_id so a later eager-load doesn't try
    // to re-load a file that no longer exists.
    let was_active = {
        let active = state.active_llm_model_id.lock().unwrap();
        active.as_deref() == Some(&model_id)
    };
    if was_active {
        *state.llm_runner.lock().unwrap() = None;
        *state.active_llm_model_id.lock().unwrap() = None;
        if let Ok(mut settings) = crate::storage::settings::get_settings(&state.db) {
            if settings.active_llm_model_id.as_deref() == Some(&model_id) {
                settings.active_llm_model_id = None;
                let _ = crate::storage::settings::update_settings(&state.db, &settings);
            }
        }
    }
    state
        .llm_model_manager
        .delete(&model_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_llm_model(
    state: State<'_, AppState>,
) -> Result<Option<LlmModelInfo>, String> {
    let active_id = state.active_llm_model_id.lock().unwrap().clone();
    match active_id {
        Some(id) => Ok(state.llm_model_manager.get_model(&id)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn set_active_llm_model(
    model_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    load_and_activate_llm(&model_id, &state)?;
    let _ = app_handle.emit("llm-model-loaded", &model_id);
    Ok(())
}

/// Paste structured Markdown (from the overlay panel's Paste button) using
/// the current OutputConfig.  Focus restoration is attempted based on the
/// most recent `prev_foreground` snapshot — if none is recorded, we just
/// rely on the clipboard/type-sim fallthrough in OutputRouter.
#[tauri::command]
pub async fn paste_structured_output(
    markdown: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let prev_hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
    if let Some(hwnd) = prev_hwnd {
        let _ = tokio::task::spawn_blocking(move || {
            crate::pipeline::restore_foreground_window_public(hwnd)
        })
        .await;
    }
    state
        .output
        .send(&markdown, &output_config)
        .map_err(|e| e.to_string())
}

/// Dev / Settings "Test" button — runs the currently loaded LLM on a canned
/// input (or a user-provided one) and returns the rendered Markdown.
#[tauri::command]
pub async fn llm_test_extract(
    text: Option<String>,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let runner = state
        .llm_runner
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "No LLM model loaded".to_string())?;
    let input = text.unwrap_or_else(|| {
        "Refactor the checkout flow in billing.tsx and cart.tsx. Keep the Stripe integration. Urgent.".to_string()
    });
    let slots: SlotExtraction = runner
        .extract_with_timeout(input, std::time::Duration::from_secs(15))
        .await
        .map_err(|e| e.to_string())?;
    Ok(render_markdown(&slots))
}

/// Load a GGUF model and install it as the active LLM runner.
///
/// Runs the load on a dedicated 256 MB-stack thread with `catch_unwind` —
/// mirrors `commands::models::load_and_activate_model` because llama.cpp
/// has the same huge debug-build stack frames that crash on Windows
/// without the wider stack.
pub fn load_and_activate_llm(model_id: &str, state: &AppState) -> Result<(), String> {
    let model_path = state
        .llm_model_manager
        .model_path(model_id)
        .ok_or_else(|| format!("LLM model '{model_id}' is not downloaded"))?;

    // Read the user's GPU preference — reuse the same toggle as Whisper,
    // since compile-time enabling the Vulkan/CUDA feature pulls in both
    // backends at once.
    let use_gpu = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.gpu_acceleration)
        .unwrap_or(false);

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(2).min(8) as i32)
        .unwrap_or(4);

    let config = LlmConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        n_threads,
        use_gpu,
        n_ctx: 2048,
        max_tokens: 384,
    };

    // Drop the previous runner before loading a replacement so llama.cpp does
    // not hold old and new GGUF weights in RAM/VRAM at the same time.
    {
        let mut runner = state.llm_runner.lock().unwrap();
        if runner.as_ref().map(|runner| runner.is_busy()).unwrap_or(false) {
            return Err(
                "Wait for the current Structured Mode extraction before switching LLM models"
                    .into(),
            );
        }
        *runner = None;
    }
    *state.active_llm_model_id.lock().unwrap() = None;

    // Load on a wide-stack thread to survive llama.cpp's debug stack frames.
    let engine = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match LlamaEngine::load(config.clone()) {
                    Ok(engine) => Ok(engine),
                    Err(e) if config.use_gpu => {
                        eprintln!(
                            "GPU LLM load failed; retrying on CPU. Original error: {e}"
                        );
                        let mut cpu_config = config;
                        cpu_config.use_gpu = false;
                        LlamaEngine::load(cpu_config)
                    }
                    Err(e) => Err(e),
                }
            }))
        })
        .map_err(|e| format!("Failed to spawn LLM loader: {e}"))?
        .join()
        .map_err(|_| "LLM loader thread panicked".to_string())?
        .map_err(|_| "LLM loader panicked during initialization".to_string())?
        .map_err(|e| format!("Failed to load LLM: {e}"))?;

    // Spawn the runner worker that will own this engine.
    let runner = LlmRunner::spawn(engine).map_err(|e| e.to_string())?;

    *state.llm_runner.lock().unwrap() = Some(Arc::new(runner));
    *state.active_llm_model_id.lock().unwrap() = Some(model_id.to_string());

    // Persist the active LLM choice so it survives restarts.
    if let Ok(mut settings) = crate::storage::settings::get_settings(&state.db) {
        settings.active_llm_model_id = Some(model_id.to_string());
        let _ = crate::storage::settings::update_settings(&state.db, &settings);
    }

    Ok(())
}
