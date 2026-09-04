use std::sync::{Arc, Mutex};

use tauri::{Manager, State, WebviewWindow};

use crate::asr::engine::WhisperEngine;
use crate::asr::parakeet::ParakeetEngine;
use crate::asr::types::AsrConfig;
use crate::asr::LoadedAsrEngine;
use crate::models::manager::ModelManager;
use crate::models::types::{
    AsrCapabilityTier, ComputeBackend, HardwareInfo, HardwareProfile, ModelFamily, ModelInfo,
};
use crate::state::AppState;

/// Process-wide guard for the multi-gigabyte ASR load/warmup sequence. Every
/// entry point (startup, UI model switch, GPU toggle) passes through
/// `load_and_activate_model` for BOTH engine families, so concurrent requests
/// cannot duplicate weights.
static WHISPER_LOAD_LOCK: Mutex<()> = Mutex::new(());

#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>, String> {
    let (profile, _) = collect_hardware_profile(Some(&state));
    Ok(state.model_manager.list_available_for_profile(&profile))
}

#[tauri::command]
pub async fn download_model(
    model_id: String,
    app_handle: tauri::AppHandle,
    caller: WebviewWindow,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_main_window(&caller)?;
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
    app_handle: tauri::AppHandle,
    caller: WebviewWindow,
) -> Result<(), String> {
    require_main_window(&caller)?;
    tokio::task::spawn_blocking(move || {
        let state = app_handle.state::<AppState>();
        let _load_guard = WHISPER_LOAD_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let model_id = ModelManager::canonical_id(&model_id).to_string();
        let capture = state
            .capture
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if capture.is_some() {
            return Err("Stop recording before deleting a speech model".to_string());
        }
        let _transition = state
            .asr_model_transition
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let is_active = state
            .active_model_id
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_deref()
            .is_some_and(|active| ModelManager::canonical_id(active) == model_id.as_str());
        if is_active {
            crate::pipeline::reset_final_asr_worker_blocking();
            clear_active_whisper(&state)?;
        }
        state
            .model_manager
            .delete(&model_id)
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|error| format!("Model delete task failed: {error}"))?
}

#[tauri::command]
pub async fn get_active_model(state: State<'_, AppState>) -> Result<Option<ModelInfo>, String> {
    let _transition = state
        .asr_model_transition
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let active_id = state.active_model_id.lock().unwrap().clone();
    match active_id {
        Some(id) => Ok(state.model_manager.get_model(&id)),
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn set_active_model(
    model_id: String,
    app_handle: tauri::AppHandle,
    caller: WebviewWindow,
) -> Result<(), String> {
    require_main_window(&caller)?;
    // The native loader spawns a large-stack thread and joins it. Keep that
    // multi-second wait off Tokio's async workers.
    let app = app_handle.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        let state = app.state::<AppState>();
        load_and_activate_model(&model_id, &state)
    })
    .await
    .map_err(|error| format!("Whisper model load task failed: {error}"))??;
    outcome.emit_gpu_fallback(&app_handle);
    Ok(())
}

fn require_main_window(caller: &WebviewWindow) -> Result<(), String> {
    if caller.label() == "main" {
        Ok(())
    } else {
        Err("This model operation is only available from the main window".into())
    }
}

/// What actually happened during a model load — callers use this to surface
/// a "running on CPU" warning instead of letting the fallback stay silent.
pub struct ModelLoadOutcome {
    /// GPU was requested but failed (even after a retry); engine is on CPU.
    pub gpu_fallback: bool,
}

impl ModelLoadOutcome {
    pub fn emit_gpu_fallback(&self, app_handle: &tauri::AppHandle) {
        use tauri::Emitter;
        if self.gpu_fallback {
            let _ = app_handle.emit(
                "whisper-gpu-fallback",
                "GPU unavailable — Whisper is running on CPU (slower). Re-select your model in Models to retry.",
            );
        } else {
            // Load stayed on its requested backend; if that backend is an
            // integrated-only Vulkan, say so (once) — every successful load
            // funnels through here with an AppHandle.
            crate::gpu_env::maybe_warn_integrated_only(app_handle);
        }
    }
}

fn load_whisper_on_native_thread(
    load_model_id: String,
    config: AsrConfig,
) -> Result<(WhisperEngine, bool), String> {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                match WhisperEngine::load(config.clone()) {
                    Ok(engine) => Ok((engine, false)),
                    Err(error) if config.use_gpu => {
                        crate::diag::log(&format!(
                            "whisper '{load_model_id}': GPU load failed, retrying GPU in 1.5s: {error}"
                        ));
                        std::thread::sleep(std::time::Duration::from_millis(1500));
                        match WhisperEngine::load(config.clone()) {
                            Ok(engine) => {
                                crate::diag::log(&format!(
                                    "whisper '{load_model_id}': GPU retry succeeded"
                                ));
                                Ok((engine, false))
                            }
                            Err(retry_error) => {
                                crate::diag::log(&format!(
                                    "whisper '{load_model_id}': GPU retry failed, falling back to CPU: {retry_error}"
                                ));
                                let mut cpu_config = config;
                                cpu_config.use_gpu = false;
                                cpu_config.beam_size = Some(2);
                                WhisperEngine::load(cpu_config).map(|engine| (engine, true))
                            }
                        }
                    }
                    Err(error) => Err(error),
                }
            }))
        })
        .map_err(|error| format!("Failed to spawn model loader: {error}"))?
        .join()
        .map_err(|_| "Model loader thread panicked".to_string())?
        .map_err(|_| "Model loader panicked during initialization".to_string())?
        .map_err(|error| format!("Failed to load model: {error}"))
}

