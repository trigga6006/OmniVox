use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

use crate::asr::engine::AsrEngine;
use crate::error::ErrorCode;
use crate::focus::{
    capture_foreground_window, get_process_name_from_hwnd, restore_foreground_window,
};
use crate::llm::schema::SlotExtraction;
use crate::llm::template::render_markdown;
use crate::postprocess::processor::TextProcessor;
use crate::screen_context::ScreenContext;
use crate::state::AppState;

/// Payload emitted on `structured-output-ready` so the overlay can render the
/// panel and offer Paste / Copy / Edit / Dismiss actions.
#[derive(Clone, serde::Serialize)]
struct StructuredOutputPayload {
    markdown: String,
    slots: SlotExtraction,
    raw_transcript: String,
}

/// Payload emitted with `recording-state-change` when the state is "error".
#[derive(Clone, serde::Serialize)]
struct ErrorPayload {
    state: &'static str,
    code: ErrorCode,
    message: String,
}

/// Emit a typed error event so the frontend can show specific guidance.
fn emit_error(app_handle: &tauri::AppHandle, code: ErrorCode, message: impl Into<String>) {
    let payload = ErrorPayload {
        state: "error",
        code,
        message: message.into(),
    };
    let _ = app_handle.emit("recording-error", &payload);
    let _ = app_handle.emit("recording-state-change", "error");
}

// ── Capture ownership coordination ───────────────────────────────────────
//
// Dictation and Command Mode share one mic + Whisper engine.  `capture_mode`
// is the ownership gate; `capture_live` + `pending_stop` close the start-vs-stop
// race for quick push-to-talk taps (a release that lands before `audio.start()`
// flips the engine live).  All capture_mode locks recover from poison rather
// than failing open — a poisoned safety gate must not silently behave as "not
// owned".
use std::sync::atomic::Ordering as CaptureOrdering;

fn read_capture_mode(state: &AppState) -> crate::state::CaptureMode {
    match state.capture_mode.lock() {
        Ok(g) => *g,
        Err(p) => *p.into_inner(),
    }
}

/// Atomically claim capture ownership for `mode`. Returns false if the mic is
/// already owned (anything other than Idle), making start-vs-start race-free.
fn claim_capture(state: &AppState, mode: crate::state::CaptureMode) -> bool {
    let mut guard = match state.capture_mode.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if *guard != crate::state::CaptureMode::Idle {
        return false;
    }
    *guard = mode;
    state.pending_stop.store(false, CaptureOrdering::SeqCst);
    state.capture_live.store(false, CaptureOrdering::SeqCst);
    true
}

/// Mark the active capture as live (audio is actually recording).
fn mark_capture_live(state: &AppState) {
    state.capture_live.store(true, CaptureOrdering::SeqCst);
}

/// Release capture ownership back to Idle.
fn release_capture(state: &AppState) {
    state.capture_live.store(false, CaptureOrdering::SeqCst);
    let mut guard = match state.capture_mode.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    *guard = crate::state::CaptureMode::Idle;
}

/// Called by a start path once audio is live: returns true if a stop arrived
/// during startup (so the caller should stop immediately). The swap happens
/// under the capture_mode lock to serialize with [`request_stop`].
fn take_startup_stop(state: &AppState) -> bool {
    let _guard = match state.capture_mode.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    state.pending_stop.swap(false, CaptureOrdering::SeqCst)
}

enum StopDecision {
    /// Capture is live — stop and process now.
    StopNow,
    /// Still starting — deferred; the start path will stop once live.
    Deferred,
    /// This capture isn't owned by the requesting path.
    NotOurs,
}

/// Decide what a stop request for `owner` should do, coordinating with the
/// start path via the capture_mode lock + `capture_live` atomic.
fn request_stop(state: &AppState, owner: crate::state::CaptureMode) -> StopDecision {
    let _guard = match state.capture_mode.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if *_guard != owner {
        return StopDecision::NotOurs;
    }
    if state.capture_live.load(CaptureOrdering::SeqCst) {
        StopDecision::StopNow
    } else {
        state.pending_stop.store(true, CaptureOrdering::SeqCst);
        StopDecision::Deferred
    }
}

/// Toggle recording on/off. Called by the frontend start/stop commands.
pub async fn toggle_recording(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    let is_recording = match state.audio.lock() {
        Ok(audio) => audio.is_recording(),
        Err(_) => {
            emit_error(
                app_handle,
                ErrorCode::InternalError,
                "Audio state lock poisoned",
            );
            return;
        }
    };

    if !is_recording {
        start_recording(app_handle, &state);
    } else {
        stop_and_transcribe(app_handle, &state).await;
    }
}

// ── Hotkey entry points ──────────────────────────────────────────────────
//
// The hotkey hook is a single serialized thread: a key-down's `fire_start` runs
// to completion before the matching key-up's `fire_stop`.  By claiming ownership
// / deciding the stop SYNCHRONOUSLY here (then spawning the heavy async work), a
// release can never be processed before its press has claimed ownership — which
// closes the stop-before-claim race entirely.

/// Claim capture ownership for a hotkey press. Call synchronously on the hook
/// thread BEFORE spawning the capture worker. Returns false if already owned.
pub fn try_claim_capture(app_handle: &tauri::AppHandle, mode: crate::state::CaptureMode) -> bool {
    claim_capture(&app_handle.state::<AppState>(), mode)
}

/// Decide whether a hotkey release should stop now. Call synchronously on the
/// hook thread. A release that lands before the capture is live is recorded as a
/// deferred stop inside `request_stop` (the start worker honors it once live).
pub fn should_stop_now(app_handle: &tauri::AppHandle, owner: crate::state::CaptureMode) -> bool {
    matches!(
        request_stop(&app_handle.state::<AppState>(), owner),
        StopDecision::StopNow
    )
}

