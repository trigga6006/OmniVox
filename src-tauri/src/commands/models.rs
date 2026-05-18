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
            if let Ok(mut settings) = crate::storage::settings::get_settings(&state.db) {
                if settings.active_model_id.as_deref() == Some(&model_id) {
                    settings.active_model_id = None;
                    let _ = crate::storage::settings::update_settings(&state.db, &settings);
                }
            }
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

    // Reserve 1–2 cores for the OS, audio capture thread, and UI rendering.
    // available_parallelism() returns logical cores (including hyperthreads),
    // so on a 4-core/8-thread laptop this gives 6 threads — enough for Whisper
    // without starving the system. Clamped to [2, 8] for safety.
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).max(2).min(8) as u32)
        .unwrap_or(4);

    // Read the GPU acceleration preference from persisted settings.
    let use_gpu = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.gpu_acceleration)
        .unwrap_or(false);

    // English-only models (.en suffix) force English; multilingual models
    // use auto-detection so users can dictate in any language.
    let is_multilingual = is_model_multilingual(model_id);
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

    // Drop the previous model before loading the replacement. Loading GGML
    // weights can use multiple GB, so keeping old + new resident at once can
    // OOM smaller GPUs or 16 GB systems during a model switch.
    {
        let audio = state.audio.lock().map_err(|_| "Audio state lock poisoned".to_string())?;
        if audio.is_recording() {
            return Err("Stop recording before switching Whisper models".into());
        }
        *state.engine.lock().unwrap() = None;
    }
    *state.active_model_id.lock().unwrap() = None;

    // Load on a thread with a larger stack — whisper.cpp + GGML backends
    // need extra stack space, especially in debug builds on Windows.
    let engine = std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024) // 256 MB — debug builds have much larger stack frames (especially with llama.cpp + whisper.cpp)
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match WhisperEngine::load(config.clone()) {
                    Ok(engine) => Ok(engine),
                    Err(e) if config.use_gpu => {
                        eprintln!(
                            "GPU Whisper load failed; retrying on CPU. Original error: {e}"
                        );
                        let mut cpu_config = config;
                        cpu_config.use_gpu = false;
                        WhisperEngine::load(cpu_config)
                    }
                    Err(e) => Err(e),
                }
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

/// Check whether a Whisper model supports multiple languages.
///
/// English-only models have an `-en` suffix or end with `-q5`.
/// Everything else (medium, large, turbo variants) is multilingual.
pub(crate) fn is_model_multilingual(model_id: &str) -> bool {
    model_id == "whisper-medium"
        || model_id == "whisper-large-v3-turbo-multi"
        || model_id == "whisper-distil-large-v3"
        || model_id == "whisper-large"
        || (!model_id.contains("-en") && !model_id.ends_with("-q5"))
}

/// Hard cap on the initial prompt length.
///
/// Whisper's prompt budget is 224 tokens (~4 chars/token for English, but 1
/// char/token for rare words).  800 chars keeps us safely under the limit so
/// we never get silently truncated mid-term, and so early user-priority terms
/// never get dropped because a mountain of static vocab filled the budget.
const PROMPT_BUDGET: usize = 800;

/// Budget for end-of-prompt reinforcement (repeated vocab words in sentence
/// form — leverages Whisper's recency bias).
const REINFORCEMENT_BUDGET: usize = 200;

/// Collect every vocabulary word the user has enabled, global + active mode.
fn collect_vocab_words(state: &AppState, active_mode: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = crate::storage::vocabulary::list_entries(&state.db) {
        out.extend(
            entries
                .into_iter()
                .filter(|e| e.is_enabled && !e.word.is_empty())
                .map(|e| e.word),
        );
    }
    if let Some(mode_id) = active_mode {
        if let Ok(entries) = crate::storage::vocabulary::list_entries_for_mode(&state.db, mode_id) {
            out.extend(
                entries
                    .into_iter()
                    .filter(|e| e.is_enabled && !e.word.is_empty())
                    .map(|e| e.word),
            );
        }
    }
    out
}

/// Collect every dictionary replacement value the user has enabled, global +
/// active mode.
fn collect_dict_replacements(state: &AppState, active_mode: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = crate::storage::dictionary::list_entries(&state.db) {
        out.extend(
            entries
                .into_iter()
                .filter(|e| e.is_enabled && !e.replacement.is_empty())
                .map(|e| e.replacement),
        );
    }
    if let Some(mode_id) = active_mode {
        if let Ok(entries) = crate::storage::dictionary::list_entries_for_mode(&state.db, mode_id) {
            out.extend(
                entries
                    .into_iter()
                    .filter(|e| e.is_enabled && !e.replacement.is_empty())
                    .map(|e| e.replacement),
            );
        }
    }
    out
}