/// Load Parakeet's ONNX sessions off the async runtime.
///
/// No GPU ladder: the model is an int8 CPU export, so `settings.gpu_acceleration`
/// does not apply and there is nothing to retry or fall back from. It still runs
/// on the large-stack native thread that Whisper uses, because the loader is
/// called from the same blocking context.
fn load_parakeet_on_native_thread(
    load_model_id: String,
    model_dir: std::path::PathBuf,
    n_threads: u32,
) -> Result<ParakeetEngine, String> {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                ParakeetEngine::load(&model_dir, n_threads)
            }))
        })
        .map_err(|error| format!("Failed to spawn model loader: {error}"))?
        .join()
        .map_err(|_| "Model loader thread panicked".to_string())?
        .map_err(|_| "Model loader panicked during initialization".to_string())?
        .map_err(|error| format!("Failed to load model '{load_model_id}': {error}"))
}

/// Decode configuration for a Whisper catalog entry. Shared by the primary
/// load, the rollback load, and the restore path so their language, prompt, and
/// beam settings cannot drift apart.
fn build_whisper_config(
    state: &AppState,
    model_id: &str,
    model_path: &std::path::Path,
    n_threads: u32,
    use_gpu: bool,
) -> AsrConfig {
    // English-only models (.en suffix) force English; multilingual models
    // use auto-detection so users can dictate in any language.
    let is_multilingual = is_model_multilingual(model_id);
    AsrConfig {
        model_path: model_path.to_string_lossy().into_owned(),
        language: (!is_multilingual).then(|| "en".into()),
        translate: false,
        n_threads,
        use_gpu,
        // Build an initial prompt to bias Whisper's decoder. English models get
        // a rich English vocabulary prompt; multilingual models get only user
        // dictionary terms (no English bias) so language detection is unbiased.
        initial_prompt: build_whisper_vocab_prompt(state, is_multilingual),
        // GPU: full 5-way beam (accuracy). CPU: beam 2 — beam search runs the
        // decoder ~beam_size× per step and dominates CPU decode latency, so on
        // CPU we trade a sliver of accuracy for noticeably snappier push-to-talk.
        beam_size: Some(if use_gpu { 5 } else { 2 }),
        temperature: None,     // default: 0.0 (deterministic)
        temperature_inc: None, // default: 0.2 (fallback on low confidence)
    }
}