/// Begin microphone capture (frontend / toggle entry — claims ownership itself).
pub fn start_recording(app_handle: &tauri::AppHandle, state: &AppState) {
    // Race-free check-and-set that requires Idle, so it bails if the mic is
    // already owned — by an active dictation (no self-corruption) or by Command
    // Mode.  The hotkey path claims separately (synchronously on the hook
    // thread) and calls `start_recording_inner` directly.
    if !claim_capture(state, crate::state::CaptureMode::Dictation) {
        return;
    }
    start_recording_inner(app_handle, state);
}

/// The dictation start body. Assumes capture ownership is ALREADY claimed.
pub(crate) fn start_recording_inner(app_handle: &tauri::AppHandle, state: &AppState) {
    // Snapshot the foreground window BEFORE we do anything that might steal focus.
    let fg = capture_foreground_window();
    if let Ok(mut prev) = state.prev_foreground.lock() {
        *prev = fg;
    }

    // Load settings once — used for auto-switch and audio ducking below.
    let settings = crate::storage::settings::get_settings(&state.db).ok();
    if let Ok(mut guard) = state.preview_done_rx.lock() {
        *guard = None;
    }

    // Auto-switch context mode based on the foreground application.
    if let Some(hwnd) = fg {
        let auto_switch = settings
            .as_ref()
            .map(|s| s.auto_switch_modes)
            .unwrap_or(false);

        if auto_switch {
            if let Some(process_name) = get_process_name_from_hwnd(hwnd) {
                // Find the target mode: either a bound mode or General fallback.
                let target_mode_id = match crate::storage::app_bindings::find_mode_for_process(
                    &state.db,
                    &process_name,
                ) {
                    Ok(Some(id)) => Some(id),
                    _ => {
                        // No binding for this app — fall back to the builtin General mode
                        crate::storage::context_modes::get_general_mode_id(&state.db).ok()
                    }
                };

                if let Some(target_mode_id) = target_mode_id {
                    let current_mode = state.active_context_mode_id.lock().unwrap().clone();
                    if current_mode.as_deref() != Some(&target_mode_id) {
                        if let Err(e) = crate::commands::context_modes::activate_mode_internal(
                            state,
                            &target_mode_id,
                        ) {
                            eprintln!("Auto-switch mode failed: {e}");
                        } else if let Ok(mode) =
                            crate::storage::context_modes::get_mode(&state.db, &target_mode_id)
                        {
                            let _ = app_handle.emit(
                                "context-mode-changed",
                                serde_json::json!({
                                    "id": mode.id.to_string(),
                                    "name": mode.name,
                                    "icon": mode.icon,
                                    "color": mode.color,
                                }),
                            );
                        }
                    }
                }
            }
        }
    }

    // Duck system volume so other audio doesn't compete with the mic.
    if settings.as_ref().map(|s| s.audio_ducking).unwrap_or(true) {
        // Convert ducking_amount (0–100, % reduction) to a volume factor.
        // 70 → keep 30% of volume (factor 0.30), 100 → mute (factor 0.0).
        let amount = settings.as_ref().map(|s| s.ducking_amount).unwrap_or(70);
        let factor = 1.0 - (amount.min(100) as f32 / 100.0);
        crate::audio::ducking::duck(Some(factor));
    }

    // Spawn screen-context capture in parallel with the user speaking.  By
    // the time stop_and_transcribe runs, the receiver typically already has
    // a value — capture cost (UIA tree walk, ~50–200 ms) is fully hidden
    // under the user's utterance.
    if settings
        .as_ref()
        .map(|s| s.use_screen_context)
        .unwrap_or(true)
    {
        let (tx, rx) = oneshot::channel::<ScreenContext>();
        if let Ok(mut guard) = state.screen_context_rx.lock() {
            *guard = Some(rx);
        }
        let fg_for_task = fg;
        tokio::task::spawn_blocking(move || {
            let ctx = crate::screen_context::capture(fg_for_task);
            // Receiver may have been dropped if the user stopped immediately
            // and we already moved past consumption — silently OK.
            let _ = tx.send(ctx);
        });
    } else {
        // Feature toggled off — clear any stale receiver from a prior run
        // so the consumer side never grabs leftover context.
        if let Ok(mut guard) = state.screen_context_rx.lock() {
            *guard = None;
        }
    }

    let mut audio = match state.audio.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            // Recover from a poisoned mutex — the previous holder panicked,
            // but the AudioCapture inside is still usable after a reset.
            let mut guard = poisoned.into_inner();
            guard.cancel(); // Reset to clean state
            guard
        }
    };

    if let Err(e) = audio.start() {
        eprintln!("Failed to start recording: {e}");
        // Release the ownership we just claimed so the mic isn't left stuck.
        release_capture(state);
        emit_error(
            app_handle,
            e.code(),
            format!("Failed to start recording: {e}"),
        );
        return;
    }

    // Grab Arc handles for the audio level emitter before dropping the lock
    let is_recording = audio.is_recording_flag();
    let rms_level = audio.rms_level_ref();
    drop(audio);

    // Spawn a periodic task that emits audio-level events to the frontend.
    // 150 ms strikes a balance between smooth VU meter animation and CPU usage.
    // (100 ms was too aggressive for low-end laptops — 10 events/s of React
    // re-renders + CSS transitions caused pill jank on integrated GPUs.)
    let handle = app_handle.clone();
    let is_rec_clone = is_recording.clone();
    tauri::async_runtime::spawn(async move {
        use std::sync::atomic::Ordering;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if !is_rec_clone.load(Ordering::Relaxed) {
                break;
            }
            let level = f32::from_bits(rms_level.load(Ordering::Relaxed));
            let _ = handle.emit("audio-level", level);
        }
    });

    // Spawn live preview task — periodically transcribes the last 5s of
    // audio and emits partial results to the overlay pill.
    //
    // Architecture: a dedicated std::thread owns a single `WhisperState` for
    // the entire preview session.  An async task periodically snapshots
    // audio and forwards it to the worker over a capacity-1 sync channel —
    // which naturally preserves the "at most one inference in flight"
    // invariant (if the worker is busy, try_send fails and we drop this
    // frame rather than queueing it).
    //
    // Win over the old design: the old code called `engine.transcribe_preview`
    // inside `spawn_blocking` every iteration, and each call did
    // `ctx.create_state()` — allocating ~500 MB of decode buffers that got
    // freed seconds later.  On 16 GB machines the churn caused visible
    // pauses and peak memory spikes.  Now the state is allocated ONCE at
    // recording start and reused across every preview tick until recording
    // ends.
    let live_preview = settings.as_ref().map(|s| s.live_preview).unwrap_or(false);

    if live_preview {
        let engine_opt: Option<Arc<crate::asr::engine::WhisperEngine>> = state
            .engine
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(Arc::clone));

        if let Some(engine) = engine_opt {
            // Capacity-1 sync channel: if worker is busy when sender tries
            // to send, try_send fails fast and we skip this round.
            let (tx_audio, rx_audio) = std::sync::mpsc::sync_channel::<Vec<f32>>(1);
            let (preview_done_tx, preview_done_rx) = oneshot::channel::<()>();

            // Worker thread — owns the WhisperState for the duration of
            // this recording session.  Exits when the async task drops its
            // sender (rx.recv returns Err).
            let worker_handle = app_handle.clone();
            let worker_engine = engine.clone();
            let worker_is_rec = is_recording.clone();
            let preview_worker = std::thread::Builder::new()
                .name("omnivox-preview".into())
                .spawn(move || {
                    use std::sync::atomic::Ordering;
                    use std::sync::mpsc::RecvTimeoutError;
                    let mut preview_state: Option<whisper_rs::WhisperState> = None;
                    loop {
                        let audio =
                            match rx_audio.recv_timeout(std::time::Duration::from_millis(250)) {
                                Ok(audio) => audio,
                                Err(RecvTimeoutError::Timeout) => {
                                    if !worker_is_rec.load(Ordering::Relaxed) {
                                        break;
                                    }
                                    continue;
                                }
                                Err(RecvTimeoutError::Disconnected) => break,
                            };
                        // Cancellation check — if user stopped recording
                        // between send and receive, skip inference.
                        if !worker_is_rec.load(Ordering::Relaxed) {
                            break;
                        }
                        if preview_state.is_none() {
                            preview_state = match worker_engine.create_preview_state() {
                                Ok(s) => Some(s),
                                Err(e) => {
                                    eprintln!("Preview: create_state failed: {e}");
                                    break;
                                }
                            };
                        }
                        let state = preview_state.as_mut().expect("preview_state set above");

                        match worker_engine.transcribe_preview_with_state(
                            state,
                            &audio,
                            Some(worker_is_rec.clone()),
                        ) {
                            Ok(text) if !text.is_empty() => {
                                if worker_is_rec.load(Ordering::Relaxed) {
                                    let _ = worker_handle.emit("transcription-preview", &text);
                                } else {
                                    break;
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                // Aborted-on-stop is the expected fast path —
                                // exit quietly so the final transcription can
                                // claim the decode buffers immediately.
                                if !worker_is_rec.load(Ordering::Relaxed) {
                                    break;
                                }
                                eprintln!("Preview inference failed: {e}");
                            }
                        }
                    }
                    // state drops here, freeing decode buffers.
                    let _ = preview_done_tx.send(());
                });

            match preview_worker {
                Ok(_) => {
                    if let Ok(mut guard) = state.preview_done_rx.lock() {
                        *guard = Some(preview_done_rx);
                    }
                }
                Err(e) => eprintln!("Preview: failed to spawn worker: {e}"),
            }

            // Async snapshot task — samples audio every 3 s, forwards to
            // worker.  When recording stops it returns, dropping tx_audio
            // and cleanly terminating the worker thread.
            let ctrl_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                use std::sync::atomic::Ordering;
                const PREVIEW_SAMPLES: usize = 16_000 * 5;

                tokio::time::sleep(std::time::Duration::from_secs(3)).await;

                loop {
                    if !is_recording.load(Ordering::Relaxed) {
                        break;
                    }

                    let samples = {
                        let st: tauri::State<'_, AppState> = ctrl_handle.state();
                        let audio = match st.audio.lock() {
                            Ok(g) => g,
                            Err(_) => break,
                        };
                        audio.snapshot_tail(PREVIEW_SAMPLES)
                    };

                    if samples.len() >= 8_000 {
                        let mut preview_samples = samples;
                        crate::audio::normalize::normalize_peak(&mut preview_samples);
                        // try_send drops this frame if the worker is still
                        // processing the previous one — backpressure without
                        // queueing.  Err(Disconnected) means worker died; exit.
                        use std::sync::mpsc::TrySendError;
                        match tx_audio.try_send(preview_samples) {
                            Ok(()) => {}
                            Err(TrySendError::Full(_)) => { /* worker busy, skip */ }
                            Err(TrySendError::Disconnected(_)) => break,
                        }
                    }

                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                }
                // Drop tx_audio → worker rx.recv errors → worker exits.
            });
        }
    }

    // All startup side-effects (audio-level emitter, live-preview worker) are
    // installed now — only here do we mark the capture live, so a release during
    // setup is *deferred* (no concurrent stop racing the preview-worker install)
    // and then honored immediately below.  A quick tap can never stick.
    mark_capture_live(state);
    if take_startup_stop(state) {
        // Released during startup (quick tap) — stop instead of showing recording.
        let handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            let st = handle.state::<AppState>();
            stop_and_transcribe(&handle, &st).await;
        });
    } else {
        // Announce "recording" only now that the capture is fully live. Emitting
        // it earlier let a frontend stop/cancel land mid-setup and race this
        // worker (release with capture_mode already reset to Idle).
        let _ = app_handle.emit("recording-state-change", "recording");
    }
}

