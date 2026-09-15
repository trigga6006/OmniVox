use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, Manager, State};

use crate::llm::engine::LlamaEngine;
use crate::llm::runner::{LlmRunner, RunnerRole};
use crate::llm::types::LlmConfig;
use crate::llm_models::manager::LlmModelManager;
use crate::llm_models::types::{LlmModelInfo, LlmModelPurpose};
use crate::state::AppState;

fn require_caller(window: &tauri::WebviewWindow, expected: &str) -> Result<(), String> {
    require_caller_label(window.label(), expected)
}

fn require_caller_label(actual: &str, expected: &str) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!(
            "LLM command is not available from window '{actual}'"
        ))
    }
}

/// Pick the best downloaded LLM when Structured or Command Mode needs one
/// without an explicit active model selection.  Cleanup-purpose entries are
/// never candidates — they are single-task normalizers that cannot drive
/// grammar-constrained extraction.
pub fn preferred_downloaded_llm_id(state: &AppState) -> Option<String> {
    let models = state.llm_model_manager.list_available();
    let usable = |m: &&LlmModelInfo| m.is_downloaded && m.purpose == LlmModelPurpose::Structured;
    models
        .iter()
        .find(|m| usable(m) && m.is_default)
        .or_else(|| models.iter().find(usable))
        .map(|m| m.id.clone())
}

/// Reject activating a model for the wrong stage.
///
/// Structured Mode drives its model with GBNF-constrained JSON prompts; a
/// cleanup normalizer only understands its own documented prompt and would
/// produce garbage (the user would see it as "Structured Mode is broken").
fn require_structured_purpose(model_id: &str) -> Result<(), String> {
    match LlmModelManager::purpose(model_id) {
        LlmModelPurpose::Structured => Ok(()),
        LlmModelPurpose::Cleanup => Err(format!(
            "LLM model '{model_id}' is a cleanup model — select it under Cleanup Mode, not as the Structured Mode model"
        )),
    }
}

#[tauri::command]
pub async fn list_llm_models(state: State<'_, AppState>) -> Result<Vec<LlmModelInfo>, String> {
    Ok(state.llm_model_manager.list_available())
}

#[tauri::command]
pub async fn download_llm_model(
    model_id: String,
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&window, "main")?;
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
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
) -> Result<(), String> {
    require_caller(&window, "main")?;
    // Serialize deletion with model loading. A candidate switch is not marked
    // active until it succeeds, so checking only active_llm_model_id could
    // otherwise delete a GGUF while its loader is still using it and then let
    // that runner install. The blocking pool keeps this wait off Tokio.
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let _load_guard = LLM_LOAD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let was_active = state
            .active_llm_model_id
            .lock()
            .map(|active| active.as_deref() == Some(&model_id))
            .unwrap_or(false);
        if was_active {
            crate::commands::settings::commit_runtime_patch(
                &state,
                crate::storage::types::SettingsPatch {
                    active_llm_model_id: Some(String::new()),
                    ..Default::default()
                },
                None,
            )?;
            let old_runner = state
                .llm_runner
                .lock()
                .unwrap()
                .take()
                .map(|(_, runner)| runner);
            if let Some(runner) = old_runner {
                runner.shutdown_and_join();
            }
            *state.active_llm_model_id.lock().unwrap() = None;
        }
        // Same treatment for the cleanup slot: the worker holds the GGUF open,
        // so deleting the file underneath it would fail outright on Windows.
        let cleanup_load_guard = CLEANUP_LOAD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        if cleanup_runner_for_model(&state, &model_id).is_some()
            || configured_cleanup_model_id(&state).as_deref() == Some(model_id.as_str())
        {
            crate::commands::settings::commit_runtime_patch(
                &state,
                crate::storage::types::SettingsPatch {
                    active_cleanup_model_id: Some(String::new()),
                    ..Default::default()
                },
                None,
            )?;
            let old_runner = state
                .cleanup_runner
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take()
                .map(|(_, runner)| runner);
            if let Some(runner) = old_runner {
                runner.shutdown_and_join();
            }
        }
        drop(cleanup_load_guard);
        state
            .llm_model_manager
            .delete(&model_id)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("LLM delete task failed: {e}"))?
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
    window: tauri::WebviewWindow,
    // State is injected but re-fetched inside `spawn_blocking` (it isn't `Send`).
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&window, "main")?;
    let canonical_model_id = LlmModelManager::canonical_id(&model_id).to_string();
    require_structured_purpose(&canonical_model_id)?;
    if state
        .llm_model_manager
        .model_path(&canonical_model_id)
        .is_none()
    {
        return Err(format!(
            "LLM model '{canonical_model_id}' is not downloaded"
        ));
    }
    if state
        .llm_runner
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|(_, runner)| runner.is_busy()))
        .unwrap_or(false)
    {
        return Err(
            "Wait for the current Structured Mode extraction before switching LLM models".into(),
        );
    }
    // A user-initiated switch supersedes queued lazy/eager loads that resolved
    // the previous model selection before this command arrived.
    let load_epoch = state
        .llm_load_epoch
        .fetch_add(1, std::sync::atomic::Ordering::AcqRel)
        .wrapping_add(1);
    // Run the (mutex-guarded, blocking) GGUF load + thread-join on the blocking
    // pool so this async command never stalls a tokio worker while holding
    // `LLM_LOAD_LOCK` (M3 / B2-8).
    let app = app_handle.clone();
    let mid = canonical_model_id.clone();
    let installed = tokio::task::spawn_blocking(move || {
        let st = app.state::<AppState>();
        load_and_activate_llm_with_status_at_epoch(&mid, &st, Some(&app), load_epoch)
    })
    .await
    .map_err(|e| format!("LLM load task failed: {e}"))??;
    if installed {
        let _ = app_handle.emit("llm-model-loaded", &canonical_model_id);
    }
    Ok(())
}