/// Load whichever engine family the catalog assigns to `model_id`.
/// Returns the loaded engine and whether a GPU request fell back to CPU
/// (always false for Parakeet, which never requests a GPU).
fn load_asr_on_native_thread(
    state: &AppState,
    model_id: &str,
    model_path: &std::path::Path,
    n_threads: u32,
    use_gpu: bool,
) -> Result<(LoadedAsrEngine, bool), String> {
    match ModelManager::family(model_id) {
        Some(ModelFamily::Parakeet) => {
            let engine = load_parakeet_on_native_thread(
                model_id.to_string(),
                model_path.to_path_buf(),
                n_threads,
            )?;
            Ok((LoadedAsrEngine::Parakeet(Arc::new(engine)), false))
        }
        _ => {
            let config = build_whisper_config(state, model_id, model_path, n_threads, use_gpu);
            let (engine, gpu_fallback) =
                load_whisper_on_native_thread(model_id.to_string(), config)?;
            Ok((LoadedAsrEngine::Whisper(Arc::new(engine)), gpu_fallback))
        }
    }
}

fn restore_previous_engine(
    state: &AppState,
    previous_model_id: Option<&str>,
    previous_uses_gpu: Option<bool>,
    n_threads: u32,
) -> Result<String, String> {
    let previous_id = previous_model_id.ok_or_else(|| "no previous model".to_string())?;
    let previous_path = state
        .model_manager
        .model_path(previous_id)
        .ok_or_else(|| format!("previous model '{previous_id}' is unavailable"))?;
    let (engine, _) = load_asr_on_native_thread(
        state,
        previous_id,
        &previous_path,
        n_threads,
        previous_uses_gpu.unwrap_or(false),
    )?;
    *state.engine.lock().unwrap() = Some(engine);
    *state.active_model_id.lock().unwrap() = Some(previous_id.to_string());
    debug_assert!(model_slots_are_coherent(Some(previous_id), true));
    Ok(previous_id.to_string())
}

fn clear_active_whisper(state: &AppState) -> Result<(), String> {
    // Persist first. If SQLite rejects the update, callers that still have a
    // working engine (notably active-model deletion) keep that engine and its
    // in-memory ID instead of being left unloaded by a failed settings write.
    crate::commands::settings::commit_runtime_patch(
        state,
        crate::storage::types::SettingsPatch {
            active_model_id: Some(String::new()),
            ..Default::default()
        },
        None,
    )?;
    *state.engine.lock().unwrap() = None;
    *state.active_model_id.lock().unwrap() = None;
    debug_assert!(model_slots_are_coherent(None, false));
    Ok(())
}