async fn wait_for_preview_worker(state: &AppState) {
    let rx = state
        .preview_done_rx
        .lock()
        .ok()
        .and_then(|mut guard| guard.take());

    if let Some(rx) = rx {
        match tokio::time::timeout(Duration::from_millis(1500), rx).await {
            Ok(Ok(())) | Ok(Err(_)) => {}
            Err(_) => eprintln!(
                "Preview worker still releasing decode buffers; continuing with final transcription"
            ),
        }
    }
}

/// Stop capture, run Whisper inference, post-process, and output the text.
pub async fn stop_and_transcribe(app_handle: &tauri::AppHandle, state: &AppState) {
    // Never finish a Command-Mode capture as dictation.  The hotkey stop path
    // already checks this, but the public `stop_recording` command and
    // `toggle_recording` call here directly — without this guard they could
    // transcribe a command utterance as text.  Poison-safe read (a poisoned
    // safety gate must not silently behave as "not command").  Ownership is
    // released only after we claim the samples below.
    if read_capture_mode(state) == crate::state::CaptureMode::Command {
        return;
    }

    // Restore system volume immediately — don't wait for transcription.
    crate::audio::ducking::unduck();

    let _ = app_handle.emit("recording-state-change", "processing");

    // Snapshot every setting the rest of this function needs in ONE DB read.
    // Previously this was 3 separate get_settings() calls (noise_reduction,
    // voice_commands/command_send, ship_mode) — each a full table scan and
    // HashMap build.  Cache once, reuse everywhere.
    let settings = crate::storage::settings::get_settings(&state.db).ok();

    // Drain the screen-context capture spawned at recording start.  Wait at
    // most 50 ms — capture should already be done since the user has been
    // speaking for a while.  On timeout we proceed without context.
    let screen_context: Option<ScreenContext> = if settings
        .as_ref()
        .map(|s| s.use_screen_context)
        .unwrap_or(true)
    {
        let rx = state
            .screen_context_rx
            .lock()
            .ok()
            .and_then(|mut g| g.take());
        if let Some(rx) = rx {
            match tokio::time::timeout(Duration::from_millis(50), rx).await {
                Ok(Ok(ctx)) => {
                    if ctx.is_empty() {
                        None
                    } else {
                        Some(ctx)
                    }
                }
                _ => None,
            }
        } else {
            None
        }
    } else {
        None
    };

    // 1. Stop capture and get raw audio samples
    let samples = {
        let mut audio = match state.audio.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match audio.stop() {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to stop recording: {e}");
                release_capture(state);
                emit_error(
                    app_handle,
                    e.code(),
                    format!("Failed to stop recording: {e}"),
                );
                return;
            }
        }
    };

    // Samples are claimed — release capture ownership so the next dictation or
    // command capture can begin while this one finishes transcribing.
    release_capture(state);

    // 1a. Let the live-preview worker drop its WhisperState before final
    // transcription allocates a fresh state. This avoids overlapping decode
    // buffers on smaller GPUs and 16 GB machines.
    wait_for_preview_worker(state).await;

    if samples.is_empty() {
        let _ = app_handle.emit("recording-state-change", "idle");
        return;
    }

    // 1b. Conditionally denoise audio with RNNoise before Whisper.
    let mut samples = samples;
    let noise_reduction = settings
        .as_ref()
        .map(|s| s.noise_reduction)
        .unwrap_or(false);
    if noise_reduction {
        crate::audio::denoise::denoise(&mut samples);
    }

    // 1c. Normalize audio levels for consistent Whisper performance.
    //     Done here (not in the capture callback) to avoid affecting the VU meter.
    crate::audio::normalize::normalize_peak(&mut samples);

    // 2. Transcribe — CPU-bound, runs on a blocking thread to keep the async
    //    runtime free for UI events during inference.
    let engine: Arc<crate::asr::engine::WhisperEngine> = {
        let guard = match state.engine.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match guard.as_ref().map(Arc::clone) {
            Some(e) => e,
            None => {
                drop(guard);
                eprintln!("No model loaded — cannot transcribe");
                emit_error(
                    app_handle,
                    ErrorCode::NoModelLoaded,
                    "No model loaded — go to Models to download one",
                );
                return;
            }
        }
    };

    // Snapshot the current vocabulary prompt so we can restore it after this
    // transcription — screen-context tokens are dynamic per utterance and
    // must not bleed into subsequent calls.  When no merged prompt is
    // produced (no screen context, or feature off) we leave the engine
    // untouched and skip the restore.
    let saved_initial_prompt = engine.get_initial_prompt();
    let merged_prompt = screen_context.as_ref().and_then(|ctx| {
        crate::screen_context::build_initial_prompt(ctx, saved_initial_prompt.as_deref())
    });
    let prompt_was_overridden = merged_prompt.is_some();
    if let Some(p) = merged_prompt.as_ref() {
        engine.set_initial_prompt(Some(p.clone()));
    }

    let engine_for_transcribe = Arc::clone(&engine);
    let transcription =
        match tokio::task::spawn_blocking(move || engine_for_transcribe.transcribe(&samples)).await
        {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => {
                if prompt_was_overridden {
                    engine.set_initial_prompt(saved_initial_prompt.clone());
                }
                eprintln!("Transcription failed: {e}");
                emit_error(
                    app_handle,
                    ErrorCode::TranscriptionFailed,
                    format!("Transcription failed: {e}"),
                );
                return;
            }
            Err(e) => {
                if prompt_was_overridden {
                    engine.set_initial_prompt(saved_initial_prompt.clone());
                }
                eprintln!("Transcription task panicked: {e}");
                emit_error(
                    app_handle,
                    ErrorCode::TranscriptionPanicked,
                    format!("Transcription crashed: {e}"),
                );
                return;
            }
        };

    if prompt_was_overridden {
        engine.set_initial_prompt(saved_initial_prompt);
    }

    if transcription.text.is_empty() {
        let _ = app_handle.emit("recording-state-change", "idle");
        return;
    }

    // 3. Post-process (dictionary replacements, capitalization, etc.)
    let processed_text = {
        let processor = match state.processor.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match processor.process(&transcription.text) {
            Ok(processed) => processed.processed,
            Err(_) => transcription.text.clone(),
        }
    };

    // 3b. Structured Mode branch.
    //
    // When the user has Structured Mode enabled, we divert to the local LLM
    // to produce a slot-filled Markdown prompt instead of running the
    // deterministic list formatter and voice-command parser.  Failure modes
    // (no runner loaded, timeout, malformed JSON) degrade gracefully to the
    // plain path — structured mode must never block dictation.
    //
    // Voice-command gate: when `structured_voice_command` is on, the user
    // must end their dictation with the trigger word "Voxify" for the LLM
    // to run.  If the word is present we strip it from the text (so it
    // doesn't end up in the user-facing output) and let structuring
    // proceed.  If it's absent, we fall through to plain output even
    // though `structured_mode` is on.  Mirrors how `command_send` gates
    // Ship Mode behind the "send" word.
    let structured_enabled = settings
        .as_ref()
        .map(|s| s.structured_mode)
        .unwrap_or(false);
    let voice_command_gate = settings
        .as_ref()
        .map(|s| s.structured_voice_command)
        .unwrap_or(false);
    let min_chars = settings
        .as_ref()
        .map(|s| s.structured_min_chars)
        .unwrap_or(40) as usize;
    let llm_timeout = settings.as_ref().map(|s| s.llm_timeout_secs).unwrap_or(8);

    // Detect and strip the trailing "Voxify" trigger word — but ONLY when
    // the voice-command gate is armed.  With the gate off, "voxify" is
    // treated as ordinary dictation content: stripping it unconditionally
    // would silently corrupt plain output (and would also steal the word
    // from Structured Mode runs that don't require it).  The gate is the
    // single signal that promotes the word from content to command.
    let (processed_text, voxify_said) = if voice_command_gate {
        crate::llm::voxify::detect_and_strip_trigger(&processed_text)
    } else {
        (processed_text, false)
    };
    crate::llm::diaglog::log(&format!(
        "pipeline: voxify_said={} voice_gate={} structured_enabled={}",
        voxify_said, voice_command_gate, structured_enabled
    ));

    // Resolve whether the LLM should run for THIS utterance.  With the
    // gate on, it's an explicit opt-in per utterance.  With the gate off,
    // the global setting governs (every qualifying transcription runs).
    let should_structure = structured_enabled && (!voice_command_gate || voxify_said);

    let configured_llm_id = settings
        .as_ref()
        .and_then(|s| s.active_llm_model_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| {
            state
                .active_llm_model_id
                .lock()
                .ok()
                .and_then(|g| g.clone())
        })
        .or_else(|| crate::commands::llm::preferred_downloaded_llm_id(state));

    let runner_opt = if should_structure {
        let existing = state.llm_runner.lock().ok().and_then(|g| g.clone());
        if existing.is_some() {
            crate::llm::diaglog::log("runner: using existing loaded runner");
            existing
        } else if let Some(model_id) = configured_llm_id.clone() {
            crate::llm::diaglog::log(&format!("runner: lazy-loading '{model_id}'"));
            match crate::commands::llm::load_and_activate_llm(&model_id, state) {
                Ok(()) => {
                    crate::llm::diaglog::log("runner: lazy-load ok");
                    state.llm_runner.lock().ok().and_then(|g| g.clone())
                }
                Err(e) => {
                    crate::llm::diaglog::log(&format!("runner: lazy-load FAILED: {e}"));
                    let _ =
                        app_handle.emit("structured-mode-degraded", &format!("Load failed: {e}"));
                    None
                }
            }
        } else {
            crate::llm::diaglog::log("runner: structured_mode=true but no configured model_id");
            None
        }
    } else {
        None
    };

    const STRUCTURED_INPUT_CHAR_CAP: usize = 1600;
    let structured_input = if processed_text.chars().count() > STRUCTURED_INPUT_CHAR_CAP {
        let clipped: String = processed_text
            .chars()
            .take(STRUCTURED_INPUT_CHAR_CAP)
            .collect();
        crate::llm::diaglog::log(&format!(
            "pipeline: truncating structured input from {} to {} chars",
            processed_text.chars().count(),
            STRUCTURED_INPUT_CHAR_CAP
        ));
        clipped
    } else {
        processed_text.clone()
    };

    let structured: Option<(String, SlotExtraction)> = if should_structure
        && processed_text.chars().count() >= min_chars
    {
        if let Some(runner) = runner_opt {
            let _ = app_handle.emit("recording-state-change", "structuring");
            let t0 = std::time::Instant::now();

            // Phase 2: when both Structured Mode and the screen-context
            // sub-toggle are on, feed the captured tokens into Qwen so
            // it can substitute phonetic guesses with verbatim screen
            // text.  Otherwise pass empty tokens — the runner falls
            // through to the legacy single-arg prompt path.
            let pass_screen_tokens = settings
                .as_ref()
                .map(|s| s.use_screen_context && s.structured_use_screen_context)
                .unwrap_or(false);
            let (sm_tokens, sm_app) = if pass_screen_tokens {
                screen_context
                    .as_ref()
                    .map(|c| (c.tokens.clone(), c.source_app.clone()))
                    .unwrap_or_default()
            } else {
                (Vec::new(), None)
            };

            crate::llm::diaglog::log(&format!(
                    "pipeline: starting extraction input_chars={} llm_input_chars={} timeout={}s min_chars={} screen_tokens={}",
                    processed_text.chars().count(),
                    structured_input.chars().count(),
                    llm_timeout,
                    min_chars,
                    sm_tokens.len(),
                ));
            match runner
                .extract_with_context_and_timeout(
                    structured_input.clone(),
                    sm_tokens,
                    sm_app,
                    Duration::from_secs(llm_timeout as u64),
                )
                .await
            {
                Ok(slots) => {
                    crate::llm::diaglog::log(&format!(
                        "pipeline: extraction OK in {}ms slots={:?}",
                        t0.elapsed().as_millis(),
                        slots
                    ));
                    let md = render_markdown(&slots);
                    Some((md, slots))
                }
                Err(e) => {
                    crate::llm::diaglog::log(&format!(
                        "pipeline: extraction FAILED after {}ms: {e}",
                        t0.elapsed().as_millis()
                    ));
                    let _ = app_handle.emit(
                        "structured-mode-degraded",
                        &format!("Extraction failed: {e}"),
                    );
                    None
                }
            }
        } else {
            let _ = app_handle.emit(
                "structured-mode-degraded",
                "No LLM model available for Structured Mode. Using plain dictation.",
            );
            None
        }
    } else if should_structure {
        crate::llm::diaglog::log(&format!(
            "pipeline: SKIPPED (input too short {} < {} chars)",
            processed_text.chars().count(),
            min_chars
        ));
        let _ = app_handle.emit(
            "structured-mode-degraded",
            &format!(
                "Dictation too short ({} chars) — need at least {}. Using plain output.",
                processed_text.chars().count(),
                min_chars
            ),
        );
        None
    } else {
        None
    };

    // 4. Apply deterministic list formatting (bullet lists for enumerated
    //     items).  Structural formatting is handled here at zero cost.
    //     When Structured Mode is active the LLM is the sole formatter —
    //     skip list formatting so we don't double-handle.
    let final_text = if let Some((md, _)) = &structured {
        md.clone()
    } else {
        crate::postprocess::formatter::format_lists(&processed_text)
    };

    // 4b. Voice command detection (if enabled).
    //     Splits text into [Text | Command] segments so the output router can
    //     type text and execute keystrokes (Shift+Enter, Ctrl+Backspace, etc.).
    //     Disabled while Structured Mode is active — the LLM already decided
    //     on the output shape and voice commands would break it.
    let voice_commands_enabled = settings.as_ref().map(|s| s.voice_commands).unwrap_or(false);
    let command_send_enabled = settings.as_ref().map(|s| s.command_send).unwrap_or(true);
    let voice_segments = if voice_commands_enabled && structured.is_none() {
        Some(
            crate::postprocess::voice_commands::parse_commands_with_options(
                &final_text,
                command_send_enabled,
            ),
        )
    } else {
        None
    };

    // 5. Kick off focus restoration in parallel with output.
    //     Skipped for Structured Mode since the panel handles pasting.
    let prev_hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
    let focus_task = if structured.is_none() {
        prev_hwnd.map(|hwnd| tokio::task::spawn_blocking(move || restore_foreground_window(hwnd)))
    } else {
        None
    };

    // Wait for focus restoration to complete before outputting text.
    if let Some(task) = focus_task {
        let _ = task.await;
    }

    // 6. Output to the focused application.
    //     When Structured Mode produced a result we skip auto-paste entirely —
    //     the Structured panel becomes the commit point (Paste / Copy / Edit /
    //     Dismiss).  The Markdown still reaches the UI via
    //     `structured-output-ready`, and history still records it.
    let output_config = match state.output_config.lock() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if structured.is_none() {
        let output_result = if let Some(ref segments) = voice_segments {
            state.output.send_segments(segments, &output_config)
        } else {
            state.output.send(&final_text, &output_config)
        };
        if let Err(e) = output_result {
            eprintln!("Output failed: {e}");
            emit_error(app_handle, e.code(), format!("Output failed: {e}"));
        }
    }

    // 6b. Ship Mode — automatically press Enter to send the message.
    //     Only fires when type simulation was used (clipboard-only can't auto-send).
    //     When Command Send is enabled it overrides Ship Mode — the user controls
    //     sending by saying "send" at the end, so we skip the automatic Enter.
    //     Also skipped in Structured Mode — pasting is user-driven from the panel.
    let command_send_active = voice_commands_enabled && command_send_enabled;
    if structured.is_none()
        && output_config.ship_mode
        && !command_send_active
        && matches!(
            output_config.mode,
            crate::output::types::OutputMode::TypeSimulation
                | crate::output::types::OutputMode::Both
        )
    {
        let _ = tokio::task::spawn_blocking(|| {
            // Wait for all keystrokes to land in the target app.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            if let Ok(mut enigo) = enigo::Enigo::new(&enigo::Settings::default()) {
                let _ =
                    enigo::Keyboard::key(&mut enigo, enigo::Key::Return, enigo::Direction::Click);
            }
        })
        .await;
    }

    // 7. Notify frontend of the result.
    //
    //    `transcription-result` always fires — History auto-refresh, the
    //    global last-transcription store, and Notes-append all listen for
    //    it, so skipping it on the Structured path would silently break
    //    those flows.  For Structured Mode we also emit the rich payload
    //    so the overlay can render the preview panel.
    let _ = app_handle.emit("transcription-result", &final_text);
    if let Some((md, slots)) = &structured {
        let _ = app_handle.emit(
            "structured-output-ready",
            &StructuredOutputPayload {
                markdown: md.clone(),
                slots: slots.clone(),
                // Use the pre-processor ASR output so "View raw transcript"
                // actually shows what the user said — processed_text has
                // already been through filler removal, dictionary, and
                // capitalization, which would mask the original words.
                raw_transcript: transcription.text.clone(),
            },
        );
    }

    // 8. Save to history.
    //     `text` is the final paste-ready string (Markdown in Structured
    //     Mode, plain text otherwise).  `raw_transcript` stores the
    //     pre-processor ASR text so the Structured panel's "View raw"
    //     disclosure always reflects what the user actually spoke.
    let raw_transcript = if structured.is_some() {
        Some(transcription.text.clone())
    } else {
        None
    };
    let record = crate::storage::types::TranscriptionRecord {
        id: uuid::Uuid::new_v4(),
        text: final_text,
        duration_ms: transcription.duration_ms,
        model_name: transcription.model_name,
        created_at: chrono::Utc::now(),
        raw_transcript,
    };
    if let Err(e) = crate::storage::history::save_transcription(&state.db, &record) {
        eprintln!("Failed to save transcription to history: {e}");
    }

    let _ = app_handle.emit("recording-state-change", "idle");
}