/// Paste structured Markdown (from the overlay panel's Paste button) using
/// the current OutputConfig. The overlay must still have a captured target,
/// and focus restoration must succeed before any clipboard or keystroke
/// output is attempted.
#[tauri::command]
pub async fn paste_structured_output(
    markdown: String,
    binding_id: String,
    generation: u64,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&window, "overlay")?;
    validate_structured_output(&markdown)?;
    let claim = state.structured_outputs.claim(&binding_id, generation)?;
    let target = claim.target();
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    let restored = tokio::task::spawn_blocking(move || {
        crate::focus::restore_foreground_window_public(target.hwnd, target.pid)
    })
    .await
    .map_err(|e| {
        state.structured_outputs.restore(claim);
        format!("Paste target restore task failed: {e}")
    })?;
    if !restored {
        state.structured_outputs.restore(claim);
        return Err("Paste target could not be restored safely".into());
    }
    // The capability is permanently consumed once an output primitive begins:
    // a reported error can occur after partial typing/paste, so restoring here
    // would make a retry duplicate content. Only the focus handoff failures
    // above are known to occur before any output side effect.
    state
        .output
        .send_to_target(&markdown, &output_config, Some(target))
        .map_err(|e| e.to_string())
}

fn validate_structured_output(markdown: &str) -> Result<(), String> {
    if markdown.trim().is_empty() {
        Err("Structured output is empty".into())
    } else {
        Ok(())
    }
}

/// Expire a Structured Mode result when its panel is dismissed. Token and
/// generation matching prevent a stale panel from discarding a newer result.
#[tauri::command]
pub async fn discard_structured_output(
    binding_id: String,
    generation: u64,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&window, "overlay")?;
    state.structured_outputs.discard(&binding_id, generation);
    Ok(())
}

/// Lifecycle signal from the exact overlay window. The capture path uses it
/// to distinguish dictation into StructuredPanel from a normal click on the
/// always-on-top record pill.
#[tauri::command]
pub async fn set_structured_panel_active(
    active: bool,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&window, "overlay")?;
    state
        .structured_panel_active
        .store(active, std::sync::atomic::Ordering::Release);
    // The overlay is created non-activatable (see `setup_overlay_window`); the
    // panel's editor is one of two surfaces that need real keyboard focus.
    let _ = window.set_focusable(active);
    Ok(())
}

/// Dev / Settings "Test" button — runs the configured LLM on a canned
/// input (or a user-provided one) and returns the rendered Markdown.
///
/// Resolves through the KEYED single-flight loader rather than reading the slot
/// directly: a runner that parked itself after a long idle still sits in the
/// slot but can never answer, so the raw read handed the user a "reload needed"
/// error where the dictation path silently reloads.  Mirrors
/// [`llm_test_cleanup`].
#[tauri::command]
pub async fn llm_test_extract(
    text: Option<String>,
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<String, String> {
    require_caller(&window, "main")?;
    // Prefer the slot's own id so a parked runner reloads the SAME model the
    // user has been testing; fall back to the configured / preferred id when
    // nothing has been loaded this session.
    let model_id = state
        .llm_runner
        .lock()
        .ok()
        .and_then(|slot| slot.as_ref().map(|(id, _)| id.clone()))
        .or_else(|| {
            state
                .active_llm_model_id
                .lock()
                .ok()
                .and_then(|id| id.clone())
        })
        .filter(|id| !id.is_empty())
        .or_else(|| preferred_downloaded_llm_id(&state))
        .ok_or_else(|| "No LLM model loaded".to_string())?;

    // The keyed loader holds a blocking mutex across the GGUF load, so it must
    // run on the blocking pool, never on this tokio worker (B2-8).
    let load_epoch = state
        .llm_load_epoch
        .load(std::sync::atomic::Ordering::Acquire);
    let app = app_handle.clone();
    let runner = tokio::task::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ensure_runner_loaded_at_epoch(&model_id, &st, Some(&app), load_epoch)
    })
    .await
    .map_err(|e| format!("LLM load task failed: {e}"))??;

    let input = text.unwrap_or_else(|| {
        "Refactor the checkout flow in billing.tsx and cart.tsx. Keep the Stripe integration. Urgent.".to_string()
    });
    let out = runner
        .extract_with_timeout(input, std::time::Duration::from_secs(15))
        .await
        .map_err(|e| e.to_string())?;
    Ok(out.markdown)
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanupTestResult {
    pub output: String,
    pub duration_ms: u64,
}