fn model_slots_are_coherent(active_id: Option<&str>, engine_present: bool) -> bool {
    active_id.is_some() == engine_present
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
pub async fn get_hardware_info(state: State<'_, AppState>) -> Result<HardwareInfo, String> {
    Ok(collect_hardware_profile(Some(&state)).1)
}

/// Collect the facts that are reliable across the platforms sysinfo supports.
///
/// We deliberately do not use Windows' legacy `AdapterRAM` value: it is a
/// 32-bit field and truncates modern cards (a 16 GB GPU is commonly reported
/// as 4 GB). A backend is marked usable only after an engine has successfully
/// loaded on it, and unknown VRAM never upgrades the recommendation by itself.
fn collect_hardware_profile(state: Option<&AppState>) -> (HardwareProfile, HardwareInfo) {
    // Do not enumerate processes just to render the Models page. CPU identity
    // and total RAM are static facts, and a narrow refresh avoids a noticeable
    // double scan because the frontend requests model + hardware data together.
    let system = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing()
            .with_cpu(sysinfo::CpuRefreshKind::nothing())
            .with_memory(sysinfo::MemoryRefreshKind::nothing().with_ram()),
    );
    let logical_cpu_cores = std::thread::available_parallelism()
        .map(|n| n.get() as u32)
        .unwrap_or_else(|_| system.cpus().len().try_into().unwrap_or(4));
    let physical_cpu_cores = sysinfo::System::physical_core_count().map(|cores| cores as u32);
    let ram_total_mb = system.total_memory() / (1024 * 1024);
    let cpu_name = system
        .cpus()
        .first()
        .map(|cpu| cpu.brand().trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("Unknown CPU")
        .to_string();

    let active_uses_gpu = state.and_then(|state| {
        let _transition = state.asr_model_transition.lock().ok()?;
        state
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.as_ref().map(|engine| engine.uses_gpu()))
    });
    let compute_backend = match active_uses_gpu {
        Some(false) => ComputeBackend::Cpu,
        Some(true) => {
            #[cfg(feature = "cuda")]
            {
                ComputeBackend::Cuda
            }
            #[cfg(all(not(feature = "cuda"), feature = "vulkan"))]
            {
                ComputeBackend::Vulkan
            }
            #[cfg(not(any(feature = "cuda", feature = "vulkan")))]
            {
                ComputeBackend::Unknown
            }
        }
        None => ComputeBackend::Unknown,
    };
    // The RTF thresholds below are calibrated against whisper.cpp decode cost.
    // Parakeet's transducer has a completely different real-time factor, so
    // `measured_asr_tier_for_backend` only samples traces tagged with the
    // Whisper family — see its trace filter.
    let measured_asr_tier = measured_asr_tier_for_backend(compute_backend);

    let profile = HardwareProfile {
        physical_cpu_cores,
        logical_cpu_cores: Some(logical_cpu_cores),
        ram_total_mb: Some(ram_total_mb),
        gpu_name: None,
        gpu_vram_mb: None,
        compute_backend,
        measured_asr_tier,
    };
    let recommended_model = ModelManager::recommend_for_profile(&profile).into();
    let info = HardwareInfo {
        cpu_name,
        cpu_cores: logical_cpu_cores,
        ram_total_mb,
        gpu_name: None,
        gpu_vram_mb: None,
        compute_backend,
        measured_asr_tier,
        recommended_model,
    };
    (profile, info)
}

/// Derive a conservative capability tier from recent, comparable dictations.
/// Three samples prevent a single warm-up or unusually short utterance from
/// changing the recommendation. Command-mode traces use a different decode
/// policy and are intentionally excluded.
fn measured_asr_tier_for_backend(backend: ComputeBackend) -> Option<AsrCapabilityTier> {
    let backend_label = match backend {
        ComputeBackend::Cpu => "cpu",
        ComputeBackend::Vulkan | ComputeBackend::Cuda | ComputeBackend::Metal => "gpu",
        ComputeBackend::Unknown => return None,
    };
    let samples: Vec<f64> = crate::perf::recent()
        .into_iter()
        .rev()
        .filter(|trace| is_comparable_dictation_trace(trace, backend_label))
        .filter_map(|trace| {
            let asr_ms = trace.asr_done?.checked_sub(trace.asr_started?)?;
            Some(asr_ms as f64 / trace.audio_duration_ms as f64)
        })
        .take(5)
        .collect();
    classify_asr_rtf(&samples)
}

/// Whether a trace is eligible as an RTF sample for `backend_label`.
///
/// The RTF thresholds `classify_asr_rtf` applies are calibrated against
/// whisper.cpp decode cost, so a Parakeet trace (a completely different
/// transducer RTF) must never enter the sample set, regardless of which
/// engine is active right now — this keeps a stale switch back to Whisper
/// from being mis-tiered by leftover Parakeet traces in the shared buffer.
fn is_comparable_dictation_trace(trace: &crate::perf::PipelineTrace, backend_label: &str) -> bool {
    trace.mode == "dictation"
        && trace.outcome == "ok"
        && trace.backend == Some(backend_label)
        && trace.family == ModelFamily::Whisper
        && trace.audio_duration_ms >= 1_000
}