/// Cancel an in-progress recording without transcribing.
pub fn cancel_recording(app_handle: &tauri::AppHandle, state: &AppState) {
    // Reset the surface that actually owns the mic — cancelling a Command-Mode
    // capture must clear the command pill, not emit a dictation idle event.
    let was_command = read_capture_mode(state) == crate::state::CaptureMode::Command;
    release_capture(state);
    crate::audio::ducking::unduck();
    if let Ok(mut audio) = state.audio.lock() {
        audio.cancel();
    }
    if was_command {
        let _ = app_handle.emit("command-state-change", "idle");
    } else {
        let _ = app_handle.emit("recording-state-change", "idle");
    }
}

/// Get the current audio level for the VU meter (0.0–1.0).
pub fn current_audio_level(state: &AppState) -> f32 {
    state.audio.lock().map(|a| a.current_level()).unwrap_or(0.0)
}

// ── Command Mode ─────────────────────────────────────────────────────────
//
// A separate capture path from dictation: the whole utterance is one command,
// triggered by the Right Ctrl hotkey (see hotkey.rs).  Shares the mic + Whisper
// engine but routes the transcript through the command matcher + executor
// (see crate::actions) instead of producing dictated text.

/// Payload for `command-confirm` — a low-confidence action awaiting Enter/Esc.
#[derive(Clone, serde::Serialize)]
struct CommandConfirmPayload {
    summary: String,
}

