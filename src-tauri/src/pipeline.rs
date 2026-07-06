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
    /// Characters dropped from the LLM input because the dictation exceeded
    /// `STRUCTURED_INPUT_CHAR_CAP`. 0 when nothing was truncated — the panel
    /// shows a warning and points at "Raw" (which always has the full text).
    truncated_chars: usize,
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
    crate::llm::diaglog::log(&format!(
        "pipeline: start_recording_inner fg={:?} fg_proc={:?}",
        fg,
        fg.and_then(get_process_name_from_hwnd)
    ));
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

    // Structured Mode: re-warm the LLM session while the user speaks.  The
    // runner drops its KV cache after 5 idle minutes; rebuilding it here
    // overlaps the multi-second prefill with the utterance instead of paying
    // it on the extraction's critical path.  No-op when already warm.
    if settings.as_ref().map(|s| s.structured_mode).unwrap_or(false) {
        if let Some(runner) = state.llm_runner.lock().ok().and_then(|g| g.clone()) {
            runner.prewarm();
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

            // Async snapshot task — samples the trailing audio ~once a second
            // and forwards it to the worker.  When recording stops it returns,
            // dropping tx_audio and cleanly terminating the worker thread.
            //
            // Cadence note: the old 3 s first-snapshot + 3 s loop meant a
            // dictation shorter than ~4 s showed NOTHING (the worker only emits
            // while still recording), and longer ones lurched forward in big 3 s
            // jumps.  A short warm-up + ~1 s cadence makes words appear quickly
            // and stream in small, smooth increments (each tick re-decodes the
            // trailing window, so a small slide ≈ a near-incremental update).
            let ctrl_handle = app_handle.clone();
            tauri::async_runtime::spawn(async move {
                use std::sync::atomic::Ordering;
                const PREVIEW_SAMPLES: usize = 16_000 * 5;

                // Short warm-up so the very first words show ~1 s in instead of 3 s+.
                tokio::time::sleep(std::time::Duration::from_millis(600)).await;

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

                    // ~1 s cadence: frequent, small updates read as smooth
                    // streaming. Capacity-1 backpressure (above) keeps it from
                    // piling up if a decode runs longer than the interval.
                    tokio::time::sleep(std::time::Duration::from_millis(900)).await;
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
        crate::llm::diaglog::log("pipeline: emitting recording-state-change=recording");
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
        // Instrumented (audit 2026-07-06): the v0.3.1 abort callback should
        // make the real drain far shorter than the 1500ms ceiling — log the
        // measured wait so the timeout can be tightened on evidence.
        let t0 = std::time::Instant::now();
        match tokio::time::timeout(Duration::from_millis(1500), rx).await {
            Ok(Ok(())) | Ok(Err(_)) => {
                crate::llm::diaglog::log(&format!(
                    "pipeline: preview worker drained in {}ms",
                    t0.elapsed().as_millis()
                ));
            }
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
            match crate::commands::llm::load_and_activate_llm_with_status(
                &model_id,
                state,
                Some(app_handle),
            ) {
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

    // Sized together with LlmConfig::default().n_ctx: 4,000 chars ≈ 1,100
    // tokens of input on top of the ~1,900-token system prompt and 384-token
    // output budget.  Anything beyond the cap is dropped from the LLM input
    // (never from the raw transcript) and surfaced to the user via
    // `truncated_chars` on the structured payload.
    const STRUCTURED_INPUT_CHAR_CAP: usize = 4000;
    let total_chars = processed_text.chars().count();
    let truncated_chars = total_chars.saturating_sub(STRUCTURED_INPUT_CHAR_CAP);
    let structured_input = if truncated_chars > 0 {
        let clipped: String = processed_text
            .chars()
            .take(STRUCTURED_INPUT_CHAR_CAP)
            .collect();
        crate::llm::diaglog::log(&format!(
            "pipeline: truncating structured input from {total_chars} to {STRUCTURED_INPUT_CHAR_CAP} chars"
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
                    crate::llm::diaglog::record(crate::llm::diaglog::ExtractionRecord {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                        input_chars: structured_input.chars().count(),
                        truncated_chars,
                        output_chars: md.chars().count(),
                        outcome: "ok".into(),
                    });
                    Some((md, slots))
                }
                Err(e) => {
                    crate::llm::diaglog::log(&format!(
                        "pipeline: extraction FAILED after {}ms: {e}",
                        t0.elapsed().as_millis()
                    ));
                    crate::llm::diaglog::record(crate::llm::diaglog::ExtractionRecord {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                        input_chars: structured_input.chars().count(),
                        truncated_chars,
                        output_chars: 0,
                        outcome: format!("Extraction failed: {e}"),
                    });
                    let _ = app_handle.emit(
                        "structured-mode-degraded",
                        &format!("Extraction failed: {e}"),
                    );
                    None
                }
            }
        } else {
            crate::llm::diaglog::record(crate::llm::diaglog::ExtractionRecord {
                timestamp: chrono::Utc::now().to_rfc3339(),
                duration_ms: 0,
                input_chars: structured_input.chars().count(),
                truncated_chars,
                output_chars: 0,
                outcome: "No LLM model available".into(),
            });
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
    // Build the effective (enabled) command table from the user's registry.
    // Falls back to the built-in defaults if the DB read fails. When Command
    // Send is off we drop the Send command so a trailing "send" stays literal.
    let voice_command_table = if voice_commands_enabled && structured.is_none() {
        let mut table = crate::storage::voice_commands::list_enabled(&state.db)
            .unwrap_or_else(|_| crate::postprocess::voice_commands::default_command_table());
        if !command_send_enabled {
            table.retain(|d| {
                !matches!(
                    d.command,
                    crate::postprocess::voice_commands::VoiceCommand::Send
                )
            });
        }
        Some(table)
    } else {
        None
    };
    // Resolve spoken list markers ("bullet point", "number item", "end list")
    // into literal `- ` / `1. ` text and merge adjacent text segments so a
    // dictated list lands as one atomic paste. Item capitalization follows
    // the writing style (VeryCasual stays lowercase).
    let capitalize_items = settings
        .as_ref()
        .map(|s| s.writing_style != "very_casual")
        .unwrap_or(true);
    let voice_segments = voice_command_table.as_ref().map(|table| {
        let segments =
            crate::postprocess::voice_commands::parse_commands_with_table(&final_text, table);
        crate::postprocess::voice_commands::resolve_list_segments(segments, capitalize_items)
    });

    // 5. Kick off focus restoration in parallel with output.
    //     Skipped for Structured Mode since the panel handles pasting.
    let prev_hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);

    // Was the dictation aimed at one of OmniVox's own windows?  A synthetic
    // Ctrl+V doesn't reliably land in our focused WebView2 input, so for our
    // own windows we insert the text via the frontend (DOM caret insertion)
    // instead of OS paste — and we skip focus restoration since our window is
    // already foreground.
    let target_is_self = prev_hwnd.map(crate::focus::hwnd_is_own_process).unwrap_or(false);

    let focus_task = if structured.is_none() && !target_is_self {
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
        if target_is_self {
            // Dictating into OmniVox itself — hand the text to the focused
            // window so it can insert at the caret of whatever field the user
            // is in.  (OS Ctrl+V into our own WebView2 control doesn't land.)
            let _ = app_handle.emit("dictation-insert", &final_text);
        } else {
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
    }

    // 6b. Ship Mode — automatically press Enter to send the message.
    //     Only fires when type simulation was used (clipboard-only can't auto-send).
    //     When Command Send is enabled it overrides Ship Mode — the user controls
    //     sending by saying "send" at the end, so we skip the automatic Enter.
    //     Also skipped in Structured Mode — pasting is user-driven from the panel.
    //     Derived from the effective table (not just the setting): if the user
    //     disabled the Send command, Ship Mode auto-send stays available.
    let command_send_active = voice_command_table
        .as_ref()
        .map(|t| {
            t.iter().any(|d| {
                matches!(
                    d.command,
                    crate::postprocess::voice_commands::VoiceCommand::Send
                )
            })
        })
        .unwrap_or(false);
    if structured.is_none()
        && !target_is_self
        && output_config.ship_mode
        && !command_send_active
        && matches!(
            output_config.mode,
            crate::output::types::OutputMode::TypeSimulation
                | crate::output::types::OutputMode::Both
        )
    {
        let _ = tokio::task::spawn_blocking(|| {
            // The router's send/send_segments are synchronous and already
            // include the 250ms post-paste guard, so by this point the paste
            // keystroke has been delivered and the clipboard held stable.
            // This settle only covers the target app *processing* its Ctrl+V
            // before Enter arrives.  600ms (850ms total after paste) is sized
            // for Ship Mode's actual targets — Electron chat/agent UIs, which
            // process paste on a renderer tick — while still 900ms faster
            // than the old blind 1500ms.  Native edit controls need far less.
            std::thread::sleep(std::time::Duration::from_millis(600));
            if let Ok(mut enigo) = enigo::Enigo::new(&enigo::Settings::default()) {
                let _ =
                    enigo::Keyboard::key(&mut enigo, enigo::Key::Return, enigo::Direction::Click);
            }
        })
        .await;
    }

    // 7. Save to history.
    //     `text` is the final paste-ready string (Markdown in Structured
    //     Mode, plain text otherwise).  `raw_transcript` stores the
    //     pre-processor ASR text so the Structured panel's "View raw"
    //     disclosure always reflects what the user actually spoke.
    //     This happens BEFORE the `transcription-result` emit so listeners
    //     that re-query history (the History page auto-refresh) see the
    //     new row without needing a settle timer.
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

    // 8. Notify frontend of the result.
    //
    //    `transcription-result` always fires — History auto-refresh, the
    //    global last-transcription store, and Notes-append all listen for
    //    it, so skipping it on the Structured path would silently break
    //    those flows.  For Structured Mode we also emit the rich payload
    //    so the overlay can render the preview panel.
    let _ = app_handle.emit("transcription-result", &record.text);
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
                truncated_chars,
            },
        );
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
    // chords / target window actions at it.  Use the command-specific capture
    // that skips OmniVox's own windows — otherwise, when OmniVox is the
    // foreground app (the user just clicked it), the target would be our own UI
    // and focus-dependent commands (copy / minimize / media) would no-op.
    let fg = crate::focus::capture_command_target_window();
    if let Ok(mut prev) = state.prev_foreground.lock() {
        *prev = fg;
    }
    if let Ok(mut pending) = state.pending_command.lock() {
        *pending = None;
    }

    // Scope the audio guard so it's provably dropped before any `.await` below
    // (a MutexGuard isn't Send, and this fn is spawned as a Send future).
    let (start_result, is_recording, rms_level) = {
        let mut audio = match state.audio.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                let mut guard = poisoned.into_inner();
                guard.cancel();
                guard
            }
        };
        let r = audio.start();
        (r, audio.is_recording_flag(), audio.rms_level_ref())
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
        // Drive the command pill's volume waveform: emit `audio-level` events
        // while listening, mirroring the dictation path.  Without this the
        // command waveform sits flat because nothing publishes the mic level
        // during a command capture (the emitter lived only in start_recording_inner).
        let handle = app_handle.clone();
        tauri::async_runtime::spawn(async move {
            use std::sync::atomic::Ordering;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                if !is_recording.load(Ordering::Relaxed) {
                    break;
                }
                let level = f32::from_bits(rms_level.load(Ordering::Relaxed));
                let _ = handle.emit("audio-level", level);
            }
        });
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
    // The LLM may interpret one utterance as a multi-step chain.
    let intents = classify_command_via_llm(app_handle, state, &utterance).await;
    if !intents.is_empty() {
        // A submitting type_text (send_message) presses Enter in another app —
        // never fire that blind. Park the WHOLE sequence and route it through
        // the same Enter/Esc confirm pill as OpenApp/CloseWindow; the chain
        // runs only after the user accepts.
        if intents
            .iter()
            .any(|i| matches!(i, crate::actions::CommandIntent::TypeText { submit: true, .. }))
        {
            let summary = confirm_chain_summary(state, &intents);
            if let Ok(mut pending) = state.pending_command.lock() {
                *pending = Some(crate::state::PendingCommand::Chain { intents });
            }
            let _ = app_handle.emit("command-confirm", &CommandConfirmPayload { summary });
            return;
        }
        if intents.len() == 1 {
            run_intent(app_handle, state, intents.into_iter().next().unwrap()).await;
        } else {
            run_chain(app_handle, state, intents).await;
        }
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
fn ensure_llm_runner(
    app_handle: &tauri::AppHandle,
    state: &AppState,
) -> Option<Arc<crate::llm::runner::LlmRunner>> {
    if let Some(r) = state.llm_runner.lock().ok().and_then(|g| g.clone()) {
        return Some(r);
    }
    let id = crate::storage::settings::get_settings(&state.db)
        .ok()
        .and_then(|s| s.active_llm_model_id)
        .filter(|id| !id.is_empty())
        .or_else(|| state.active_llm_model_id.lock().ok().and_then(|g| g.clone()))
        .or_else(|| crate::commands::llm::preferred_downloaded_llm_id(state))?;
    match crate::commands::llm::load_and_activate_llm_with_status(&id, state, Some(app_handle)) {
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
) -> Vec<crate::actions::CommandIntent> {
    if utterance.is_empty() {
        return Vec::new();
    }
    // First free-form command pays a one-time model load; subsequent ones are fast.
    let runner = match ensure_llm_runner(app_handle, state) {
        Some(r) => r,
        None => return Vec::new(),
    };

    let timeout = crate::storage::settings::get_settings(&state.db)
        .map(|s| s.llm_timeout_secs)
        .unwrap_or(8);

    match runner
        .classify_command_with_timeout(utterance.to_string(), Duration::from_secs(timeout as u64))
        .await
    {
        Ok(intents) => intents,
        Err(e) => {
            crate::llm::diaglog::log(&format!("command classify failed: {e}"));
            Vec::new()
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
        // `OpenApp` keeps the interactive confirm flow for low-confidence/
        // ambiguous matches.
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
                        *pending = Some(crate::state::PendingCommand::OpenApp {
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
        // Consequential — never fire blind. Stash the captured window + title and
        // route through the same Enter/Esc confirm pill that OpenApp uses.
        CommandIntent::CloseWindow => {
            let hwnd = state.prev_foreground.lock().ok().and_then(|g| *g);
            match hwnd {
                None => emit_command_result(app_handle, "error", "No window to close"),
                Some(h) => {
                    let title = crate::actions::executor::window_title(h);
                    if let Ok(mut pending) = state.pending_command.lock() {
                        *pending = Some(crate::state::PendingCommand::CloseWindow {
                            hwnd: h,
                            title: title.clone(),
                        });
                    }
                    let summary = if title.trim().is_empty() {
                        "Close this window?".to_string()
                    } else {
                        format!("Close \u{201c}{title}\u{201d}?")
                    };
                    let _ = app_handle
                        .emit("command-confirm", &CommandConfirmPayload { summary });
                }
            }
        }
        // Everything else is fire-and-report via the shared no-confirm executor.
        other => {
            let target = state.prev_foreground.lock().ok().and_then(|g| *g);
            let (summary, ok) = execute_intent_now(target, other).await;
            emit_command_result(app_handle, if ok { "done" } else { "error" }, summary);
        }
    }
}

/// Execute a single intent against `target_hwnd` — the window to restore focus
/// to before a keystroke/window action (`None` = act on whatever is currently
/// foreground) — and return its `(summary, ok)` result instead of emitting to
/// the pill.  Unlike `run_intent`, `OpenApp` never prompts here: a chain (and
/// the single-intent delegation) must not pause mid-sequence, so an app only
/// launches on a confident, unambiguous match.
async fn execute_intent_now(
    target_hwnd: Option<isize>,
    intent: crate::actions::CommandIntent,
) -> (String, bool) {
    use crate::actions::CommandIntent;

    match intent {
        CommandIntent::OpenApp(name) => {
            let lookup = name.clone();
            let resolved =
                tokio::task::spawn_blocking(move || crate::actions::app_index::resolve(&lookup))
                    .await
                    .ok()
                    .flatten();
            match resolved {
                Some(r) if r.score >= crate::actions::app_index::AUTO && !r.ambiguous => {
                    match crate::actions::app_index::launch(&r.app_id) {
                        Ok(()) => (format!("Opened {}", r.name), true),
                        Err(e) => (e, false),
                    }
                }
                // An app resolved but wasn't confident/unambiguous enough to
                // auto-launch.  In a chain there's no confirm prompt, so report
                // it as a skip rather than the misleading "no app found".
                Some(_) => (
                    format!("Not sure which app you meant by \u{201c}{name}\u{201d}"),
                    false,
                ),
                None => (format!("No app found for \u{201c}{name}\u{201d}"), false),
            }
        }
        // Foreground keystroke action: restore the target window first, then
        // fire the chord at it (enigo hits whatever has focus).
        CommandIntent::KeyChord(chord) => {
            run_blocking(chord.past_tense(), move || {
                if let Some(h) = target_hwnd {
                    crate::focus::restore_foreground_window_public(h);
                }
                crate::actions::executor::run_chord(chord)
            })
            .await
        }
        // Media/volume keys are global (system-wide) — they ignore focus, so we
        // do NOT restore the target window (avoids yanking focus around for a
        // key that doesn't need it).
        CommandIntent::Media(action) => {
            run_blocking(action.label(), move || {
                crate::actions::executor::run_media(action)
            })
            .await
        }
        CommandIntent::Window(action) => {
            run_blocking(action.label(), move || {
                crate::actions::executor::run_window(action, target_hwnd)
            })
            .await
        }
        // Browser actions open in the default browser — no focus restore needed.
        CommandIntent::WebSearch(query) => {
            run_blocking("Web search", move || {
                crate::actions::executor::run_web_search(&query)
            })
            .await
        }
        CommandIntent::OpenUrl(url) => {
            run_blocking("Opened link", move || {
                crate::actions::executor::run_open_url(&url)
            })
            .await
        }
        // CloseWindow is consequential and must be confirmed — `run_intent`
        // handles the single-intent case interactively.  A chain never pauses to
        // confirm, so closing a window inside one is refused (this aborts the
        // rest of the chain rather than closing a window the user didn't OK).
        CommandIntent::CloseWindow => (
            "Closing a window isn't supported inside a multi-step command".to_string(),
            false,
        ),
        // Focus-dependent like a key chord: restore the target window, then
        // paste (and, for a confirmed send, press Enter).  A submitting
        // TypeText only ever reaches here after the user accepted the confirm
        // pill — the classify path parks it as a PendingCommand::Chain first.
        CommandIntent::TypeText { text, submit } => {
            let label = if submit { "Sent message" } else { "Typed text" };
            run_blocking(label, move || {
                if let Some(h) = target_hwnd {
                    crate::focus::restore_foreground_window_public(h);
                }
                crate::actions::executor::run_type_text(&text, submit)
            })
            .await
        }
    }
}

/// Human summary for the confirm pill when a classified sequence contains a
/// submitting type_text.  A lone send names the window it will land in
/// ("Send “fix the bug” to “Claude”?"); a chain narrates its steps
/// ("Open Claude, then send “fix the bug”?").
fn confirm_chain_summary(state: &AppState, intents: &[crate::actions::CommandIntent]) -> String {
    use crate::actions::CommandIntent::*;

    if let [TypeText { text, submit: true }] = intents {
        let title = state
            .prev_foreground
            .lock()
            .ok()
            .and_then(|g| *g)
            .map(crate::actions::executor::window_title)
            .filter(|t| !t.trim().is_empty());
        return match title {
            Some(t) => format!("Send \u{201c}{text}\u{201d} to \u{201c}{t}\u{201d}?"),
            None => format!("Send \u{201c}{text}\u{201d}?"),
        };
    }

    let parts: Vec<String> = intents
        .iter()
        .map(|i| match i {
            OpenApp(name) => format!("open {name}"),
            KeyChord(k) => k.past_tense().to_lowercase(),
            Media(m) => m.label().to_lowercase(),
            Window(w) => w.label().to_lowercase(),
            WebSearch(q) if q.trim().is_empty() => "open a web search".to_string(),
            WebSearch(q) => format!("search for \u{201c}{q}\u{201d}"),
            OpenUrl(u) => format!("open {u}"),
            CloseWindow => "close the window".to_string(),
            TypeText { text, submit: true } => format!("send \u{201c}{text}\u{201d}"),
            TypeText { text, submit: false } => format!("type \u{201c}{text}\u{201d}"),
        })
        .collect();
    let mut s = parts.join(", then ");
    if let Some(first) = s.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    format!("{s}?")
}

/// Execute a multi-step chain sequentially, settling briefly between steps so a
/// just-launched app can take focus before the next keystroke lands.  Emits one
/// combined result to the pill ("Opened Spotify · Play/Pause").
async fn run_chain(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    intents: Vec<crate::actions::CommandIntent>,
) {
    use crate::actions::CommandIntent;

    let total = intents.len();
    let mut summaries = Vec::with_capacity(total);
    let mut all_ok = true;

    // Focus target for keystroke/window steps.  Starts as the window that was
    // foreground before command capture; once a step launches an app we re-aim
    // at that app (now foreground) so "open notepad and paste" targets notepad,
    // not the window the user spoke from.
    let mut target = state.prev_foreground.lock().ok().and_then(|g| *g);

    // OmniVox's own windows — never re-aim a chain's keystrokes at our own UI if
    // the overlay/main happens to be foreground during the launch settle.
    let own = own_window_hwnds(app_handle);

    // Set when a launch did NOT confirm the new app took the foreground, so
    // `target` can't be trusted for a focus-dependent step.
    let mut target_unverified = false;

    for (i, intent) in intents.into_iter().enumerate() {
        // A focus-dependent step (key chord / window action) after an
        // unverified launch would fire into the window the user spoke from, not
        // the app they just opened — refuse it rather than risk a stray
        // paste/save/close-tab.  Global media keys don't need a window, so they
        // are exempt and keep going.
        let needs_focus = matches!(
            intent,
            CommandIntent::KeyChord(_)
                | CommandIntent::Window(_)
                | CommandIntent::TypeText { .. }
        );
        if target_unverified && needs_focus {
            summaries.push("couldn't confirm the launched app's window".to_string());
            all_ok = false;
            break;
        }

        let launched_app = matches!(intent, CommandIntent::OpenApp(_));
        let (summary, ok) = execute_intent_now(target, intent).await;
        summaries.push(summary);

        // Abort the rest of the chain on a failed step.  Later steps almost
        // always depend on this one (keystrokes meant for an app that didn't
        // open), and firing them blind would hit the wrong window — e.g. a
        // failed "open notepad" must NOT be followed by "paste" into whatever
        // the user was looking at.
        if !ok {
            all_ok = false;
            break;
        }

        if i + 1 < total && launched_app {
            // Confirm the launched app actually took the foreground before
            // aiming later steps at it.  Verified (a new non-own window) →
            // retarget; unverified (app already foreground, or too slow) → keep
            // the old target but flag it so a following focus-dependent step
            // won't fire blind.
            match settle_after_launch(target, &own).await {
                Some(hwnd) => {
                    target = Some(hwnd);
                    target_unverified = false;
                }
                None => target_unverified = true,
            }
        } else if i + 1 < total {
            // Brief settle so the prior action lands before the next fires.
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    emit_command_result(
        app_handle,
        if all_ok { "done" } else { "error" },
        summaries.join(" \u{00b7} "),
    );
}

/// HWNDs of OmniVox's own windows (main + overlay), so a chain never re-aims a
/// launched-app focus retarget at our own UI.
#[cfg(target_os = "windows")]
fn own_window_hwnds(app_handle: &tauri::AppHandle) -> Vec<isize> {
    app_handle
        .webview_windows()
        .values()
        .filter_map(|w| w.hwnd().ok().map(|h| h.0 as isize))
        .collect()
}

#[cfg(not(target_os = "windows"))]
fn own_window_hwnds(_app_handle: &tauri::AppHandle) -> Vec<isize> {
    Vec::new()
}

/// Poll (up to ~2.4s) for a freshly launched app to take the foreground.
/// Returns `Some(hwnd)` as soon as a window that is NOT `prev` and NOT one of
/// OmniVox's own windows (`own`) becomes foreground — a verified *non-own*
/// foreground change, presumed (but not proven — we have no launched-app
/// identity) to be the new app.  Returns `None` if focus never moves there (app
/// was already foreground, or is too slow): the caller then cannot trust a
/// retarget and must not fire a focus-dependent step blind.
async fn settle_after_launch(prev: Option<isize>, own: &[isize]) -> Option<isize> {
    for _ in 0..16 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        if let Some(h) = capture_foreground_window() {
            if Some(h) != prev && !own.contains(&h) {
                return Some(h);
            }
        }
    }
    None
}

/// Run a blocking command action off the async runtime and return a
/// `(summary, ok)` pair.  `label` is the success summary ("Copied", "Web
/// search"); on failure the error string is returned instead.
async fn run_blocking(
    label: &str,
    f: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> (String, bool) {
    match tokio::task::spawn_blocking(f).await {
        Ok(Ok(())) => (label.to_string(), true),
        Ok(Err(e)) => (e, false),
        Err(_) => ("Command execution failed".to_string(), false),
    }
}

/// Execute the pending (confirmed) command — called by the `confirm_command`
/// Tauri command when the user accepts a low-confidence app match, a window
/// close, or a chain containing a message send.
pub async fn confirm_pending_command(app_handle: &tauri::AppHandle, state: &AppState) {
    let pending = state
        .pending_command
        .lock()
        .ok()
        .and_then(|mut g| g.take());
    let Some(p) = pending else {
        return;
    };
    match p {
        crate::state::PendingCommand::OpenApp { app_id, name } => {
            match crate::actions::app_index::launch(&app_id) {
                Ok(()) => emit_command_result(app_handle, "done", format!("Opened {name}")),
                Err(e) => emit_command_result(app_handle, "error", e),
            }
        }
        crate::state::PendingCommand::CloseWindow { hwnd, title } => {
            let label = if title.trim().is_empty() {
                "Closed window".to_string()
            } else {
                format!("Closed {title}")
            };
            match crate::actions::executor::run_close_window(Some(hwnd)) {
                Ok(()) => emit_command_result(app_handle, "done", label),
                Err(e) => emit_command_result(app_handle, "error", e),
            }
        }
        crate::state::PendingCommand::Chain { intents } => {
            // The user accepted the whole sequence up front, so the chain
            // runner may now execute its submitting type_text steps.
            run_chain(app_handle, state, intents).await;
        }
    }
}

/// Clear a pending command (user cancelled the confirm).
pub fn cancel_pending_command(app_handle: &tauri::AppHandle, state: &AppState) {
    if let Ok(mut g) = state.pending_command.lock() {
        *g = None;
    }
    let _ = app_handle.emit("command-state-change", "idle");
}

/// Result of a "Test command" dry-run (Models → Command tab). Reports how the
/// two-tier brain resolved an utterance WITHOUT executing anything.
#[derive(Clone, serde::Serialize)]
pub struct CommandTestResult {
    /// "matcher" (tier-1 instant), "llm" (Qwen fallback), or "none".
    pub tier: &'static str,
    pub recognized: bool,
    /// Human-readable resolved intent ("Open app: Spotify", "Close current window…").
    pub summary: String,
    pub duration_ms: u64,
}

fn describe_intent(intent: &crate::actions::CommandIntent) -> String {
    use crate::actions::CommandIntent::*;
    match intent {
        OpenApp(name) => format!("Open app: {name}"),
        KeyChord(k) => format!("Key chord: {}", k.past_tense()),
        Media(m) => format!("Media: {}", m.label()),
        Window(w) => format!("Window: {}", w.label()),
        WebSearch(q) if q.trim().is_empty() => "Web search (open search page)".to_string(),
        WebSearch(q) => format!("Web search: {q}"),
        OpenUrl(u) => format!("Open URL: {u}"),
        CloseWindow => "Close current window (will ask to confirm)".to_string(),
        TypeText { text, submit: false } => format!("Type: \u{201c}{text}\u{201d}"),
        TypeText { text, submit: true } => {
            format!("Send message: \u{201c}{text}\u{201d} (will ask to confirm)")
        }
    }
}

/// Dry-run an utterance through the SAME two-tier path Command Mode uses
/// (deterministic matcher → Qwen fallback) but DON'T execute it — powers the
/// "Test command" box so the user can see how the brain parses a phrase.
pub async fn test_command(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    utterance: &str,
) -> CommandTestResult {
    let utt = utterance.trim();
    let t0 = std::time::Instant::now();
    if utt.is_empty() {
        return CommandTestResult {
            tier: "none",
            recognized: false,
            summary: "Nothing to test".into(),
            duration_ms: 0,
        };
    }
    // Tier 1 — deterministic matcher (microseconds, no model load).
    if let Some(intent) = crate::actions::match_command(utt) {
        return CommandTestResult {
            tier: "matcher",
            recognized: true,
            summary: describe_intent(&intent),
            duration_ms: t0.elapsed().as_millis() as u64,
        };
    }
    // Tier 2 — Qwen fallback (lazy-loads the active LLM on first use).  The LLM
    // may return a multi-step chain; describe each step joined for the preview.
    let intents = classify_command_via_llm(app_handle, state, utt).await;
    if !intents.is_empty() {
        let summary = intents
            .iter()
            .map(describe_intent)
            .collect::<Vec<_>>()
            .join(" \u{00b7} ");
        return CommandTestResult {
            tier: "llm",
            recognized: true,
            summary,
            duration_ms: t0.elapsed().as_millis() as u64,
        };
    }
    CommandTestResult {
        tier: "none",
        recognized: false,
        summary: "Not a recognized command".into(),
        duration_ms: t0.elapsed().as_millis() as u64,
    }
}