/// Settings "Test" button for Cleanup Mode — runs the full cleanup path
/// (lazy model load included) against the configured cleanup model.
#[tauri::command]
pub async fn llm_test_cleanup(
    text: String,
    app_handle: tauri::AppHandle,
    window: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<CleanupTestResult, String> {
    require_caller(&window, "main")?;
    let model_id = configured_cleanup_model_id(&state)
        .ok_or_else(|| "No cleanup model selected — choose one under Cleanup Mode".to_string())?;
    let styling = crate::llm::prompt::cleanup_styling(
        &state
            .settings
            .read()
            .map(|s| s.values().writing_style.clone())
            .unwrap_or_default(),
    )
    .to_string();

    let app = app_handle.clone();
    let mid = model_id.clone();
    let runner = tokio::task::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ensure_cleanup_runner_loaded(&mid, &st)
    })
    .await
    .map_err(|e| format!("Cleanup load task failed: {e}"))??;

    let timeout = Duration::from_secs(
        state
            .settings
            .read()
            .map(|s| s.values().llm_timeout_secs)
            .unwrap_or(8)
            .max(1) as u64,
    );
    let started = Instant::now();
    let output = runner
        .cleanup_with_timeout(text, styling, timeout)
        .await
        .map_err(|e| e.to_string())?;
    Ok(CleanupTestResult {
        output,
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

/// The cleanup model the user selected, if any.  Unlike the Structured Mode
/// resolver there is no "pick whatever is downloaded" fallback: cleanup is an
/// explicit opt-in, and silently normalizing a transcript with a model the
/// user never chose would be a surprise.
pub fn configured_cleanup_model_id(state: &AppState) -> Option<String> {
    state
        .settings
        .read()
        .ok()
        .and_then(|s| s.values().active_cleanup_model_id.clone())
        .filter(|id| !id.is_empty())
}

/// Single-flight guard for cleanup-model loads.  Separate from
/// [`LLM_LOAD_LOCK`] so a cold cleanup load and a cold Structured Mode load do
/// not serialize behind each other; each still coalesces its own callers.
static CLEANUP_LOAD_LOCK: Mutex<()> = Mutex::new(());

fn cleanup_runner_for_model(state: &AppState, canonical_id: &str) -> Option<Arc<LlmRunner>> {
    let guard = state.cleanup_runner.lock().ok()?;
    usable_slot_lookup(guard.as_ref(), canonical_id, usable_runner)
}

/// Lazily load the cleanup model and install it in the dedicated slot,
/// coalescing concurrent callers exactly like [`ensure_runner_loaded`].
///
/// Holds a `std::sync::Mutex` across the blocking GGUF load, so it MUST run on
/// blocking infrastructure (a `spawn_blocking` task), never a tokio worker.
pub fn ensure_cleanup_runner_loaded(
    model_id: &str,
    state: &AppState,
) -> Result<Arc<LlmRunner>, String> {
    let canonical = LlmModelManager::canonical_id(model_id).to_string();
    if LlmModelManager::purpose(&canonical) != LlmModelPurpose::Cleanup {
        return Err(format!("LLM model '{canonical}' is not a cleanup model"));
    }
    if let Some(r) = cleanup_runner_for_model(state, &canonical) {
        return Ok(r);
    }
    let _load_guard = CLEANUP_LOAD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(r) = cleanup_runner_for_model(state, &canonical) {
        return Ok(r);
    }

    let model_path = state
        .llm_model_manager
        .model_path(&canonical)
        .ok_or_else(|| format!("Cleanup model '{canonical}' is not downloaded"))?;
    // Cleanup runs on CPU regardless of the shared `gpu_acceleration` toggle.
    // The s1-mini CPU path is the verified one (~300 ms/pass), and offloading it
    // would stack another ~1.1 GB of weights onto the VRAM Whisper and the
    // Structured Mode model are already competing for.  With `use_gpu` false,
    // `load_runner_on_wide_thread` skips the GPU fallback ladder entirely and
    // loads straight onto CPU.
    let config = LlmConfig::for_cleanup(model_path.to_string_lossy().into_owned(), false);

    // Release any previous cleanup runner before loading a replacement so two
    // GGUF copies never sit in RAM/VRAM at once.
    let previous = state
        .cleanup_runner
        .lock()
        .ok()
        .and_then(|mut slot| slot.take());
    if let Some((_, runner)) = previous {
        runner.shutdown_and_join();
    }

    let load_started = Instant::now();
    let (runner, backend) = load_runner_on_wide_thread(
        &canonical,
        config,
        crate::llm::profiles::get(crate::llm::profiles::DEFAULT_PROFILE_ID),
        RunnerRole::Cleanup,
    )?;
    let runner = Arc::new(runner);
    crate::diag::log(&format!(
        "cleanup llm '{canonical}': ready backend={} load_and_warm_ms={}",
        backend.label(),
        load_started.elapsed().as_millis()
    ));
    *state
        .cleanup_runner
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = Some((canonical, Arc::clone(&runner)));
    Ok(runner)
}

/// Release the cleanup worker and its model memory.  Called when the user
/// turns Cleanup Mode off; the next enable lazily reloads.
pub fn drop_cleanup_runner(state: &AppState) {
    let runner = state
        .cleanup_runner
        .lock()
        .ok()
        .and_then(|mut slot| slot.take().map(|(_, runner)| runner));
    if let Some(runner) = runner {
        drop(tauri::async_runtime::spawn_blocking(move || {
            runner.shutdown_and_join();
        }));
    }
}

/// Recent structured-mode extraction attempts (newest first) from the
/// in-memory ring buffer — powers the diagnostics panel on the Models page.
#[tauri::command]
pub async fn get_llm_diagnostics(
    window: tauri::WebviewWindow,
) -> Result<Vec<crate::llm::diaglog::ExtractionRecord>, String> {
    require_caller(&window, "main")?;
    Ok(crate::llm::diaglog::recent())
}

/// Process-wide single-flight guard for model loads.
///
/// `load_and_activate_llm` releases the `llm_runner` mutex before the (multi-
/// second) GGUF load, so two callers that both observe `llm_runner == None`
/// would otherwise each load a full copy into RAM/VRAM concurrently and race
/// to install the runner (exhaustion; last-wins).  Every load path funnels
/// through here; the lazy [`ensure_runner_loaded`] double-checks under this
/// lock so late arrivals coalesce onto the winner instead of loading again.
static LLM_LOAD_LOCK: Mutex<()> = Mutex::new(());

const PARTIAL_GPU_LAYERS: u32 = 16;

fn gpu_fallback_plan() -> [u32; 4] {
    [u32::MAX, u32::MAX, PARTIAL_GPU_LAYERS, 0]
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum LlmBackendOutcome {
    Cpu,
    GpuFull,
    GpuPartial { layers: u32 },
    CpuFallback,
}

impl LlmBackendOutcome {
    fn label(&self) -> String {
        match self {
            Self::Cpu => "cpu".into(),
            Self::GpuFull => "gpu_full".into(),
            Self::GpuPartial { layers } => format!("gpu_partial_{layers}"),
            Self::CpuFallback => "cpu_fallback".into(),
        }
    }

    fn fell_back_from_gpu(&self) -> bool {
        matches!(self, Self::GpuPartial { .. } | Self::CpuFallback)
    }
}

#[derive(Debug, Clone, Serialize)]
struct LlmLoadOutcome {
    model_id: String,
    backend: LlmBackendOutcome,
    duration_ms: u64,
}

/// Emit `llm-status` transitions around a load.  Lock-free core shared by the
/// serialized entry points below — callers MUST hold [`LLM_LOAD_LOCK`].
fn load_and_activate_llm_emit(
    model_id: &str,
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    load_epoch: u64,
) -> Result<bool, String> {
    if let Some(app) = app {
        let _ = app.emit("llm-status", "loading");
    }
    let result = load_and_activate_llm_outcome(model_id, state, load_epoch);
    if let Some(app) = app {
        match &result {
            Ok(Some(outcome)) => {
                let _ = app.emit("llm-backend-status", outcome);
                if outcome.backend.fell_back_from_gpu() {
                    let _ = app.emit("llm-gpu-fallback", outcome);
                }
                crate::gpu_env::maybe_warn_integrated_only(app);
                let _ = app.emit("llm-status", "ready");
            }
            Ok(None) => {
                let _ = app.emit("llm-status", "idle");
            }
            Err(e) => {
                let _ = app.emit("llm-status", &format!("error: {e}"));
            }
        }
    }
    result.map(|outcome| outcome.is_some())
}

/// Load a GGUF model and install it as the active LLM runner, emitting
/// `llm-status` transitions (loading → ready | error: …) when an
/// AppHandle is provided so the overlay can explain why the first
/// structured dictation is slow instead of appearing hung.
///
/// Always (re)loads the requested model — used by explicit switches and eager
/// loads.  Serializes with all other loads via [`LLM_LOAD_LOCK`] so two
/// activations never hold two GGUF copies in memory at once.  Lazy callers
/// should prefer [`ensure_runner_loaded`], which coalesces instead of
/// reloading when a runner is already present.
pub fn load_and_activate_llm_with_status(
    model_id: &str,
    state: &AppState,
    app: Option<&tauri::AppHandle>,
) -> Result<bool, String> {
    let load_epoch = state
        .llm_load_epoch
        .load(std::sync::atomic::Ordering::Acquire);
    load_and_activate_llm_with_status_at_epoch(model_id, state, app, load_epoch)
}

pub fn load_and_activate_llm_with_status_at_epoch(
    model_id: &str,
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    load_epoch: u64,
) -> Result<bool, String> {
    let _load_guard = LLM_LOAD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    load_and_activate_llm_emit(model_id, state, app, load_epoch)
}

/// Keyed lookup shared by [`runner_for_model`]: return the slot's value ONLY
/// when its stored (canonical) model id matches `canonical`.  Pure so the
/// "returns the requested model or None" invariant is unit-testable without a
/// real runner (B2-16).
fn keyed_slot_lookup<T: Clone>(slot: Option<&(String, T)>, canonical: &str) -> Option<T> {
    match slot {
        Some((id, value)) if id == canonical => Some(value.clone()),
        _ => None,
    }
}

/// The full slot-resolution rule: the value must be BOTH the requested model and
/// still usable.  Factored out of `runner_for_model` / `cleanup_runner_for_model`
/// so the pair is unit-testable without a real runner (same reason as
/// [`keyed_slot_lookup`]): a parked runner in the slot must read as ABSENT so the
/// caller falls through to its load path instead of handing back a dead worker.
fn usable_slot_lookup<T: Clone>(
    slot: Option<&(String, T)>,
    canonical: &str,
    usable: impl Fn(&T) -> bool,
) -> Option<T> {
    keyed_slot_lookup(slot, canonical).filter(|value| usable(value))
}

/// Return the loaded runner ONLY when it is the REQUESTED (canonical) model.
///
/// The single-flight double-check must not hand back whatever runner happens to
/// be installed: a lazy load for model Y that raced a load of X would otherwise
/// silently get X (M3 / B2-8).  The runner is stored TOGETHER with the canonical
/// id of the model it was loaded for under ONE mutex, so this reads a consistent
/// (model, runner) pair in a single lock — a concurrent switch can never return
/// X's runner keyed to Y (B2-16).
fn runner_for_model(state: &AppState, canonical_id: &str) -> Option<Arc<LlmRunner>> {
    let guard = state.llm_runner.lock().ok()?;
    usable_slot_lookup(guard.as_ref(), canonical_id, usable_runner)
}

/// A runner that parked itself to release its model weights after a long idle
/// still sits in the slot, but it can never serve another request.  Reporting
/// it as absent is what makes the reload transparent: both `ensure_*` functions
/// then fall through to their load path, which takes the stale runner out of
/// the slot and installs a freshly loaded one.
fn usable_runner(runner: &Arc<LlmRunner>) -> bool {
    !runner.is_parked()
}

/// Lazily ensure a runner for `model_id` is loaded, coalescing concurrent
/// callers onto a single model load (single-flight).
///
/// Returns the existing runner ONLY when it is the requested model.  When it
/// must load, it re-checks under [`LLM_LOAD_LOCK`] — a caller that lost the race
/// to load the SAME model coalesces onto the winner; a request for a DIFFERENT
/// model proceeds to (re)load rather than being fobbed off with the wrong runner.
///
/// This holds a `std::sync::Mutex` across the blocking load + thread join, so it
/// MUST be called on blocking infrastructure (a `spawn_blocking` task), never
/// directly on a tokio worker (B2-8).
pub fn ensure_runner_loaded(
    model_id: &str,
    state: &AppState,
    app: Option<&tauri::AppHandle>,
) -> Result<Arc<LlmRunner>, String> {
    let load_epoch = state
        .llm_load_epoch
        .load(std::sync::atomic::Ordering::Acquire);
    ensure_runner_loaded_at_epoch(model_id, state, app, load_epoch)
}

pub fn ensure_runner_loaded_at_epoch(
    model_id: &str,
    state: &AppState,
    app: Option<&tauri::AppHandle>,
    load_epoch: u64,
) -> Result<Arc<LlmRunner>, String> {
    let canonical = crate::llm_models::manager::LlmModelManager::canonical_id(model_id).to_string();
    if !load_epoch_matches(state, load_epoch) {
        return Err("LLM load was canceled by a settings change".into());
    }
    // Fast path — the REQUESTED model is already loaded, no lock needed.
    if let Some(r) = runner_for_model(state, &canonical) {
        return Ok(r);
    }
    // Serialize the load; the winner installs the runner and everyone waiting
    // on the SAME model coalesces onto it via the re-check below.
    let _load_guard = LLM_LOAD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if !load_epoch_matches(state, load_epoch) {
        return Err("LLM load was canceled by a settings change".into());
    }
    if let Some(r) = runner_for_model(state, &canonical) {
        return Ok(r);
    }
    if !load_and_activate_llm_emit(&canonical, state, app, load_epoch)? {
        return Err("LLM load was canceled by a settings change".into());
    }
    runner_for_model(state, &canonical).ok_or_else(|| "LLM runner missing after load".to_string())
}

/// Load a GGUF model and install it as the active LLM runner.
///
/// Runs the load on a dedicated 256 MB-stack thread with `catch_unwind` —
/// mirrors `commands::models::load_and_activate_model` because llama.cpp
/// has the same huge debug-build stack frames that crash on Windows
/// without the wider stack.
pub fn load_and_activate_llm(model_id: &str, state: &AppState) -> Result<(), String> {
    let load_epoch = state
        .llm_load_epoch
        .load(std::sync::atomic::Ordering::Acquire);
    let _load_guard = LLM_LOAD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    load_and_activate_llm_outcome(model_id, state, load_epoch).map(|_| ())
}

fn load_and_activate_llm_outcome(
    model_id: &str,
    state: &AppState,
    load_epoch: u64,
) -> Result<Option<LlmLoadOutcome>, String> {
    // Normalize IDs persisted by older versions (v0.2.9's `…-q4` entries) so
    // the canonical ID is what gets activated and written back to settings.
    let model_id = LlmModelManager::canonical_id(model_id);
    // Every structured/command load funnels through here, including the eager
    // loads driven by persisted settings — so the purpose guard belongs here
    // too, not only on the user-initiated switch.
    require_structured_purpose(model_id)?;
    let model_path = state
        .llm_model_manager
        .model_path(model_id)
        .ok_or_else(|| format!("LLM model '{model_id}' is not downloaded"))?;

    // Read the user's GPU preference — reuse the same toggle as Whisper,
    // since compile-time enabling the Vulkan/CUDA feature pulls in both
    // backends at once.
    let use_gpu = state
        .settings
        .read()
        .map(|settings| settings.values().gpu_acceleration)
        .unwrap_or(false);

    // n_ctx / max_tokens / n_threads live in LlmConfig::default() — the
    // single source of truth for engine sizing.
    let config = LlmConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        use_gpu,
        ..LlmConfig::default()
    };

    // Drop the previous runner before loading a replacement so llama.cpp does
    // not hold old and new GGUF weights in RAM/VRAM at the same time.
    if !load_epoch_matches(state, load_epoch) {
        return Ok(None);
    }

    let previous_runner = {
        let mut runner = state.llm_runner.lock().unwrap();
        if runner.as_ref().map(|(_, r)| r.is_busy()).unwrap_or(false) {
            return Err(
                "Wait for the current Structured Mode extraction before switching LLM models"
                    .into(),
            );
        }
        runner.take()
    };
    let previous_model_id = previous_runner
        .as_ref()
        .map(|(id, _)| id.clone())
        .or_else(|| {
            state
                .active_llm_model_id
                .lock()
                .ok()
                .and_then(|id| id.clone())
        });
    if let Some((_, runner)) = previous_runner {
        runner.shutdown_and_join();
    }

    let load_started = Instant::now();
    let profile = *state.active_structured_profile.lock().unwrap();

    let (runner, backend_outcome) =
        match load_runner_on_wide_thread(model_id, config, profile, RunnerRole::Structured) {
            Ok(loaded) => loaded,
            Err(load_error) => {
                return recover_failed_replacement(
                    state,
                    load_epoch,
                    previous_model_id.as_deref(),
                    use_gpu,
                    profile,
                    load_error,
                );
            }
        };

    let runner = Arc::new(runner);
    let mut slot = state.llm_runner.lock().unwrap();
    if !load_epoch_matches(state, load_epoch) {
        drop(slot);
        runner.shutdown_and_join();
        return Ok(None);
    }

    // Persist through the authoritative runtime patch API so the in-memory
    // settings snapshot and SQLite can never disagree about the active model.
    if let Err(error) = crate::commands::settings::commit_runtime_patch(
        state,
        crate::storage::types::SettingsPatch {
            active_llm_model_id: Some(model_id.to_string()),
            ..Default::default()
        },
        None,
    ) {
        drop(slot);
        runner.shutdown_and_join();
        return recover_failed_replacement(
            state,
            load_epoch,
            previous_model_id.as_deref(),
            use_gpu,
            profile,
            format!("Failed to persist active LLM: {error}"),
        );
    }

    if !load_epoch_matches(state, load_epoch) {
        drop(slot);
        runner.shutdown_and_join();
        return Ok(None);
    }

    *slot = Some((model_id.to_string(), runner));
    *state.active_llm_model_id.lock().unwrap() = Some(model_id.to_string());
    drop(slot);

    let outcome = LlmLoadOutcome {
        model_id: model_id.to_string(),
        backend: backend_outcome,
        duration_ms: load_started.elapsed().as_millis() as u64,
    };
    crate::diag::log(&format!(
        "llm '{}': ready backend={} load_and_warm_ms={}",
        outcome.model_id,
        outcome.backend.label(),
        outcome.duration_ms
    ));
    if matches!(
        outcome.backend,
        LlmBackendOutcome::GpuFull | LlmBackendOutcome::GpuPartial { .. }
    ) {
        crate::gpu_env::note_gpu_load("llm");
    }
    Ok(Some(outcome))
}

fn load_epoch_matches(state: &AppState, expected: u64) -> bool {
    load_epoch_is_current(
        state
            .llm_load_epoch
            .load(std::sync::atomic::Ordering::Acquire),
        expected,
    )
}

fn load_epoch_is_current(current: u64, expected: u64) -> bool {
    current == expected
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FailedReplacementAction {
    Cancel,
    RestorePrevious,
    LeaveUnloaded,
}

fn failed_replacement_action(epoch_matches: bool, has_previous: bool) -> FailedReplacementAction {
    match (epoch_matches, has_previous) {
        (false, _) => FailedReplacementAction::Cancel,
        (true, true) => FailedReplacementAction::RestorePrevious,
        (true, false) => FailedReplacementAction::LeaveUnloaded,
    }
}

fn recover_failed_replacement(
    state: &AppState,
    load_epoch: u64,
    previous_model_id: Option<&str>,
    use_gpu: bool,
    profile: &'static crate::llm::profiles::Profile,
    load_error: String,
) -> Result<Option<LlmLoadOutcome>, String> {
    match failed_replacement_action(
        load_epoch_matches(state, load_epoch),
        previous_model_id.is_some(),
    ) {
        FailedReplacementAction::Cancel => Ok(None),
        FailedReplacementAction::LeaveUnloaded => Err(load_error),
        FailedReplacementAction::RestorePrevious => {
            let previous_model_id = previous_model_id.expect("action requires previous model");
            let previous_path = state
                .llm_model_manager
                .model_path(previous_model_id)
                .ok_or_else(|| {
                    format!(
                        "{load_error}; previous LLM '{previous_model_id}' could not be restored because its file is missing"
                    )
                })?;
            let restore_config = LlmConfig {
                model_path: previous_path.to_string_lossy().into_owned(),
                use_gpu,
                ..LlmConfig::default()
            };
            crate::diag::log(&format!(
                "llm replacement failed; restoring previous model '{previous_model_id}'"
            ));
            let (restored, _) = load_runner_on_wide_thread(
                previous_model_id,
                restore_config,
                profile,
                RunnerRole::Structured,
            )
            .map_err(|restore_error| {
                format!(
                    "{load_error}; previous LLM '{previous_model_id}' also failed to restore: {restore_error}"
                )
            })?;
            let restored = Arc::new(restored);
            let mut slot = state.llm_runner.lock().unwrap();
            if !load_epoch_matches(state, load_epoch) {
                drop(slot);
                restored.shutdown_and_join();
                return Ok(None);
            }
            *slot = Some((previous_model_id.to_string(), restored));
            *state.active_llm_model_id.lock().unwrap() = Some(previous_model_id.to_string());
            Err(format!("{load_error}; previous LLM restored"))
        }
    }
}

fn load_runner_on_wide_thread(
    model_id: &str,
    config: LlmConfig,
    profile: &'static crate::llm::profiles::Profile,
    role: RunnerRole,
) -> Result<(LlmRunner, LlmBackendOutcome), String> {
    // Every failed candidate is freed before this helper returns, so rollback
    // never overlaps it in RAM/VRAM.
    let load_model_id = model_id.to_string();
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let plan = gpu_fallback_plan();
                let load_runner = |candidate: LlmConfig, layers: u32| {
                    let engine = LlamaEngine::load_with_gpu_layers(candidate, layers)?;
                    match role {
                        RunnerRole::Structured => LlmRunner::spawn(engine, profile),
                        RunnerRole::Cleanup => LlmRunner::spawn_cleanup(engine),
                    }
                };

                if !config.use_gpu {
                    return load_runner(config, 0).map(|runner| (runner, LlmBackendOutcome::Cpu));
                }
                if !LlamaEngine::supports_gpu_offload()? {
                    crate::diag::log(&format!(
                        "llm '{load_model_id}': GPU requested but llama backend has no offload device; using CPU"
                    ));
                    let mut cpu_config = config;
                    cpu_config.use_gpu = false;
                    return load_runner(cpu_config, 0)
                        .map(|runner| (runner, LlmBackendOutcome::CpuFallback));
                }

                match load_runner(config.clone(), plan[0]) {
                    Ok(runner) => Ok((runner, LlmBackendOutcome::GpuFull)),
                    Err(first_error) => {
                        crate::diag::log(&format!(
                            "llm '{load_model_id}': full GPU load/warm failed; retrying once: {first_error}"
                        ));
                        std::thread::sleep(Duration::from_millis(500));
                        match load_runner(config.clone(), plan[1]) {
                            Ok(runner) => {
                                crate::diag::log(&format!(
                                    "llm '{load_model_id}': GPU retry succeeded"
                                ));
                                Ok((runner, LlmBackendOutcome::GpuFull))
                            }
                            Err(retry_error) => {
                                crate::diag::log(&format!(
                                    "llm '{load_model_id}': full GPU retry failed; trying {PARTIAL_GPU_LAYERS} offloaded layers: {retry_error}"
                                ));
                                match load_runner(config.clone(), plan[2]) {
                                    Ok(runner) => Ok((
                                        runner,
                                        LlmBackendOutcome::GpuPartial {
                                            layers: PARTIAL_GPU_LAYERS,
                                        },
                                    )),
                                    Err(partial_error) => {
                                        crate::diag::log(&format!(
                                            "llm '{load_model_id}': partial GPU load/warm failed; falling back to CPU: {partial_error}"
                                        ));
                                        let mut cpu_config = config;
                                        cpu_config.use_gpu = false;
                                        load_runner(cpu_config, plan[3]).map(|runner| {
                                            (runner, LlmBackendOutcome::CpuFallback)
                                        })
                                    }
                                }
                            }
                        }
                    }
                }
            }))
        })
        .map_err(|e| format!("Failed to spawn LLM loader: {e}"))?
        .join()
        .map_err(|_| "LLM loader thread panicked".to_string())?
        .map_err(|_| "LLM loader panicked during initialization".to_string())?
        .map_err(|e| format!("Failed to load LLM: {e}"))
}