/// Payload for `command-result`.
#[derive(Clone, serde::Serialize)]
struct CommandResultPayload {
    status: &'static str, // "done" | "error"
    summary: String,
}

fn emit_command_result(
    app_handle: &tauri::AppHandle,
    status: &'static str,
    summary: impl Into<String>,
) {
    let _ = app_handle.emit(
        "command-result",
        &CommandResultPayload {
            status,
            summary: summary.into(),
        },
    );
}

/// The command capture body. Assumes capture ownership is ALREADY claimed (the
/// hotkey hook claims synchronously on its thread before spawning this).
pub(crate) async fn start_command_inner(app_handle: &tauri::AppHandle) {
    let state = app_handle.state::<AppState>();

    // Snapshot the foreground window so we can restore focus before firing key
    // chords / target window actions at it.
    let fg = capture_foreground_window();
    if let Ok(mut prev) = state.prev_foreground.lock() {
        *prev = fg;
    }
    if let Ok(mut pending) = state.pending_command.lock() {
        *pending = None;
    }

    // Scope the audio guard so it's provably dropped before any `.await` below
    // (a MutexGuard isn't Send, and this fn is spawned as a Send future).
    let start_result = {
        let mut audio = match state.audio.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.cancel();
                guard
            }
        };
        audio.start()
    };
    if let Err(e) = start_result {
        release_capture(&state);
        emit_command_result(app_handle, "error", format!("Microphone error: {e}"));
        return;
    }

    // Audio is live. Honor a release that already landed during startup (a quick
    // tap), otherwise show the listening pill.
    mark_capture_live(&state);
    if take_startup_stop(&state) {
        stop_and_run_command(app_handle, &state).await;
    } else {
        let _ = app_handle.emit("command-state-change", "listening");
    }
}

