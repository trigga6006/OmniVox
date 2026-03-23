use std::sync::Arc;

use tauri::State;

use crate::asr::engine::WhisperEngine;
use crate::asr::types::AsrConfig;
use crate::models::manager::ModelManager;
use crate::models::types::{HardwareInfo, ModelInfo};
use crate::state::AppState;

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    Ok(state.model_manager.list_available())
}

#[tauri::command]
pub async fn download_model(
    model_id: String,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .downloader
        .download(&model_id, &app_handle)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn delete_model(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // If this is the active model, unload it first
    {
        let active = state.active_model_id.lock().unwrap();
        if active.as_deref() == Some(&model_id) {
            drop(active);
            *state.engine.lock().unwrap() = None;
            *state.active_model_id.lock().unwrap() = None;
        }
    }

    state
        .model_manager
        .delete(&model_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_active_model(state: State<'_, AppState>) -> Result<Option<ModelInfo>, String> {
    let active_id = state.active_model_id.lock().unwrap().clone();
    match active_id {
        Some(id) => Ok(state.model_manager.get_model(&id)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn set_active_model(
    model_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    load_and_activate_model(&model_id, &state)
}

/// Returns whether the binary was compiled with GPU (Vulkan/CUDA) support.
/// The frontend uses this to show or hide the GPU toggle in Settings.
#[tauri::command]
pub async fn get_gpu_support() -> Result<bool, String> {
    // whisper-rs sets the internal `_gpu` feature when `cuda` or `vulkan` is enabled.
    // We mirror that with our own feature flags.
    Ok(cfg!(any(feature = "vulkan", feature = "cuda")))
}

#[tauri::command]
pub async fn get_hardware_info() -> Result<HardwareInfo, String> {
    let cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4);

    let recommended = ModelManager::recommend_for_cores(cpu_cores);

    Ok(HardwareInfo {
        cpu_name: "Unknown CPU".into(),
        cpu_cores,
        ram_total_mb: 0,
        gpu_name: None,
        gpu_vram_mb: None,
        recommended_model: recommended.into(),
    })
}

/// Shared logic: verify model exists on disk, load Whisper engine, set as active.
/// Used by both `set_active_model` command and the first-launch setup.
pub fn load_and_activate_model(
    model_id: &str,
    state: &AppState,
) -> Result<(), String> {
    let model_path = state
        .model_manager
        .model_path(model_id)
        .ok_or_else(|| format!("Model '{}' is not downloaded", model_id))?;

    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or(4)
        .min(8);

    // Read the GPU acceleration preference from persisted settings.
    let use_gpu = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.gpu_acceleration)
        .unwrap_or(false);

    // Build an initial prompt from dictionary entries to bias Whisper toward
    // recognizing domain-specific vocabulary on the first pass.  We collect
    // the "replacement" values (the correct forms) from both global and
    // active-mode dictionaries.
    let initial_prompt = build_whisper_vocab_prompt(state);

    let config = AsrConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        language: Some("en".into()),
        translate: false,
        n_threads,
        use_gpu,
        initial_prompt,
    };

    // Load on a thread with a larger stack — whisper.cpp + GGML backends
    // need extra stack space, especially in debug builds on Windows.
    let engine = std::thread::Builder::new()
        .stack_size(128 * 1024 * 1024) // 128 MB — debug builds have much larger stack frames
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                WhisperEngine::load(config)
            }))
        })
        .map_err(|e| format!("Failed to spawn model loader: {e}"))?
        .join()
        .map_err(|_| "Model loader thread panicked".to_string())?
        .map_err(|_| "Model loader panicked during initialization".to_string())?
        .map_err(|e| format!("Failed to load model: {e}"))?;

    *state.engine.lock().unwrap() = Some(Arc::new(engine));
    *state.active_model_id.lock().unwrap() = Some(model_id.to_string());

    Ok(())
}

/// Build a Whisper initial prompt from dictionary replacement values.
///
/// Collects all enabled dictionary entries (global + active context mode)
/// and joins their replacement forms into a comma-separated string.
/// This biases Whisper toward recognizing domain-specific terms on the
/// first transcription pass, reducing reliance on post-processing fixes.
fn build_whisper_vocab_prompt(state: &AppState) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();

    // Global dictionary entries
    if let Ok(entries) = crate::storage::dictionary::list_entries(&state.db) {
        for entry in &entries {
            if entry.is_enabled && !entry.replacement.is_empty() {
                terms.push(entry.replacement.clone());
            }
        }
    }

    // Active mode's dictionary entries
    if let Ok(guard) = state.active_context_mode_id.lock() {
        if let Some(ref mode_id) = *guard {
            if let Ok(entries) = crate::storage::dictionary::list_entries_for_mode(&state.db, mode_id) {
                for entry in &entries {
                    if entry.is_enabled && !entry.replacement.is_empty() {
                        terms.push(entry.replacement.clone());
                    }
                }
            }
        }
    }

    if terms.is_empty() {
        None
    } else {
        terms.dedup();
        Some(terms.join(", "))
    }
}