fn classify_asr_rtf(samples: &[f64]) -> Option<AsrCapabilityTier> {
    if samples.len() < 3
        || samples
            .iter()
            .any(|sample| !sample.is_finite() || *sample < 0.0)
    {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = sorted[sorted.len() / 2];
    Some(if median <= 0.15 {
        AsrCapabilityTier::High
    } else if median <= 0.30 {
        AsrCapabilityTier::Balanced
    } else {
        AsrCapabilityTier::Entry
    })
}

/// Shared logic: verify model exists on disk, load Whisper engine, set as active.
/// Used by both `set_active_model` command and the first-launch setup.
pub fn load_and_activate_model(
    model_id: &str,
    state: &AppState,
) -> Result<ModelLoadOutcome, String> {
    let _load_guard = WHISPER_LOAD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let model_id = ModelManager::canonical_id(model_id);
    let model_path = state
        .model_manager
        .model_path(model_id)
        .ok_or_else(|| format!("Model '{}' is not downloaded", model_id))?;

    // Exclude capture claims for the complete transition. Pipelines use the
    // transition mutex around engine snapshot + worker enqueue, so reset has
    // no gap through which an old engine Arc can be submitted afterward.
    let capture = state
        .capture
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if capture.is_some() {
        return Err("Stop recording before switching Whisper models".into());
    }
    let _transition = state
        .asr_model_transition
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if state
        .audio
        .lock()
        .map_err(|_| "Audio state lock poisoned".to_string())?
        .is_recording()
    {
        return Err("Stop recording before switching Whisper models".into());
    }

    // Reserve 1–2 cores for the OS, audio capture thread, and UI rendering.
    // available_parallelism() returns logical cores (including hyperthreads),
    // so on a 4-core/8-thread laptop this gives 6 threads — enough for Whisper
    // without starving the system. Clamped to [2, 8] for safety.
    let n_threads = std::thread::available_parallelism()
        .map(|n| n.get().saturating_sub(2).clamp(2, 8) as u32)
        .unwrap_or(4);

    // Read the authoritative runtime snapshot. A settings patch persists this
    // value before requesting a reload, so the loader cannot observe stale DB
    // state while multiple WebViews are updating independent fields.
    let use_gpu = state
        .settings
        .read()
        .map(|settings| settings.values().gpu_acceleration)
        .unwrap_or(false);

    // Coalesce duplicate successful loads after waiting for the in-flight
    // request. A GPU->CPU fallback deliberately does not match a GPU request,
    // allowing an explicit re-select to retry the preferred backend. Parakeet
    // ignores the GPU setting entirely, so a matching id and family is enough.
    let family = ModelManager::family(model_id).unwrap_or_default();
    let active_matches = state
        .active_model_id
        .lock()
        .ok()
        .is_some_and(|active| active.as_deref() == Some(model_id));
    let already_loaded = active_matches
        && state
            .engine
            .lock()
            .ok()
            .and_then(|engine| engine.clone())
            .is_some_and(|engine| match (family, &engine) {
                (ModelFamily::Whisper, LoadedAsrEngine::Whisper(whisper)) => {
                    whisper.uses_gpu() == use_gpu
                }
                (ModelFamily::Parakeet, LoadedAsrEngine::Parakeet(_)) => true,
                _ => false,
            });
    if already_loaded {
        return Ok(ModelLoadOutcome {
            gpu_fallback: false,
        });
    }

    let previous_model_id = state
        .active_model_id
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let previous_uses_gpu = state
        .engine
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        .map(|engine| engine.uses_gpu());

    crate::pipeline::reset_final_asr_worker_blocking();

    // Drop the previous model before loading the replacement. Loading GGML
    // weights can use multiple GB, so keeping old + new resident at once can
    // OOM smaller GPUs or 16 GB systems during a model switch.
    *state.engine.lock().unwrap() = None;
    *state.active_model_id.lock().unwrap() = None;

    // Primary and rollback loads share one native-thread implementation so
    // their family dispatch, GPU retry, and CPU fallback behavior cannot drift.
    let t0 = std::time::Instant::now();
    let load_result = load_asr_on_native_thread(state, model_id, &model_path, n_threads, use_gpu);

    let (engine, gpu_fallback) = match load_result {
        Ok(loaded) => loaded,
        Err(switch_error) => {
            // Rollback rebuilds whichever FAMILY the previous model belonged to.
            let rollback = previous_model_id.as_deref().and_then(|previous_id| {
                let previous_path = state.model_manager.model_path(previous_id)?;
                Some((previous_id.to_string(), previous_path))
            });

            if let Some((previous_id, previous_path)) = rollback {
                match load_asr_on_native_thread(
                    state,
                    &previous_id,
                    &previous_path,
                    n_threads,
                    previous_uses_gpu.unwrap_or(false),
                ) {
                    Ok((previous_engine, _)) => {
                        *state.engine.lock().unwrap() = Some(previous_engine);
                        *state.active_model_id.lock().unwrap() = Some(previous_id.clone());
                        debug_assert!(model_slots_are_coherent(Some(&previous_id), true));
                        return Err(format!(
                            "{switch_error}; restored previous model '{previous_id}'"
                        ));
                    }
                    Err(rollback_error) => {
                        crate::diag::log(&format!(
                            "asr rollback failed after switch error: {rollback_error}"
                        ));
                    }
                }
            }

            // No working engine remains. Clear both in-memory mirrors and the
            // persisted ID so callers never observe an ID for a missing model.
            *state.engine.lock().unwrap() = None;
            *state.active_model_id.lock().unwrap() = None;
            let clear_error = crate::commands::settings::commit_runtime_patch(
                state,
                crate::storage::types::SettingsPatch {
                    active_model_id: Some(String::new()),
                    ..Default::default()
                },
                None,
            )
            .err();
            return Err(match clear_error {
                Some(clear_error) => format!(
                    "{switch_error}; previous model could not be restored; clearing persisted model also failed: {clear_error}"
                ),
                None => format!(
                    "{switch_error}; previous model could not be restored and no speech model is active"
                ),
            });
        }
    };

    crate::diag::log(&format!(
        "asr '{model_id}' loaded in {}ms backend={}",
        t0.elapsed().as_millis(),
        if matches!(family, ModelFamily::Parakeet) {
            "cpu (onnx runtime)"
        } else if gpu_fallback {
            "cpu (GPU FALLBACK)"
        } else if use_gpu {
            "gpu"
        } else {
            "cpu (by setting)"
        }
    ));
    if matches!(family, ModelFamily::Whisper) && use_gpu && !gpu_fallback {
        crate::gpu_env::note_gpu_load("asr");
    }

    // Persist and update the authoritative runtime snapshot atomically.
    if let Err(commit_error) = crate::commands::settings::commit_runtime_patch(
        state,
        crate::storage::types::SettingsPatch {
            active_model_id: Some(model_id.to_string()),
            ..Default::default()
        },
        None,
    ) {
        drop(engine);
        return match restore_previous_engine(
            state,
            previous_model_id.as_deref(),
            previous_uses_gpu,
            n_threads,
        ) {
            Ok(previous_id) => Err(format!(
                "Failed to persist model switch: {commit_error}; restored previous model '{previous_id}'"
            )),
            Err(rollback_error) => {
                let clear_error = clear_active_whisper(state).err();
                Err(format!(
                    "Failed to persist model switch: {commit_error}; rollback failed: {rollback_error}{}",
                    clear_error
                        .map(|error| format!("; clearing persisted model failed: {error}"))
                        .unwrap_or_default()
                ))
            }
        };
    }

    *state.engine.lock().unwrap() = Some(engine);
    *state.active_model_id.lock().unwrap() = Some(model_id.to_string());
    debug_assert!(model_slots_are_coherent(Some(model_id), true));

    Ok(ModelLoadOutcome { gpu_fallback })
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

/// Check the catalog's explicit language capability. Unknown IDs are not
/// promoted to multilingual by an ID-shape guess.
pub(crate) fn is_model_multilingual(model_id: &str) -> bool {
    matches!(
        ModelManager::language_support(model_id),
        Some(crate::models::types::ModelLanguageSupport::Multilingual)
    )
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
pub(crate) fn build_whisper_vocab_prompt(
    state: &AppState,
    is_multilingual: bool,
) -> Option<String> {
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

    let static_vocab = if is_multilingual {
        MULTILINGUAL_VOCAB
    } else {
        ENGLISH_VOCAB
    };

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
        if trimmed.is_empty() {
            continue;
        }
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
    let suffix_len = if reinforcement.is_empty() {
        0
    } else {
        2 + reinforcement.len()
    };
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

    if prompt.is_empty() {
        None
    } else {
        Some(prompt)
    }
}

#[cfg(test)]
mod tests {
    use super::{classify_asr_rtf, is_comparable_dictation_trace, model_slots_are_coherent};
    use crate::models::types::{AsrCapabilityTier, ModelFamily};
    use crate::perf::PipelineTrace;

    #[test]
    fn measured_tier_requires_three_samples_and_uses_the_median() {
        assert_eq!(classify_asr_rtf(&[0.10, 0.12]), None);
        assert_eq!(
            classify_asr_rtf(&[0.12, 0.10, 0.80]),
            Some(AsrCapabilityTier::High)
        );
        assert_eq!(
            classify_asr_rtf(&[0.20, 0.29, 0.25]),
            Some(AsrCapabilityTier::Balanced)
        );
        assert_eq!(
            classify_asr_rtf(&[0.31, 0.50, 0.40]),
            Some(AsrCapabilityTier::Entry)
        );
    }

    #[test]
    fn model_transition_identity_never_allows_a_split_slot() {
        assert!(model_slots_are_coherent(None, false));
        assert!(model_slots_are_coherent(Some("whisper-base-en"), true));
        assert!(!model_slots_are_coherent(Some("whisper-base-en"), false));
        assert!(!model_slots_are_coherent(None, true));
    }

    fn comparable_whisper_trace() -> PipelineTrace {
        PipelineTrace {
            generation: 0,
            mode: "dictation",
            model: None,
            backend: Some("cpu"),
            family: ModelFamily::Whisper,
            audio_duration_ms: 2_000,
            claim_to_mic_live_ms: None,
            stop_received: None,
            audio_stopped: None,
            preview_drained: None,
            preprocess_done: None,
            asr_started: Some(0),
            asr_done: Some(300),
            llm_started: None,
            llm_done: None,
            output_started: None,
            output_done: None,
            stop_to_visible_delivery_ms: None,
            visible_delivery_kind: None,
            completed: None,
            outcome: "ok",
        }
    }

    #[test]
    fn tier_sampling_excludes_parakeet_traces_so_a_switch_back_to_whisper_is_not_mistiered() {
        let whisper = comparable_whisper_trace();
        assert!(is_comparable_dictation_trace(&whisper, "cpu"));

        let parakeet = PipelineTrace {
            family: ModelFamily::Parakeet,
            ..comparable_whisper_trace()
        };
        assert!(!is_comparable_dictation_trace(&parakeet, "cpu"));

        // Every other eligibility rule still applies alongside the family filter.
        let wrong_backend = PipelineTrace {
            backend: Some("gpu"),
            ..comparable_whisper_trace()
        };
        assert!(!is_comparable_dictation_trace(&wrong_backend, "cpu"));
    }
}