/// Stop a command capture and run the recognized command. The hotkey hook
/// decides StopNow synchronously, then spawns this.
pub(crate) async fn stop_and_run_command(app_handle: &tauri::AppHandle, state: &AppState) {
    let _ = app_handle.emit("command-state-change", "recognizing");

    let mut samples = {
        let mut audio = match state.audio.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        match audio.stop() {
            Ok(s) => s,
            Err(e) => {
                release_capture(state);
                emit_command_result(app_handle, "error", format!("Microphone error: {e}"));
                return;
            }
        }
    };

    // Samples claimed — release capture ownership (held through audio.stop() so a
    // racing dictation start can't grab the mic mid-stop).
    release_capture(state);

    if samples.is_empty() {
        let _ = app_handle.emit("command-state-change", "idle");
        return;
    }
    crate::audio::normalize::normalize_peak(&mut samples);

    let engine = {
        let guard = match state.engine.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.as_ref().map(Arc::clone)
    };
    let Some(engine) = engine else {
        emit_command_result(app_handle, "error", "No speech model loaded");
        return;
    };

    let transcription =
        match tokio::task::spawn_blocking(move || engine.transcribe(&samples)).await {
            Ok(Ok(t)) => t,
            Ok(Err(e)) => {
                emit_command_result(app_handle, "error", format!("Transcription failed: {e}"));
                return;
            }
            Err(e) => {
                emit_command_result(app_handle, "error", format!("Transcription crashed: {e}"));
                return;
            }
        };

    let utterance = transcription.text.trim().to_string();

    // Fast path: deterministic grammar match (microseconds, no LLM).
    if let Some(intent) = crate::actions::match_command(&utterance) {
        run_intent(app_handle, state, intent).await;
        return;
    }

    // Slow path: free-form phrasing the grammar didn't catch → Qwen fallback.
    if let Some(intent) = classify_command_via_llm(app_handle, state, &utterance).await {
        run_intent(app_handle, state, intent).await;
        return;
    }

    let heard = if utterance.is_empty() {
        "nothing".to_string()
    } else {
        format!("\u{201c}{utterance}\u{201d}")
    };
    emit_command_result(app_handle, "error", format!("No command recognized ({heard})"));
}