#[cfg(test)]
mod tests {
    use super::{
        failed_replacement_action, gpu_fallback_plan, keyed_slot_lookup, load_epoch_is_current,
        require_caller_label, require_structured_purpose, usable_slot_lookup,
        validate_structured_output, FailedReplacementAction,
    };

    /// A cleanup normalizer must never become the Structured Mode extractor —
    /// it cannot follow the JSON prompt and the user would read the garbage as
    /// "Structured Mode is broken".
    #[test]
    fn cleanup_models_cannot_be_activated_for_structured_mode() {
        let error = require_structured_purpose("s1-mini-0.6b-q4")
            .expect_err("cleanup model must be rejected");
        assert!(error.contains("cleanup model"), "unexpected error: {error}");
        assert!(require_structured_purpose("qwen3-1.7b-instruct-q8").is_ok());
        // Legacy ids canonicalize before the check.
        assert!(require_structured_purpose("qwen3-1.7b-instruct-q4").is_ok());
    }

    /// The keyed read `runner_for_model` uses must return the value ONLY for the
    /// requested model — a concurrent switch that installed model Y's runner
    /// must never satisfy a request for X (B2-16).
    #[test]
    fn keyed_slot_lookup_returns_requested_or_none() {
        let slot = ("qwen-x".to_string(), 7i32);
        assert_eq!(keyed_slot_lookup(Some(&slot), "qwen-x"), Some(7));
        // Slot now holds model Y's runner (the switch replaced it) — a request
        // for X gets None, never Y's runner mislabeled as X.
        let slot_y = ("qwen-y".to_string(), 9i32);
        assert_eq!(keyed_slot_lookup(Some(&slot_y), "qwen-x"), None);
        // Empty slot (mid-load) returns None.
        assert_eq!(keyed_slot_lookup::<i32>(None, "qwen-x"), None);
    }