/// Build a Whisper initial prompt from static vocabulary + dictionary entries.
///
/// - English models get the full English vocab to bias toward correct
///   recognition of technical terms and proper nouns.
/// - Multilingual models get only universal abbreviations + user dictionary
///   terms, so the language detector works unbiased for non-English speech.
///
/// Improvements over the previous version:
///  - **Case-insensitive dedup** via HashSet instead of `Vec::dedup()` (which
///    only dropped consecutive duplicates and left most overlap behind).
///  - **Single lock acquisition** on `active_context_mode_id` (was locked
///    twice — once for term collection, once for reinforcement collection).
///  - **Vocabulary collected once**, reused for both term list and
///    reinforcement (was collected twice).
///  - **Budget cap** at 800 chars so we never exceed Whisper's 224-token
///    prompt limit.  Priority: user vocab first, then dictionary, then
///    static vocab — when we hit the budget, static vocab (lowest priority)
///    is what gets truncated.
///  - **Reinforcement appended once**, capped at 200 chars.  The previous
///    code appended it twice, which on users with large vocab doubled the
///    reinforcement and pushed everything else past the truncation limit.
pub(crate) fn build_whisper_vocab_prompt(state: &AppState, is_multilingual: bool) -> Option<String> {
    use std::collections::HashSet;

    // Snapshot active mode once so we don't hold the lock during DB queries.
    let active_mode: Option<String> = state
        .active_context_mode_id
        .lock()
        .ok()
        .and_then(|g| g.clone());
    let active_mode_ref = active_mode.as_deref();

    let vocab_words = collect_vocab_words(state, active_mode_ref);
    let dict_terms = collect_dict_replacements(state, active_mode_ref);

    let static_vocab = if is_multilingual { MULTILINGUAL_VOCAB } else { ENGLISH_VOCAB };

    // Dedup case-insensitively, preserving insertion order.  Priority order:
    // user vocab (first — highest bias value) → user dictionary →
    // static vocab (lowest — gets truncated first if budget is tight).
    let mut seen: HashSet<String> = HashSet::new();
    let mut ordered_terms: Vec<String> = Vec::new();
    let sources: [&[String]; 2] = [&vocab_words, &dict_terms];
    for src in sources.iter() {
        for term in src.iter() {
            if seen.insert(term.to_lowercase()) {
                ordered_terms.push(term.clone());
            }
        }
    }
    for term in static_vocab.split(", ") {
        let trimmed = term.trim();
        if trimmed.is_empty() { continue; }
        if seen.insert(trimmed.to_lowercase()) {
            ordered_terms.push(trimmed.to_string());
        }
    }

    if ordered_terms.is_empty() && vocab_words.is_empty() {
        return None;
    }

    // Build reinforcement first so we can budget the term list around it.
    // End-of-prompt repetition gives the decoder the strongest signal.
    let mut reinforcement = String::new();
    for w in &vocab_words {
        let chunk_len = w.len() + 2; // "w. "
        if reinforcement.len() + chunk_len > REINFORCEMENT_BUDGET {
            break;
        }
        reinforcement.push_str(w);
        reinforcement.push_str(". ");
    }
    let reinforcement = reinforcement.trim_end();

    // Term-list budget = total - ". " separator - reinforcement.
    let suffix_len = if reinforcement.is_empty() { 0 } else { 2 + reinforcement.len() };
    let term_budget = PROMPT_BUDGET.saturating_sub(suffix_len);

    // Fill the term list up to the budget, stopping at the last term that
    // fully fits (no mid-term truncation, which would poison Whisper's
    // tokenization of the following term).
    let mut prompt = String::new();
    for term in &ordered_terms {
        let projected = if prompt.is_empty() {
            term.len()
        } else {
            prompt.len() + 2 + term.len()
        };
        if projected > term_budget {
            break;
        }
        if !prompt.is_empty() {
            prompt.push_str(", ");
        }
        prompt.push_str(term);
    }

    if !reinforcement.is_empty() {
        if !prompt.is_empty() {
            prompt.push_str(". ");
        }
        prompt.push_str(reinforcement);
    }

    if prompt.is_empty() { None } else { Some(prompt) }
}