/// Try the Qwen LLM fallback to interpret a free-form command the fast-path
/// matcher didn't recognize.  Reuses the Structured-Mode runner (lazy-loading
/// the configured LLM if none is loaded).  Returns `None` on any failure — the
/// caller then reports "unrecognized".
/// Get the loaded LLM runner, or lazily load the configured model.  Uses the
/// same model-selection chain as Structured Mode (active setting →
/// in-memory active id → preferred downloaded), so Command Mode never loads a
/// *different* model than the one the user activated — whichever loads first
/// becomes the shared `state.llm_runner`.  Returns None if nothing is
/// configured/available or the load fails.
fn ensure_llm_runner(state: &AppState) -> Option<Arc<crate::llm::runner::LlmRunner>> {
    if let Some(r) = state.llm_runner.lock().ok().and_then(|g| g.clone()) {
        return Some(r);
    }
    let id = crate::storage::settings::get_settings(&state.db)
        .ok()
        .and_then(|s| s.active_llm_model_id)
        .filter(|id| !id.is_empty())
        .or_else(|| state.active_llm_model_id.lock().ok().and_then(|g| g.clone()))
        .or_else(|| crate::commands::llm::preferred_downloaded_llm_id(state))?;
    match crate::commands::llm::load_and_activate_llm(&id, state) {
        Ok(()) => state.llm_runner.lock().ok().and_then(|g| g.clone()),
        Err(e) => {
            crate::llm::diaglog::log(&format!("command LLM lazy-load failed: {e}"));
            None
        }
    }
}