    /// A runner that parked itself to release its weights still sits in the
    /// slot, but it can never answer.  Every resolution path (dictation, Command
    /// Mode, the Settings Test buttons) must read it as "no model loaded" so it
    /// falls through to the reload instead of returning a dead worker.
    #[test]
    fn a_parked_runner_in_the_slot_reads_as_absent() {
        // `bool` stands in for the runner: true = parked. The production
        // predicate is `usable_runner`, i.e. `!is_parked()`.
        let usable = |parked: &bool| !*parked;

        let parked = ("qwen-x".to_string(), true);
        assert_eq!(usable_slot_lookup(Some(&parked), "qwen-x", usable), None);

        let live = ("qwen-x".to_string(), false);
        assert_eq!(
            usable_slot_lookup(Some(&live), "qwen-x", usable),
            Some(false)
        );
        // The keyed check still applies on top of usability.
        assert_eq!(usable_slot_lookup(Some(&live), "qwen-y", usable), None);
        assert_eq!(usable_slot_lookup::<bool>(None, "qwen-x", usable), None);
    }

    #[test]
    fn gpu_fallback_plan_tries_partial_offload_before_cpu() {
        let plan = gpu_fallback_plan();
        assert_eq!(plan[0], u32::MAX);
        assert_eq!(plan[1], plan[0]);
        assert!((1..plan[1]).contains(&plan[2]));
        assert_eq!(plan[3], 0);
    }

    #[test]
    fn ipc_caller_labels_are_exact() {
        assert!(require_caller_label("main", "main").is_ok());
        assert!(require_caller_label("overlay", "overlay").is_ok());
        assert!(require_caller_label("overlay", "main").is_err());
        assert!(require_caller_label("main", "overlay").is_err());
    }

    #[test]
    fn failed_replacement_transition_is_epoch_and_previous_aware() {
        assert_eq!(
            failed_replacement_action(false, true),
            FailedReplacementAction::Cancel
        );
        assert_eq!(
            failed_replacement_action(true, true),
            FailedReplacementAction::RestorePrevious
        );
        assert_eq!(
            failed_replacement_action(true, false),
            FailedReplacementAction::LeaveUnloaded
        );
    }

    #[test]
    fn only_the_current_load_epoch_may_install() {
        assert!(load_epoch_is_current(7, 7));
        assert!(!load_epoch_is_current(8, 7));
        assert!(!load_epoch_is_current(6, 7));
    }

    #[test]
    fn empty_structured_output_is_rejected_before_capability_claim() {
        assert!(validate_structured_output("").is_err());
        assert!(validate_structured_output(" \r\n\t ").is_err());
        assert!(validate_structured_output("# Note").is_ok());
    }
}
