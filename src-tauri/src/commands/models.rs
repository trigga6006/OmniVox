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
    // Invalidate cache so the next list_models call picks up the new file
    state.model_manager.invalidate_cache();
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

    // English-only models (.en suffix) force English; multilingual models
    // use auto-detection so users can dictate in any language.
    let is_multilingual = model_id == "whisper-medium"
        || model_id == "whisper-large-v3-turbo-multi"
        || model_id == "whisper-distil-large-v3"
        || model_id == "whisper-large"
        || (!model_id.contains("-en") && !model_id.ends_with("-q5"));
    let language = if is_multilingual { None } else { Some("en".into()) };

    // Build an initial prompt to bias Whisper's decoder.
    // English models get a rich English vocabulary prompt.
    // Multilingual models get only user dictionary terms (no English bias)
    // so the language detector works unbiased.
    let initial_prompt = build_whisper_vocab_prompt(state, is_multilingual);

    let config = AsrConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        language,
        translate: false,
        n_threads,
        use_gpu,
        initial_prompt,
        beam_size: None,       // default: 5 (beam search)
        temperature: None,     // default: 0.0 (deterministic)
        temperature_inc: None, // default: 0.2 (fallback on low confidence)
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

    // Persist the active model choice so it survives restarts
    if let Ok(mut settings) = crate::storage::settings::get_settings(&state.db) {
        settings.active_model_id = Some(model_id.to_string());
        let _ = crate::storage::settings::update_settings(&state.db, &settings);
    }

    Ok(())
}

/// English vocabulary prompt — biases the decoder toward correct recognition
/// of common English technical terms, abbreviations, and proper nouns.
/// These are terms Whisper frequently mishears without prompting.
const ENGLISH_VOCAB: &str = "\
AI, API, URL, HTTP, HTTPS, JSON, CSS, HTML, XML, YAML, TOML, \
JavaScript, TypeScript, Python, Rust, Go, Ruby, Java, C++, C#, Swift, Kotlin, PHP, \
GitHub, GitLab, VS Code, ChatGPT, GPT, LLM, OpenAI, Anthropic, Claude, \
CLI, SQL, NoSQL, REST, GraphQL, OAuth, JWT, SSH, TLS, SSL, DNS, TCP, UDP, \
UI, UX, RAM, CPU, GPU, SSD, NVMe, USB, HDMI, WiFi, Bluetooth, \
PDF, PNG, JPEG, SVG, GIF, MP3, MP4, WebM, \
AWS, Azure, GCP, Docker, Kubernetes, Linux, Ubuntu, macOS, Windows, \
npm, pip, cargo, brew, apt, git, curl, wget, \
React, Vue, Angular, Next.js, Node.js, Express, Django, Flask, FastAPI, \
MongoDB, PostgreSQL, MySQL, Redis, SQLite, Elasticsearch, \
Terraform, Ansible, Jenkins, CircleCI, Webpack, Vite, ESLint, Prettier, \
OmniVox, Whisper, GGML, Vulkan, CUDA";

/// Multilingual vocabulary prompt — language-neutral terms only.
/// Uses universal abbreviations and brand names that are the same across
/// all languages.  Deliberately avoids English-specific words so the
/// language detector runs unbiased.
const MULTILINGUAL_VOCAB: &str = "\
AI, API, URL, HTTP, HTTPS, JSON, CSS, HTML, XML, PDF, USB, WiFi, Bluetooth, \
GPU, CPU, RAM, SSD, DNS, SSH, SSL, TLS, \
GitHub, ChatGPT, GPT, OpenAI, Google, Microsoft, Apple, Amazon, \
Docker, Linux, Windows, macOS, Android, iOS, \
OmniVox, Whisper";

/// Build a Whisper initial prompt from static vocabulary + dictionary entries.
///
/// - English models get the full English vocab to bias toward correct
///   recognition of technical terms and proper nouns.
/// - Multilingual models get only universal abbreviations + user dictionary
///   terms, so the language detector works unbiased for non-English speech.
fn build_whisper_vocab_prompt(state: &AppState, is_multilingual: bool) -> Option<String> {
    let mut terms: Vec<String> = Vec::new();

    // Start with the appropriate static vocabulary
    let vocab = if is_multilingual { MULTILINGUAL_VOCAB } else { ENGLISH_VOCAB };
    for term in vocab.split(", ") {
        let trimmed = term.trim();
        if !trimmed.is_empty() {
            terms.push(trimmed.to_string());
        }
    }

    // Global dictionary entries (user-defined, applies to all languages)
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