async fn classify_command_via_llm(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    utterance: &str,
) -> Option<crate::actions::CommandIntent> {
    if utterance.is_empty() {
        return None;
    }
    let _ = app_handle; // reserved for future "thinking" UI; keeps signature stable

    // First free-form command pays a one-time model load; subsequent ones are fast.
    let runner = ensure_llm_runner(state)?;

    let timeout = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.llm_timeout_secs)
        .unwrap_or(8);

    match runner
        .classify_command_with_timeout(utterance.to_string(), Duration::from_secs(timeout as u64))
        .await
    {
        Ok(opt) => opt,
        Err(e) => {
            crate::llm::diaglog::log(&format!("command classify failed: {e}"));
            None
        }
    }
}

async fn run_intent(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    intent: crate::actions::CommandIntent,
) {
    use crate::actions::CommandIntent;

    match intent {
        CommandIntent::OpenApp(name) => {
            // Resolve off the async runtime — a cold index spawns PowerShell, and
            // even warm scoring shouldn't run on a tokio worker.
            let lookup = name.clone();
            let resolved = tokio::task::spawn_blocking(move || {
                crate::actions::app_index::resolve(&lookup)
            })
            .await
            .ok()
            .flatten();
            match resolved {
                None => emit_command_result(
                    app_handle,
                    "error",
                    format!("No app found for \u{201c}{name}\u{201d}"),
                ),
                Some(r) if r.score >= crate::actions::app_index::AUTO && !r.ambiguous => {
                    match crate::actions::app_index::launch(&r.app_id) {
                        Ok(()) => {
                            emit_command_result(app_handle, "done", format!("Opened {}", r.name))
                        }
                        Err(e) => emit_command_result(app_handle, "error", e),
                    }
                }
                Some(r) => {
                    // Low confidence or ambiguous (close runner-up) — ask first.
                    if let Ok(mut pending) = state.pending_command.lock() {
                        *pending = Some(crate::state::PendingCommand {
                            app_id: r.app_id,
                            name: r.name.clone(),
                        });
                    }
                    let _ = app_handle.emit(
                        "command-confirm",
                        &CommandConfirmPayload {
                            summary: format!("Open {}?", r.name),
                        },
                    );
                }
            }
        }
        // Foreground keystroke/media actions: restore the user's window first,
        // then fire the keys at it.
        CommandIntent::KeyChord(chord) => {
            let hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
            spawn_report(app_handle, chord.past_tense(), move || {
                if let Some(h) = hwnd {
                    crate::focus::restore_foreground_window_public(h);
                }
                crate::actions::executor::run_chord(chord)
            })
            .await;
        }
        CommandIntent::Media(action) => {
            let hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
            spawn_report(app_handle, action.label(), move || {
                if let Some(h) = hwnd {
                    crate::focus::restore_foreground_window_public(h);
                }
                crate::actions::executor::run_media(action)
            })
            .await;
        }
        CommandIntent::Window(action) => {
            let hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
            spawn_report(app_handle, action.label(), move || {
                crate::actions::executor::run_window(action, hwnd)
            })
            .await;
        }
        // Browser actions open in the default browser — no focus restore needed.
        CommandIntent::WebSearch(query) => {
            spawn_report(app_handle, "Web search", move || {
                crate::actions::executor::run_web_search(&query)
            })
            .await;
        }
        CommandIntent::OpenUrl(url) => {
            spawn_report(app_handle, "Opened link", move || {
                crate::actions::executor::run_open_url(&url)
            })
            .await;
        }
    }
}

/// Run a blocking command action off the async runtime and emit its result to
/// the command pill.  `label` is the success summary ("Copied", "Web search").
async fn spawn_report(
    app_handle: &tauri::AppHandle,
    label: &str,
    f: impl FnOnce() -> Result<(), String> + Send + 'static,
) {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(())) => emit_command_result(app_handle, "done", label.to_string()),
        Ok(Err(e)) => emit_command_result(app_handle, "error", e),
        Err(_) => emit_command_result(app_handle, "error", "Command execution failed"),
    }
}

/// Execute the pending (confirmed) command — called by the `confirm_command`
/// Tauri command when the user accepts a low-confidence app match.
pub fn confirm_pending_command(app_handle: &tauri::AppHandle, state: &AppState) {
    let pending = state
        .pending_command
        .lock()
        .ok()
        .and_then(|mut g| g.take());
    let Some(p) = pending else {
        return;
    };
    match crate::actions::app_index::launch(&p.app_id) {
        Ok(()) => emit_command_result(app_handle, "done", format!("Opened {}", p.name)),
        Err(e) => emit_command_result(app_handle, "error", e),
    }
}

/// Clear a pending command (user cancelled the confirm).
pub fn cancel_pending_command(app_handle: &tauri::AppHandle, state: &AppState) {
    if let Ok(mut g) = state.pending_command.lock() {
        *g = None;
    }
    let _ = app_handle.emit("command-state-change", "idle");
}
