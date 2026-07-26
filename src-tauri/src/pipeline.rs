use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};
use tokio::sync::oneshot;

use crate::asr::engine::AsrEngine;
use crate::error::ErrorCode;
use crate::focus::{
    capture_foreground_window, get_process_name_from_hwnd, restore_foreground_window,
};
use crate::llm::profiles::ProfileOutput;
use crate::postprocess::processor::TextProcessor;
use crate::screen_context::ScreenContext;
use crate::state::AppState;

/// Payload emitted on `structured-output-ready` so the overlay can render the
/// panel and offer Paste / Copy / Edit / Dismiss actions.
#[derive(Clone, serde::Serialize)]
struct StructuredOutputPayload {
    markdown: String,
    /// Profile-specific slot object — `SlotExtraction`-shaped for the
    /// default agent-prompt profile, email/notes shapes for the others.
    slots: serde_json::Value,
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
/// Which OmniVox window owns `hwnd` (the HWND snapshotted at record start), if
/// any — used to scope in-app dictation delivery to the exact target window
/// without a frontend focus race.
#[cfg(windows)]
fn window_label_for_hwnd(app: &tauri::AppHandle, hwnd: isize) -> Option<String> {
    use tauri::Manager;
    app.webview_windows()
        .into_iter()
        .find(|(_, w)| w.hwnd().ok().map(|h| h.0 as isize) == Some(hwnd))
        .map(|(label, _)| label)
}

#[cfg(not(windows))]
fn window_label_for_hwnd(_app: &tauri::AppHandle, _hwnd: isize) -> Option<String> {
    None
}

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
    // Bind the dictation target's identity (hwnd + owning pid) NOW, at capture —
    // an inline consequential voice command re-verifies against this, and
    // capturing the pid here (not at output time) means a HWND recycled to a
    // different process before output fails identity instead of passing (B2-3).
    if let Ok(mut t) = state.dictation_target.lock() {
        *t = fg.map(|h| crate::focus::WindowTarget {
            hwnd: h,
            pid: crate::focus::pid_for_hwnd(h),
        });
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
        if let Some(runner) = state
            .llm_runner
            .lock()
            .ok()
            .and_then(|g| g.as_ref().map(|(_, r)| Arc::clone(r)))
        {
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
        // Restore system volume — we ducked above but never got to record, so
        // the stop path that normally un-ducks won't run. Unconditional: unduck
        // take()s the saved level and no-ops when nothing was ducked, and it
        // also clears any duck leaked by a prior aborted start.
        crate::audio::ducking::unduck();
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

    // Snapshot THIS dictation's bound target (hwnd + owning pid) BEFORE we
    // release capture — the instant we do, an overlapping dictation can start
    // and overwrite the shared `dictation_target`/`prev_foreground` slots.  This
    // stopping dictation must send its text into the window IT targeted, so we
    // carry the snapshot through transcription → output rather than re-reading
    // the shared slot late (B2-11).
    let dictation_target = state.dictation_target.lock().ok().and_then(|g| *g);

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

    // Route this dictation to the scratchpad when EITHER its window was the
    // record-start foreground, OR the scratchpad is open with capture on (so you
    // can read in one window and dictate answers into the pad). Gated on the
    // window being visible, so a closed pad never hijacks normal dictation.
    // Either way it's PLAIN capture — never Structured Mode, which yields a slot
    // panel, not text to append.
    let route_to_scratchpad = {
        use tauri::Manager;
        let foreground_is_scratchpad = dictation_target
            .and_then(|t| window_label_for_hwnd(app_handle, t.hwnd))
            .as_deref()
            == Some("scratchpad");
        let capturing = state
            .scratchpad_capture
            .load(std::sync::atomic::Ordering::Acquire)
            && app_handle
                .get_webview_window("scratchpad")
                .and_then(|w| w.is_visible().ok())
                .unwrap_or(false);
        foreground_is_scratchpad || capturing
    };

    // Resolve whether the LLM should run for THIS utterance.  With the
    // gate on, it's an explicit opt-in per utterance.  With the gate off,
    // the global setting governs (every qualifying transcription runs).
    let should_structure =
        structured_enabled && (!voice_command_gate || voxify_said) && !route_to_scratchpad;

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
        if let Some(model_id) = configured_llm_id.clone() {
            // Always resolve through the KEYED single-flight loader — never a
            // bare "use whatever runner is loaded" fast path (B2-16): its own
            // fast path (`runner_for_model`) returns the loaded runner ONLY when
            // it is this exact model, otherwise it (re)loads the configured one,
            // so a concurrent switch can't feed a different model's runner into
            // this extraction.  The load holds LLM_LOAD_LOCK across a blocking
            // GGUF load + thread join, so it runs on the blocking pool — never on
            // this tokio worker (B2-8).
            crate::llm::diaglog::log(&format!("runner: resolving '{model_id}' (keyed)"));
            let app = app_handle.clone();
            let mid = model_id.clone();
            let loaded = tokio::task::spawn_blocking(move || {
                let st = app.state::<AppState>();
                crate::commands::llm::ensure_runner_loaded(&mid, &st, Some(&app))
            })
            .await;
            match loaded {
                Ok(Ok(r)) => {
                    crate::llm::diaglog::log("runner: resolve ok");
                    Some(r)
                }
                Ok(Err(e)) => {
                    crate::llm::diaglog::log(&format!("runner: resolve FAILED: {e}"));
                    let _ =
                        app_handle.emit("structured-mode-degraded", &format!("Load failed: {e}"));
                    None
                }
                Err(e) => {
                    crate::llm::diaglog::log(&format!("runner: resolve task failed: {e}"));
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

    let structured: Option<ProfileOutput> = if should_structure
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
                Ok(out) => {
                    crate::llm::diaglog::log(&format!(
                        "pipeline: extraction OK in {}ms slots={}",
                        t0.elapsed().as_millis(),
                        out.slots
                    ));
                    crate::llm::diaglog::record(crate::llm::diaglog::ExtractionRecord {
                        timestamp: chrono::Utc::now().to_rfc3339(),
                        duration_ms: t0.elapsed().as_millis() as u64,
                        input_chars: structured_input.chars().count(),
                        truncated_chars,
                        output_chars: out.markdown.chars().count(),
                        outcome: "ok".into(),
                    });
                    Some(out)
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
    let final_text = if let Some(out) = &structured {
        out.markdown.clone()
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
    //     Derived from the pre-release snapshot (B2-11), not a late re-read of
    //     the shared `prev_foreground` slot (both were set to the same window at
    //     capture start, so this is equivalent but immune to a concurrent
    //     overwrite).
    let prev_hwnd = dictation_target.map(|t| t.hwnd);

    // Was the dictation aimed at one of OmniVox's own windows?  A synthetic
    // Ctrl+V doesn't reliably land in our focused WebView2 input, so for our
    // own windows we insert the text via the frontend (DOM caret insertion)
    // instead of OS paste — and we skip focus restoration since our window is
    // already foreground.
    let target_is_self = prev_hwnd.map(crate::focus::hwnd_is_own_process).unwrap_or(false);

    let focus_task = if structured.is_none() && !target_is_self {
        dictation_target.map(|t| {
            tokio::task::spawn_blocking(move || restore_foreground_window(t.hwnd, t.pid))
        })
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
        if route_to_scratchpad {
            // The scratchpad is the target (its window was foreground at record
            // start, OR it's open with capture on). Route the plain transcript
            // there regardless of which app is focused — never paste into some
            // other window the user was only reading.
            let _ = app_handle.emit(
                "dictation-insert",
                serde_json::json!({ "text": final_text, "target": "scratchpad" }),
            );
        } else if target_is_self {
            // A different OmniVox window (main/overlay) — caret-insert there. The
            // "main" label lets the main window stand its Notes-append down when
            // the dictation was actually aimed at another OmniVox window.
            let _ = app_handle.emit(
                "dictation-insert",
                serde_json::json!({ "text": final_text, "target": "main" }),
            );
        } else {
            let output_result = if let Some(ref segments) = voice_segments {
                // Inline voice commands that fire OS input (Send/Enter, mouse,
                // key combos) must land in the SAME window the dictation
                // targeted — use the identity (hwnd + pid) snapshotted before we
                // released capture (B2-11), which was itself bound at recording
                // START (B2-3).  No late pid re-read: a HWND recycled to another
                // process before output must FAIL identity, not be re-anchored to
                // whatever owns the handle now.  The router re-verifies before
                // each consequential primitive; `LaunchApp` is gated by the
                // `launch_app_voice_commands_enabled` setting.
                let target = dictation_target;
                let allow_launch = segments.iter().any(|s| {
                    matches!(
                        s,
                        crate::postprocess::voice_commands::OutputSegment::Command(
                            crate::postprocess::voice_commands::VoiceCommand::LaunchApp(_)
                        )
                    )
                }) && crate::commands::settings::launch_app_voice_command_enabled(&state.db);
                state
                    .output
                    .send_segments(segments, &output_config, target, allow_launch)
            } else {
                // Plain paste (Ctrl+V) is focus-dependent. restore_foreground_window
                // can fail silently (Windows foreground lock, HWND recycle, another
                // app grabbing focus) — its own doc says a failed restore must not be
                // treated as success (H2). Re-verify identity right at paste time so a
                // failed restore can't land the dictation in the wrong window. This is
                // the same gate the segment/command path applies. verify_foreground_target
                // returns true on non-Windows and when the target was already foreground,
                // so normal dictation is unaffected; a refusal still saves to history below.
                let target_ok = dictation_target
                    .map(|t| crate::focus::verify_foreground_target(t.hwnd, t.pid))
                    .unwrap_or(true);
                if target_ok {
                    state.output.send(&final_text, &output_config)
                } else {
                    emit_error(
                        app_handle,
                        crate::error::ErrorCode::KeystrokeError,
                        "Paste skipped — the target window lost focus. Your dictation is saved to history.",
                    );
                    Ok(())
                }
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
        && !route_to_scratchpad
        && output_config.ship_mode
        && !command_send_active
        && matches!(
            output_config.mode,
            crate::output::types::OutputMode::TypeSimulation
                | crate::output::types::OutputMode::Both
        )
    {
        let ship_target = dictation_target;
        let _ = tokio::task::spawn_blocking(move || {
            // The router's send/send_segments are synchronous and already
            // include the 250ms post-paste guard, so by this point the paste
            // keystroke has been delivered and the clipboard held stable.
            // This settle only covers the target app *processing* its Ctrl+V
            // before Enter arrives.  600ms (850ms total after paste) is sized
            // for Ship Mode's actual targets — Electron chat/agent UIs, which
            // process paste on a renderer tick — while still 900ms faster
            // than the old blind 1500ms.  Native edit controls need far less.
            std::thread::sleep(std::time::Duration::from_millis(600));
            // Re-verify foreground AFTER the settle: focus may have changed
            // during the 600ms, and Enter == "send" in Ship Mode's chat/agent
            // targets — firing it into a window we KNOW is no longer the target
            // could submit somewhere else. Skip ONLY when we have a target that
            // fails verification. When no target was captured (None), fall back
            // to the pre-change behavior and fire — there's no identity to check
            // against, and refusing would leave the message pasted-but-unsent.
            let send_ok = match ship_target {
                Some(t) => crate::focus::verify_foreground_target(t.hwnd, t.pid),
                None => true,
            };
            if send_ok {
                if let Ok(mut enigo) = enigo::Enigo::new(&enigo::Settings::default()) {
                    let _ = enigo::Keyboard::key(
                        &mut enigo,
                        enigo::Key::Return,
                        enigo::Direction::Click,
                    );
                }
            } else {
                crate::llm::diaglog::log(
                    "ship mode: auto-send Enter skipped — target window changed after paste",
                );
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
    if let Some(out) = &structured {
        let _ = app_handle.emit(
            "structured-output-ready",
            &StructuredOutputPayload {
                markdown: out.markdown.clone(),
                slots: out.slots.clone(),
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
///
/// `id` is the pending command's monotonic id: the pill echoes it back to
/// `confirm_command` / `cancel_command` so a stale pill can never consume a
/// newer command's confirm.  `editable_text` is always present (null when the
/// action isn't an editable send) to match the frontend contract.
#[derive(Clone, serde::Serialize)]
struct CommandConfirmPayload {
    id: u64,
    summary: String,
    /// When the pending action sends a typed message, the message text —
    /// the pill shows it in an editable textarea so a mishearing can be
    /// fixed before Enter fires (a verbatim send is the whole risk).  `null`
    /// otherwise.
    editable_text: Option<String>,
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

/// Snapshot the current command's [`crate::state::CommandContext`].  Falls back
/// to an empty, id-0 context if none is set (shouldn't happen — the capture
/// start always sets one — but never target a stale/absent binding).
fn current_command_context(state: &AppState) -> crate::state::CommandContext {
    state
        .command_context
        .lock()
        .ok()
        .and_then(|g| *g)
        .unwrap_or(crate::state::CommandContext {
            id: 0,
            target_hwnd: None,
            target_pid: None,
            captured_at: std::time::Instant::now(),
        })
}

/// A command is superseded (cancelled) when the monotonic cancellation floor has
/// been raised to at least its id — i.e. a "stop" for this or a later capture.
fn command_superseded(cancel_floor: u64, ctx_id: u64) -> bool {
    ctx_id != 0 && cancel_floor >= ctx_id
}

/// Whether a confirm/cancel request targets the current pending command.  A
/// `None` request id is the trusted keyboard path (it operates on the current
/// pending directly); a `Some` id (the Tauri command from the pill) must equal
/// the parked command's id.
fn confirm_id_matches(pending_id: u64, requested: Option<u64>) -> bool {
    match requested {
        Some(req) => req == pending_id,
        None => true,
    }
}

/// Emit a terminal `idle` command state, but ONLY when nothing else is going on
/// (M4 generation-gate): if a newer command is listening or already parked its
/// own confirm, a stale confirm/cancel must not clobber it back to idle.
///
/// `finalizing_id` is the command generation this idle would terminate.  A
/// newer command capture claims a higher id the instant it starts — even while
/// it is still classifying (mic released → capture Idle, no pending yet), a
/// window in which the capture-idle + no-pending checks alone would wrongly
/// fire.  So we also refuse to emit when a newer id has since been issued
/// (`command_id_gen` moved past `finalizing_id`), closing that race (B2-2).  A
/// `finalizing_id` of 0 skips the generation gate (no specific generation to
/// protect).
fn emit_command_idle_if_free(app_handle: &tauri::AppHandle, state: &AppState, finalizing_id: u64) {
    if finalizing_id != 0 {
        let latest = state
            .command_id_gen
            .load(std::sync::atomic::Ordering::Acquire);
        if latest != finalizing_id {
            return; // a newer command owns the pill — don't idle it out.
        }
    }
    let capture_idle = read_capture_mode(state) == crate::state::CaptureMode::Idle;
    let no_pending = state
        .pending_command
        .lock()
        .map(|g| g.is_none())
        .unwrap_or(true);
    if capture_idle && no_pending {
        let _ = app_handle.emit("command-state-change", "idle");
    }
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
    // Bind the target ONCE, at capture, into an immutable per-command context
    // carried through classify → pending → execute → undo.  Execution never
    // re-reads the shared `prev_foreground` slot (a concurrent dictation can
    // overwrite it — the H1 redirect).
    let ctx = crate::state::CommandContext {
        id: state.next_command_id(),
        target_hwnd: fg,
        target_pid: fg.and_then(crate::focus::pid_for_hwnd),
        captured_at: std::time::Instant::now(),
    };
    if let Ok(mut c) = state.command_context.lock() {
        *c = Some(ctx);
    }
    // Keep the legacy dictation slot in sync (harmless — nothing in the command
    // path reads it now), but the command's real target lives in `ctx`.
    if let Ok(mut prev) = state.prev_foreground.lock() {
        *prev = fg;
    }
    if let Ok(mut pending) = state.pending_command.lock() {
        *pending = None;
    }
    crate::hotkey::set_confirm_pending(None);

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

    // Snapshot the command's immutable context NOW, while this capture still
    // owns the mic — a later capture can overwrite `state.command_context`, but
    // this in-flight command carries its own bound target + id from here on.
    let ctx = current_command_context(state);

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

    // Tier-0 safety phrases — checked before everything else so no app name
    // or LLM interpretation can ever shadow them.  "stop"/"cancel" raises the
    // cancellation floor to THIS command's id, invalidating every command issued
    // so far (an in-flight chain checks it between steps; a command still
    // classifying checks it before executing).  It never gets cleared, so a stop
    // spoken during classification still prevents that command's execution while
    // later commands (higher ids) proceed.  Also clears any parked confirm.
    let norm = crate::actions::matcher::normalize(&utterance);
    // Also test the politeness-peeled form so "please stop", "can you stop",
    // "stop please", "undo that please" still hit these tier-0 phrases — Whisper
    // very commonly returns the polite wrapper, and a cancel that only matched
    // the bare word could watch a queued send fire anyway. peel_politeness only
    // strips leading/trailing politeness, so "stop music" stays "stop music"
    // (still a transport command, not a cancel).
    let peeled = crate::actions::matcher::peel_politeness(&norm);
    let is_cancel = |s: &str| {
        matches!(
            s,
            "stop" | "stop it" | "cancel" | "cancel that" | "never mind" | "nevermind" | "abort"
        )
    };
    if is_cancel(norm.as_str()) || is_cancel(peeled.as_str()) {
        // `fetch_max`, never `store` (B2-1): an older delayed stop must not
        // LOWER a floor a newer command already raised.  The floor only ever
        // rises, so a "stop" for an earlier command can't un-cancel a later one.
        state
            .command_cancel_floor
            .fetch_max(ctx.id, std::sync::atomic::Ordering::AcqRel);
        // Clear the parked confirm ONLY if it belongs to THIS stop or an earlier
        // command (id <= stop id).  A confirm parked by a NEWER command (higher
        // id) must survive this stop — clearing it unconditionally would discard
        // a command the user issued after saying "stop".
        if let Ok(mut g) = state.pending_command.lock() {
            if g.as_ref().map(|(c, _)| c.id <= ctx.id).unwrap_or(false) {
                *g = None;
                crate::hotkey::set_confirm_pending(None);
            }
        }
        emit_command_result(app_handle, "done", "Stopped");
        return;
    }

    // Tier-0 undo — before the matcher so bare "undo" (Ctrl+Z chord) never
    // shadows it.  Reverses the assistant's own last action, not the app's.
    let is_undo = |s: &str| {
        matches!(
            s,
            "undo that" | "undo it" | "undo last command" | "undo the last command"
        )
    };
    if is_undo(norm.as_str()) || is_undo(peeled.as_str()) {
        run_undo(app_handle, state, ctx).await;
        return;
    }

    // Fast path: deterministic grammar match (microseconds, no LLM).
    if let Some(intent) = crate::actions::match_command(&utterance) {
        if let crate::actions::CommandIntent::OpenApp(name) = intent {
            // Resolve ONCE here — dispatch_open_app reuses this result instead of
            // re-resolving. The open-verb matcher is greedy ("show me the desktop",
            // "run the tests", "go to youtube.com" all become OpenApp(<tail>)); when
            // the tail doesn't resolve to an installed app, defer to the LLM so it
            // can be reinterpreted (show_desktop, web_search, open_url, …) — but
            // ONLY when an LLM is actually installed. With no LLM, dispatch straight
            // to the precise "No app found" instead of a misleading "install a model".
            let lookup = name.clone();
            let resolved = tokio::task::spawn_blocking(move || {
                crate::actions::app_index::resolve(&lookup)
            })
            .await
            .ok()
            .flatten();
            if resolved.is_none() && any_llm_configured(state) {
                // fall through to classify_command_via_llm below
            } else {
                dispatch_open_app(app_handle, state, ctx, name, resolved).await;
                return;
            }
        } else {
            run_intent(app_handle, state, ctx, intent).await;
            return;
        }
    }

    // Slow path: free-form phrasing the grammar didn't catch → Qwen fallback.
    // The LLM may interpret one utterance as a multi-step chain.
    let intents = classify_command_via_llm(app_handle, state, &utterance).await;
    if !intents.is_empty() {
        // A "stop" spoken WHILE we were classifying (a fresh capture can start
        // once this one released the mic) raised the floor to at/above our id —
        // honor it before parking or executing anything.
        if command_superseded(
            state
                .command_cancel_floor
                .load(std::sync::atomic::Ordering::Acquire),
            ctx.id,
        ) {
            emit_command_result(app_handle, "done", "Stopped");
            return;
        }
        // A submitting type_text (send_message) presses Enter in another app —
        // never fire that blind. Park the WHOLE sequence and route it through
        // the same Enter/Esc confirm pill as OpenApp/CloseWindow; the chain
        // runs only after the user accepts.
        if intents
            .iter()
            .any(|i| matches!(i, crate::actions::CommandIntent::TypeText { submit: true, .. }))
        {
            let summary = confirm_chain_summary(&ctx, &intents);
            // Editable when the chain sends exactly one message — the pill
            // shows a textarea so a Whisper mishearing can be corrected
            // before the Enter fires.
            let editable_text = {
                let sends: Vec<&String> = intents
                    .iter()
                    .filter_map(|i| match i {
                        crate::actions::CommandIntent::TypeText { text, submit: true } => {
                            Some(text)
                        }
                        _ => None,
                    })
                    .collect();
                match sends.as_slice() {
                    [one] => Some((*one).clone()),
                    _ => None,
                }
            };
            if let Ok(mut pending) = state.pending_command.lock() {
                *pending = Some((ctx, crate::state::PendingCommand::Chain { intents }));
            }
            // Editable confirms deliberately do NOT arm the hook's Enter/Esc
            // path: the pill shows a focusable textarea, and a global Enter
            // swallow would eat the user's edits (and send as-heard — the
            // opposite of what a review surface is for).  The textarea's own
            // key handlers cover Ctrl+Enter / Esc instead.
            crate::hotkey::set_confirm_pending(if editable_text.is_none() {
                Some(ctx.id)
            } else {
                None
            });
            let _ = app_handle.emit(
                "command-confirm",
                &CommandConfirmPayload {
                    id: ctx.id,
                    summary,
                    editable_text,
                },
            );
            return;
        }

        // A bare LLM-produced URL the utterance never named is a fabrication
        // risk (the model invents a plausible domain) — confirm the whole
        // sequence instead of opening blind.  A grounded URL ("open github dot
        // com") stays auto.  Applies to a URL ANYWHERE in a chain, not just a
        // lone OpenUrl (a two-step "open chrome then go to <invented>" used to
        // bypass this and open unconfirmed).
        let has_ungrounded_url = intents.iter().any(|i| {
            matches!(
                i,
                crate::actions::CommandIntent::OpenUrl(url)
                    if !url_grounded_in_utterance(&utterance, url)
            )
        });
        if has_ungrounded_url {
            let summary = match intents.as_slice() {
                [crate::actions::CommandIntent::OpenUrl(url)] => format!("Open {url}?"),
                _ => confirm_chain_summary(&ctx, &intents),
            };
            if let Ok(mut pending) = state.pending_command.lock() {
                *pending = Some((ctx, crate::state::PendingCommand::Chain { intents }));
            }
            crate::hotkey::set_confirm_pending(Some(ctx.id));
            let _ = app_handle.emit(
                "command-confirm",
                &CommandConfirmPayload {
                    id: ctx.id,
                    summary,
                    editable_text: None,
                },
            );
            return;
        }
        if intents.len() == 1 {
            run_intent(app_handle, state, ctx, intents.into_iter().next().unwrap()).await;
        } else {
            run_chain(app_handle, state, ctx, intents).await;
        }
        return;
    }

    // Distinguish "not a command" from "Command Mode is on but no LLM is
    // installed to interpret free-form phrasings". Without a model, everything
    // the deterministic matcher misses (search, "tell X …", multi-step) would
    // otherwise report as a generic non-command — tell the user the real cause.
    if !utterance.is_empty() && !any_llm_configured(state) {
        emit_command_result(
            app_handle,
            "error",
            "No language model installed — add one in Models for free-form commands",
        );
        return;
    }

    let heard = if utterance.is_empty() {
        "nothing".to_string()
    } else {
        format!("\u{201c}{utterance}\u{201d}")
    };
    emit_command_result(app_handle, "error", format!("No command recognized ({heard})"));
}

/// Reverse the assistant's most recent undoable action ("undo that").
/// One-shot: the slot is consumed so a second "undo that" reports there's
/// nothing left rather than repeating the reversal.
async fn run_undo(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    ctx: crate::state::CommandContext,
) {
    use crate::state::LastAction;

    let action = state.last_action.lock().ok().and_then(|mut g| g.take());
    let Some(action) = action else {
        emit_command_result(app_handle, "error", "Nothing to undo");
        return;
    };

    // Honor a "stop"/"cancel" spoken concurrently (B2-13): a stop for this or a
    // later capture raised the cancel floor to >= our id — don't fire the
    // reversal (WM_CLOSE / restore / Ctrl+Z).  The undo slot was already taken
    // above, which is the intended one-shot behavior even when cancelled.
    if command_superseded(
        state
            .command_cancel_floor
            .load(std::sync::atomic::Ordering::Acquire),
        ctx.id,
    ) {
        emit_command_result(app_handle, "done", "Stopped");
        return;
    }

    let (summary, ok) = match action {
        LastAction::LaunchedApp { target, name } => {
            let label = format!("Closed {name}");
            match run_blocking_result(move || {
                // The launched window may have been closed / recycled since —
                // verify identity before WM_CLOSE so we never close a stranger.
                if !crate::focus::window_identity_ok(target.hwnd, target.pid) {
                    return Err(format!("{name} is no longer open"));
                }
                crate::actions::executor::run_close_window(Some(target.hwnd))
            })
            .await
            {
                Ok(()) => (label, true),
                Err(e) => (e, false),
            }
        }
        LastAction::Minimized { target } => {
            match run_blocking_result(move || {
                if !crate::focus::window_identity_ok(target.hwnd, target.pid) {
                    return Err("That window is no longer open".to_string());
                }
                crate::actions::executor::run_restore_window(target.hwnd)
            })
            .await
            {
                Ok(()) => ("Restored the window".to_string(), true),
                Err(e) => (e, false),
            }
        }
        LastAction::ShowDesktop => {
            // Win+D toggles — firing it again brings the windows back.
            match run_blocking_result(|| {
                crate::actions::executor::run_chord(crate::actions::KeyChord::ShowDesktop)
            })
            .await
            {
                Ok(()) => ("Brought your windows back".to_string(), true),
                Err(e) => (e, false),
            }
        }
        LastAction::TypedText { target } => {
            match run_blocking_result(move || {
                // Restore + verify the original window before Ctrl+Z, so undo
                // doesn't fire into whatever is focused now.
                if let Some(t) = target {
                    crate::focus::restore_foreground_window_public(t.hwnd, t.pid);
                    if !crate::focus::verify_foreground_target(t.hwnd, t.pid) {
                        return Err("The original window isn't in focus".to_string());
                    }
                }
                crate::actions::executor::run_chord(crate::actions::KeyChord::Undo)
            })
            .await
            {
                Ok(()) => ("Undid the typed text".to_string(), true),
                Err(e) => (e, false),
            }
        }
    };

    emit_command_result(app_handle, if ok { "done" } else { "error" }, summary);
}

/// `run_blocking` sibling that preserves the Result instead of flattening to
/// a summary — undo wants its own success labels.
async fn run_blocking_result(
    f: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<(), String> {
    match tokio::task::spawn_blocking(f).await {
        Ok(r) => r,
        Err(_) => Err("Command execution failed".to_string()),
    }
}

/// The registrable label of a host — the leftmost label of its eTLD+1, resolved
/// against the real (embedded, offline) Public Suffix List.  For `github.com` →
/// `github`; `github.evil.com` → `evil`; `github.evil.com.my` → `evil` (suffix
/// `com.my`). Lowercased.  Using the PSL (not a hand-maintained multi-part-TLD
/// table) means multi-level ccTLD suffixes like `com.my`/`com.au`/`co.uk` are all
/// covered, so a `<brand>.evil.<multi.tld>` spoof can never ground on `<brand>`.
fn registrable_label(host: &str) -> Option<String> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    // `domain_str` returns the registrable domain (eTLD+1), e.g. "evil.com.my".
    let registrable = psl::domain_str(&host)?;
    registrable.split('.').next().map(|s| s.to_string())
}

/// True when the utterance plausibly names the URL's registrable domain — "open
/// github dot com" grounds `github.com`; an LLM-invented domain does not.
///
/// Hardened over the Batch-1 heuristic: the URL is parsed with the `url` crate
/// (only http/https, no embedded userinfo — `github.com@evil.example` reads as
/// github but navigates to evil), grounding is on the eTLD+1 registrable label
/// (so `github.evil.com.au` is NOT grounded by "github"), and the match is on a
/// whole normalized token rather than a substring (so "app.com" is not grounded
/// by the word "application"). Labels shorter than 3 chars never ground.
fn url_grounded_in_utterance(utterance: &str, target: &str) -> bool {
    let t = target.trim();
    let normalized = if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    let parsed = match url::Url::parse(&normalized) {
        Ok(u) => u,
        Err(_) => return false,
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    // Userinfo is the spoof: the visible label isn't the navigated host.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return false;
    }
    let host = match parsed.host_str() {
        Some(h) if !h.is_empty() => h,
        _ => return false,
    };
    let label = match registrable_label(host) {
        Some(l) => l,
        None => return false,
    };
    if label.len() < 3 {
        return false;
    }
    let norm = crate::actions::matcher::normalize(utterance);
    norm.split(' ').any(|tok| tok == label)
}

#[cfg(test)]
mod url_grounding_tests {
    use super::url_grounded_in_utterance;

    #[test]
    fn spoken_domain_is_grounded() {
        assert!(url_grounded_in_utterance(
            "open github dot com",
            "https://github.com"
        ));
        assert!(url_grounded_in_utterance(
            "go to YouTube",
            "https://www.youtube.com"
        ));
    }

    #[test]
    fn invented_domain_is_not_grounded() {
        assert!(!url_grounded_in_utterance(
            "open my bank website",
            "https://chase.com"
        ));
        // Too-short labels never count as grounded.
        assert!(!url_grounded_in_utterance("open x", "https://x.com"));
    }

    #[test]
    fn subdomain_spoof_is_not_grounded() {
        // The registrable label is "evil", not the spoken "github".
        assert!(!url_grounded_in_utterance(
            "open github",
            "https://github.evil.com"
        ));
        // A genuinely grounded registrable domain still passes.
        assert!(url_grounded_in_utterance("go to github", "https://github.com"));
    }

    #[test]
    fn userinfo_spoof_is_not_grounded() {
        // `github.com@evil.example` reads as github but navigates to evil.
        assert!(!url_grounded_in_utterance(
            "open github",
            "github.com@evil.example"
        ));
        assert!(!url_grounded_in_utterance(
            "open github",
            "https://github.com@evil.example"
        ));
    }

    #[test]
    fn multi_part_tld_spoof_is_not_grounded() {
        // Registrable label is "evil" (suffix com.au), not the spoken "github".
        assert!(!url_grounded_in_utterance(
            "open github",
            "github.evil.com.au"
        ));
        // `com.my` was MISSING from the old hand-maintained table, so
        // `github.evil.com.my` used to ground on "github" (suffix read as "com").
        // With the real PSL the registrable label is "evil" — not grounded (B2-5).
        assert!(!url_grounded_in_utterance(
            "open github",
            "github.evil.com.my"
        ));
        // A real multi-part-TLD domain still grounds on its registrable label.
        assert!(url_grounded_in_utterance("go to bbc", "https://bbc.co.uk"));
        assert!(url_grounded_in_utterance(
            "go to mybank",
            "https://mybank.com.my"
        ));
    }

    #[test]
    fn substring_word_does_not_ground() {
        // "application" must NOT ground "app.com" — token match, not substring.
        assert!(!url_grounded_in_utterance(
            "open my application",
            "https://app.com"
        ));
    }
}

#[cfg(test)]
mod command_gating_tests {
    use super::{command_superseded, confirm_id_matches};

    #[test]
    fn stop_supersedes_in_flight_and_older_commands() {
        // A "stop" (its own id 5) raises the floor to 5, cancelling ids <= 5.
        assert!(command_superseded(5, 3)); // command 3 was in flight → cancelled
        assert!(command_superseded(5, 5));
        // A newer command (id 6) issued after the stop proceeds.
        assert!(!command_superseded(5, 6));
        // Fresh state: floor 0 cancels nothing.
        assert!(!command_superseded(0, 1));
        // id 0 is the "no command" sentinel and is never cancelled.
        assert!(!command_superseded(5, 0));
    }

    #[test]
    fn confirm_id_must_match_pending() {
        assert!(confirm_id_matches(7, Some(7)));
        assert!(!confirm_id_matches(7, Some(6)));
        // Trusted keyboard path (None) always targets the current pending.
        assert!(confirm_id_matches(7, None));
    }

    #[test]
    fn cancel_floor_fetch_max_never_lowers() {
        use std::sync::atomic::{AtomicU64, Ordering};
        // The real cancel floor: monotonic via `fetch_max`.
        let floor = AtomicU64::new(0);
        // A "stop" for command 3 raises the floor to 3.
        floor.fetch_max(3, Ordering::AcqRel);
        // Command 5 is issued AFTER that stop (a newer capture).  A delayed
        // "stop" for the older command 2 now lands — it must NOT lower the floor.
        floor.fetch_max(2, Ordering::AcqRel);
        let f = floor.load(Ordering::Acquire);
        assert_eq!(f, 3, "an older delayed stop must never lower the floor");
        // Command 3 stays cancelled; the newer command 5 survives the old stop.
        assert!(command_superseded(f, 3));
        assert!(!command_superseded(f, 5));
    }

    #[test]
    fn compare_and_take_rejects_replaced_slot() {
        use std::sync::Mutex;
        // Model the pending slot exactly as `confirm_pending_command` sees it:
        // a command 6 has REPLACED whatever command 5 parked.
        let slot: Mutex<Option<(u64, &str)>> = Mutex::new(Some((6, "cmd6")));

        // The single-guard compare-and-take used by confirm/cancel.
        let take_if_matches = |confirm_id: Option<u64>| -> Option<(u64, &str)> {
            let mut g = slot.lock().unwrap();
            match g.as_ref() {
                Some((id, _)) if confirm_id_matches(*id, confirm_id) => g.take(),
                _ => None,
            }
        };

        // A stale confirm echoing id 5 must NOT consume command 6's pending.
        assert!(
            take_if_matches(Some(5)).is_none(),
            "stale confirm must not consume a replaced slot"
        );
        assert!(
            slot.lock().unwrap().is_some(),
            "the newer command's pending must survive"
        );
        // The matching confirm (id 6) consumes it.
        assert_eq!(take_if_matches(Some(6)), Some((6, "cmd6")));
        assert!(slot.lock().unwrap().is_none());
    }
}

#[cfg(test)]
mod cancel_in_closure_tests {
    use super::execute_intent_now;
    use crate::actions::{CommandIntent, KeyChord};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    /// A "stop" that raised the cancel floor must abort a focus-dependent
    /// primitive from INSIDE its `spawn_blocking` closure — before it fires
    /// (B2-13).  With no target a missing in-closure gate would report "No target
    /// window…"; the gate fires FIRST and reports "stopped", proving the floor is
    /// consulted inside the closure (and no OS input is ever reached).
    #[tokio::test]
    async fn keychord_aborts_inside_closure_when_floor_raised() {
        let floor = Arc::new(AtomicU64::new(0));
        // A "stop" for command id 5 raised the floor to 5 → supersedes id 5.
        floor.store(5, Ordering::SeqCst);
        let res =
            execute_intent_now(&floor, 5, None, CommandIntent::KeyChord(KeyChord::Copy)).await;
        assert!(!res.ok);
        assert_eq!(res.summary, "stopped");
    }

    /// With the floor clear the same call passes the cancel gate and is rejected
    /// only by the absent-target guard — proving the gate is conditional, not
    /// always-on.
    #[tokio::test]
    async fn keychord_passes_cancel_gate_when_floor_clear() {
        let floor = Arc::new(AtomicU64::new(0));
        let res =
            execute_intent_now(&floor, 5, None, CommandIntent::KeyChord(KeyChord::Copy)).await;
        assert!(!res.ok);
        assert_ne!(res.summary, "stopped");
    }
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
    // Resolve the configured model id and go through the KEYED loader — no bare
    // "return whatever runner is loaded" fast path (B2-16).  `ensure_runner_loaded`
    // returns the already-loaded runner when it matches this id, else loads it,
    // so Command Mode and Structured Mode always share ONE model rather than one
    // silently getting the other's runner during a switch.
    let id = crate::storage::settings::get_settings(&state.db)
        .ok()
        .and_then(|s| s.active_llm_model_id)
        .filter(|id| !id.is_empty())
        .or_else(|| state.active_llm_model_id.lock().ok().and_then(|g| g.clone()))
        .or_else(|| crate::commands::llm::preferred_downloaded_llm_id(state))?;
    // Single-flight: shares the model load with a concurrent Structured-Mode
    // dictation instead of loading a second GGUF copy (M3).
    match crate::commands::llm::ensure_runner_loaded(&id, state, Some(app_handle)) {
        Ok(r) => Some(r),
        Err(e) => {
            crate::llm::diaglog::log(&format!("command LLM lazy-load failed: {e}"));
            None
        }
    }
}

/// True when some LLM model is configured or downloaded (does NOT load it).
/// Mirrors `ensure_llm_runner`'s id-resolution chain so Command Mode can tell
/// "not a command" apart from "no model installed to interpret free-form speech".
fn any_llm_configured(state: &AppState) -> bool {
    crate::storage::settings::get_settings(&state.db)
        .ok()
        .and_then(|s| s.active_llm_model_id)
        .filter(|id| !id.is_empty())
        .or_else(|| state.active_llm_model_id.lock().ok().and_then(|g| g.clone()))
        .or_else(|| crate::commands::llm::preferred_downloaded_llm_id(state))
        .is_some()
}

async fn classify_command_via_llm(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    utterance: &str,
) -> Vec<crate::actions::CommandIntent> {
    if utterance.is_empty() {
        return Vec::new();
    }
    // First free-form command pays a one-time model load; subsequent ones are
    // fast.  The load holds LLM_LOAD_LOCK across a blocking GGUF load + thread
    // join, so run it on the blocking pool rather than stalling this tokio
    // worker (B2-8).
    let app = app_handle.clone();
    let runner = tokio::task::spawn_blocking(move || {
        let st = app.state::<AppState>();
        ensure_llm_runner(&app, &st)
    })
    .await
    .ok()
    .flatten();
    let runner = match runner {
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

/// Dispatch a resolved (or unresolved) `OpenApp`, given an already-computed
/// resolution. Shared by `run_intent` and the fast path so the app index is
/// resolved exactly once per command (not twice).
async fn dispatch_open_app(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    ctx: crate::state::CommandContext,
    name: String,
    resolved: Option<crate::actions::app_index::ResolveResult>,
) {
    match resolved {
        None => {
            // The app may have been installed after our last enumeration — kick a
            // throttled background rescan so a retry can find it.
            tokio::task::spawn_blocking(crate::actions::app_index::refresh_if_stale);
            emit_command_result(
                app_handle,
                "error",
                format!("No app found for \u{201c}{name}\u{201d}"),
            )
        }
        Some(r) if r.score >= crate::actions::app_index::AUTO && !r.ambiguous => {
            // Re-check the cancel floor AFTER the async app-resolution step and
            // immediately before the launch (B2-1): a "stop" spoken while we
            // resolved must abort the launch.
            if command_superseded(
                state
                    .command_cancel_floor
                    .load(std::sync::atomic::Ordering::Acquire),
                ctx.id,
            ) {
                emit_command_result(app_handle, "done", "Stopped");
                return;
            }
            match crate::actions::app_index::launch(&r.app_id) {
                Ok(identity) => {
                    record_launch_for_undo(app_handle, r.name.clone(), ctx.target_hwnd, identity);
                    emit_command_result(app_handle, "done", format!("Opened {}", r.name))
                }
                Err(e) => emit_command_result(app_handle, "error", e),
            }
        }
        Some(r) => {
            // Low confidence or ambiguous (close runner-up) — ask first.
            if let Ok(mut pending) = state.pending_command.lock() {
                *pending = Some((
                    ctx,
                    crate::state::PendingCommand::OpenApp {
                        app_id: r.app_id,
                        name: r.name.clone(),
                    },
                ));
            }
            crate::hotkey::set_confirm_pending(Some(ctx.id));
            let _ = app_handle.emit(
                "command-confirm",
                &CommandConfirmPayload {
                    id: ctx.id,
                    summary: format!("Open {}?", r.name),
                    editable_text: None,
                },
            );
        }
    }
}

async fn run_intent(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    ctx: crate::state::CommandContext,
    intent: crate::actions::CommandIntent,
) {
    use crate::actions::CommandIntent;

    // A "stop" (for this or a later capture) invalidates us before we act.
    if command_superseded(
        state
            .command_cancel_floor
            .load(std::sync::atomic::Ordering::Acquire),
        ctx.id,
    ) {
        emit_command_result(app_handle, "done", "Stopped");
        return;
    }

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
            dispatch_open_app(app_handle, state, ctx, name, resolved).await;
        }
        // Consequential — never fire blind. Stash the BOUND window (captured at
        // command start, not the live foreground) + its pid and route through
        // the same Enter/Esc confirm pill that OpenApp uses.
        CommandIntent::CloseWindow => {
            match ctx.target_hwnd {
                None => emit_command_result(app_handle, "error", "No window to close"),
                Some(h) => {
                    let title = crate::actions::executor::window_title(h);
                    if let Ok(mut pending) = state.pending_command.lock() {
                        *pending = Some((
                            ctx,
                            crate::state::PendingCommand::CloseWindow {
                                hwnd: h,
                                pid: ctx.target_pid,
                                title: title.clone(),
                            },
                        ));
                    }
                    crate::hotkey::set_confirm_pending(Some(ctx.id));
                    let summary = if title.trim().is_empty() {
                        "Close this window?".to_string()
                    } else {
                        format!("Close \u{201c}{title}\u{201d}?")
                    };
                    let _ = app_handle.emit(
                        "command-confirm",
                        &CommandConfirmPayload {
                            id: ctx.id,
                            summary,
                            editable_text: None,
                        },
                    );
                }
            }
        }
        // OmniVox's own scratchpad window — dispatched here rather than in the
        // OS-only executor because it needs the Tauri AppHandle.  Open/close
        // are harmless and run immediately; clear wipes saved content, so it
        // parks behind the same Enter/Esc confirm pill as CloseWindow.
        CommandIntent::Scratchpad(action) => {
            use crate::actions::ScratchpadAction;
            match action {
                ScratchpadAction::Open => {
                    match crate::commands::scratchpad::open_scratchpad_impl(app_handle).await {
                        Ok(()) => {
                            emit_command_result(app_handle, "done", "Opened the scratchpad")
                        }
                        Err(e) => emit_command_result(app_handle, "error", e),
                    }
                }
                ScratchpadAction::Close => {
                    if crate::commands::scratchpad::close_scratchpad_impl(app_handle) {
                        emit_command_result(app_handle, "done", "Closed the scratchpad");
                    } else {
                        emit_command_result(app_handle, "error", "The scratchpad isn't open");
                    }
                }
                ScratchpadAction::Clear => {
                    if let Ok(mut pending) = state.pending_command.lock() {
                        *pending = Some((ctx, crate::state::PendingCommand::ClearScratchpad));
                    }
                    crate::hotkey::set_confirm_pending(Some(ctx.id));
                    let _ = app_handle.emit(
                        "command-confirm",
                        &CommandConfirmPayload {
                            id: ctx.id,
                            summary: "Clear everything in the scratchpad?".to_string(),
                            editable_text: None,
                        },
                    );
                }
            }
        }
        // Everything else is fire-and-report via the shared no-confirm executor,
        // bound to the command's captured target (not the live foreground).
        other => {
            let target = ctx.target();
            let undoable = undoable_from_intent(&other, target);
            let res =
                execute_intent_now(&state.command_cancel_floor, ctx.id, target, other).await;
            if res.ok {
                record_undoable(state, undoable);
            }
            emit_command_result(
                app_handle,
                if res.ok { "done" } else { "error" },
                res.summary,
            );
        }
    }
}

/// What (if anything) this intent can undo once it succeeds.  Launched apps
/// are handled separately — their undo target is the verified foreground
/// window from `settle_after_launch`, not the intent itself.
fn undoable_from_intent(
    intent: &crate::actions::CommandIntent,
    target: Option<crate::focus::WindowTarget>,
) -> Option<crate::state::LastAction> {
    use crate::actions::{CommandIntent, KeyChord, WindowAction};
    match intent {
        CommandIntent::Window(WindowAction::Minimize) => {
            target.map(|t| crate::state::LastAction::Minimized { target: t })
        }
        CommandIntent::KeyChord(KeyChord::ShowDesktop) => {
            Some(crate::state::LastAction::ShowDesktop)
        }
        CommandIntent::TypeText { submit: false, .. } => {
            Some(crate::state::LastAction::TypedText { target })
        }
        _ => None,
    }
}

/// Overwrite the single undo slot when the action is undoable.
fn record_undoable(state: &AppState, action: Option<crate::state::LastAction>) {
    if let Some(a) = action {
        if let Ok(mut g) = state.last_action.lock() {
            *g = Some(a);
        }
    }
}

/// Record a launched app's window for undo, off the result path — polls for the
/// app to take the foreground (same discipline as `settle_after_launch`) and
/// stores it ONLY when the settled window's identity is PROVEN to be the launched
/// app (B2-4).  `prev` is the command's captured target (the window the user
/// spoke from), so the settle ignores it; `identity` is the launch's expected
/// identity used to prove the settled window.
///
/// Residual limitation (B2-4): AppsFolder launches go through `explorer.exe`,
/// which never exposes the child app's pid, so `identity` is a
/// `LaunchIdentity::Package` (AUMID) that we do NOT correlate to a window.  Such
/// launches are therefore always "unproven" here and record NO undo entry —
/// preferred over a later "undo that" WM_CLOSE-ing a window that merely won
/// focus during the settle.
fn record_launch_for_undo(
    app_handle: &tauri::AppHandle,
    name: String,
    prev: Option<isize>,
    identity: crate::actions::app_index::LaunchIdentity,
) {
    let own = own_window_hwnds(app_handle);
    let h = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(settled) = settle_after_launch(prev, &own, &identity).await {
            if !settled.identity_proven {
                return;
            }
            let state = h.state::<AppState>();
            if let Ok(mut g) = state.last_action.lock() {
                *g = Some(crate::state::LastAction::LaunchedApp {
                    target: settled.target,
                    name,
                });
            };
        }
    });
}

/// Outcome of a single fire-and-report intent execution.
struct IntentResult {
    summary: String,
    ok: bool,
    /// Set only when this step LAUNCHED an app (`OpenApp`), carrying the launch's
    /// expected identity so the chain runner can prove the settled window belongs
    /// to it before retargeting focus-dependent steps at it (B2-4).
    launched: Option<crate::actions::app_index::LaunchIdentity>,
}

/// Execute a single intent against `target` — the window bound at command
/// capture (`None` = no window was captured) — and return its result instead of
/// emitting to the pill.  Focus-dependent intents (key chord / window action /
/// type) refuse to fire when the target is absent or its identity can't be
/// re-verified (extends the blind-fire refusal to H2/M8).  Unlike `run_intent`,
/// `OpenApp` never prompts here: a chain (and the single-intent delegation) must
/// not pause mid-sequence, so an app only launches on a confident, unambiguous
/// match.
///
/// `cancel_floor` + `ctx_id` are re-checked immediately before EVERY side-
/// effecting primitive — not just at the caller's pre-step gate.  Each blocking
/// closure holds a cloned `Arc` handle to the floor and re-reads it right before
/// firing (and, for `TypeText`, again between the paste and the submitting
/// Enter): a "stop" spoken while the closure restores focus or holds the
/// post-paste guard must abort before the primitive lands (B2-13).  `OpenApp`
/// additionally re-checks after its async app-resolution gap, before the launch
/// (B2-1).
async fn execute_intent_now(
    cancel_floor: &std::sync::Arc<std::sync::atomic::AtomicU64>,
    ctx_id: u64,
    target: Option<crate::focus::WindowTarget>,
    intent: crate::actions::CommandIntent,
) -> IntentResult {
    use crate::actions::CommandIntent;
    use std::sync::atomic::Ordering;

    // OpenApp is the only step that both awaits (app resolution) before its sink
    // AND carries a launch identity, so it is handled on its own.
    if let CommandIntent::OpenApp(name) = intent {
        let lookup = name.clone();
        let resolved =
            tokio::task::spawn_blocking(move || crate::actions::app_index::resolve(&lookup))
                .await
                .ok()
                .flatten();
        return match resolved {
            Some(r) if r.score >= crate::actions::app_index::AUTO && !r.ambiguous => {
                // Re-check the cancel floor AFTER resolution, immediately before
                // the launch (B2-1) — a "stop" spoken while resolving aborts it.
                if command_superseded(
                    cancel_floor.load(std::sync::atomic::Ordering::Acquire),
                    ctx_id,
                ) {
                    return IntentResult {
                        summary: "stopped".to_string(),
                        ok: false,
                        launched: None,
                    };
                }
                match crate::actions::app_index::launch(&r.app_id) {
                    Ok(identity) => IntentResult {
                        summary: format!("Opened {}", r.name),
                        ok: true,
                        launched: Some(identity),
                    },
                    Err(e) => IntentResult {
                        summary: e,
                        ok: false,
                        launched: None,
                    },
                }
            }
            // An app resolved but wasn't confident/unambiguous enough to
            // auto-launch.  In a chain there's no confirm prompt, so report it
            // as a skip rather than the misleading "no app found".
            Some(_) => IntentResult {
                summary: format!("Not sure which app you meant by \u{201c}{name}\u{201d}"),
                ok: false,
                launched: None,
            },
            None => {
                // Installed-after-enumeration self-heal (see run_intent).
                tokio::task::spawn_blocking(crate::actions::app_index::refresh_if_stale);
                IntentResult {
                    summary: format!("No app found for \u{201c}{name}\u{201d}"),
                    ok: false,
                    launched: None,
                }
            }
        };
    }

    let (summary, ok) = match intent {
        // Handled above.
        CommandIntent::OpenApp(_) => unreachable!("OpenApp handled before this match"),
        // Foreground keystroke action: restore the bound target, VERIFY it took
        // focus (identity + pid), then fire the chord — never into whatever
        // happened to be focused.  Refuse when there's no bound target.
        CommandIntent::KeyChord(chord) => {
            let floor = std::sync::Arc::clone(cancel_floor);
            run_blocking(chord.past_tense(), move || {
                if command_superseded(floor.load(Ordering::Acquire), ctx_id) {
                    return Err("stopped".to_string());
                }
                let t = target.ok_or_else(|| {
                    "No target window for this command".to_string()
                })?;
                if !crate::focus::restore_foreground_window(t.hwnd, t.pid)
                    || !crate::focus::verify_foreground_target(t.hwnd, t.pid)
                {
                    return Err("Target window is not in focus".to_string());
                }
                // Re-check immediately before the chord — the focus restore slept.
                if command_superseded(floor.load(Ordering::Acquire), ctx_id) {
                    return Err("stopped".to_string());
                }
                crate::actions::executor::run_chord(chord)
            })
            .await
        }
        // Media/volume keys are global (system-wide) — they ignore focus, so we
        // do NOT restore the target window (avoids yanking focus around for a
        // key that doesn't need it).
        CommandIntent::Media(action) => {
            let floor = std::sync::Arc::clone(cancel_floor);
            run_blocking(action.label(), move || {
                if command_superseded(floor.load(Ordering::Acquire), ctx_id) {
                    return Err("stopped".to_string());
                }
                crate::actions::executor::run_media(action)
            })
            .await
        }
        // Window action targets a specific window (need not be foreground) —
        // verify it still exists and belongs to the same process before acting.
        CommandIntent::Window(action) => {
            let floor = std::sync::Arc::clone(cancel_floor);
            run_blocking(action.label(), move || {
                if command_superseded(floor.load(Ordering::Acquire), ctx_id) {
                    return Err("stopped".to_string());
                }
                let t = target.ok_or_else(|| "No target window".to_string())?;
                if !crate::focus::window_identity_ok(t.hwnd, t.pid) {
                    return Err("Target window no longer exists".to_string());
                }
                crate::actions::executor::run_window(action, Some(t.hwnd))
            })
            .await
        }
        // Browser actions open in the default browser — no focus restore needed.
        CommandIntent::WebSearch(query) => {
            let floor = std::sync::Arc::clone(cancel_floor);
            run_blocking("Web search", move || {
                if command_superseded(floor.load(Ordering::Acquire), ctx_id) {
                    return Err("stopped".to_string());
                }
                crate::actions::executor::run_web_search(&query)
            })
            .await
        }
        CommandIntent::OpenUrl(url) => {
            let floor = std::sync::Arc::clone(cancel_floor);
            run_blocking("Opened link", move || {
                if command_superseded(floor.load(Ordering::Acquire), ctx_id) {
                    return Err("stopped".to_string());
                }
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
        // Scratchpad control needs the Tauri AppHandle (pipeline-level, like
        // CloseWindow's confirm) — and clear must never run un-confirmed — so
        // scratchpad steps aren't supported inside multi-step chains yet.
        CommandIntent::Scratchpad(_) => (
            "The scratchpad isn't supported inside a multi-step command".to_string(),
            false,
        ),
        // Focus-dependent like a key chord: `run_type_text` restores + verifies
        // the bound target, pastes, RE-verifies, then (for a confirmed send)
        // presses Enter.  A submitting TypeText only ever reaches here after the
        // user accepted the confirm pill — the classify path parks it first.
        CommandIntent::TypeText { text, submit } => {
            let label = if submit { "Sent message" } else { "Typed text" };
            let floor = std::sync::Arc::clone(cancel_floor);
            run_blocking(label, move || {
                let t = target.ok_or_else(|| {
                    "No target window for this message".to_string()
                })?;
                // `run_type_text` polls this before the paste and between the
                // paste and the submitting Enter — a "stop" spoken during the
                // focus restore / post-paste guard aborts before Enter (B2-13).
                let should_cancel =
                    || command_superseded(floor.load(Ordering::Acquire), ctx_id);
                crate::actions::executor::run_type_text(&text, submit, t, should_cancel)
            })
            .await
        }
    };
    IntentResult {
        summary,
        ok,
        launched: None,
    }
}

/// Human summary for the confirm pill when a classified sequence contains a
/// submitting type_text.  A lone send names the window it will land in
/// ("Send “fix the bug” to “Claude”?"); a chain narrates its steps
/// ("Open Claude, then send “fix the bug”?").
fn confirm_chain_summary(
    ctx: &crate::state::CommandContext,
    intents: &[crate::actions::CommandIntent],
) -> String {
    use crate::actions::CommandIntent::*;

    if let [TypeText { text, submit: true }] = intents {
        let title = ctx
            .target_hwnd
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
            Scratchpad(a) => match a {
                crate::actions::ScratchpadAction::Open => "open the scratchpad".to_string(),
                crate::actions::ScratchpadAction::Close => "close the scratchpad".to_string(),
                crate::actions::ScratchpadAction::Clear => "clear the scratchpad".to_string(),
            },
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
    ctx: crate::state::CommandContext,
    intents: Vec<crate::actions::CommandIntent>,
) {
    use crate::actions::CommandIntent;

    let total = intents.len();
    let mut summaries = Vec::with_capacity(total);
    let mut all_ok = true;

    // NOTE (H6): the cancellation floor is NEVER cleared here — it's monotonic.
    // A "stop" spoken during classification (or between steps) that raised the
    // floor to >= our id must keep this chain from running.  Clearing it on
    // entry (the old `command_abort = false`) would have discarded exactly that
    // stop.  Later commands get higher ids, so nothing wedges.

    // Focus target for keystroke/window steps.  Starts as the window BOUND at
    // command capture; once a step launches an app we re-aim at that verified
    // app so "open notepad and paste" targets notepad, not the window the user
    // spoke from.
    let mut target = ctx.target();

    // OmniVox's own windows — never re-aim a chain's keystrokes at our own UI if
    // the overlay/main happens to be foreground during the launch settle.
    let own = own_window_hwnds(app_handle);

    // Set when a launch did NOT confirm the new app took the foreground, so
    // `target` can't be trusted for a focus-dependent step.
    let mut target_unverified = false;

    for (i, intent) in intents.into_iter().enumerate() {
        // Cancellation (H6): a spoken "stop"/"cancel" raised the floor to >= our
        // id — honor it before firing anything further.  Checked per step so a
        // 5-step chain can be halted between any two actions.
        if command_superseded(
            state
                .command_cancel_floor
                .load(std::sync::atomic::Ordering::Acquire),
            ctx.id,
        ) {
            summaries.push("stopped".to_string());
            all_ok = false;
            break;
        }

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
        let undoable = undoable_from_intent(&intent, target);
        let IntentResult {
            summary,
            ok,
            launched,
        } = execute_intent_now(&state.command_cancel_floor, ctx.id, target, intent).await;
        if ok {
            record_undoable(state, undoable);
        }
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
            // Confirm the launched app took the foreground AND that the settled
            // window's identity is PROVEN to be it before aiming later steps at
            // it (B2-4).  Proven → retarget; unproven (AppsFolder launch exposes
            // no child pid, app already foreground, or too slow) → keep the old
            // target but flag it so a following focus-dependent step won't fire
            // blind.
            let settled = match &launched {
                Some(identity) => {
                    settle_after_launch(target.map(|t| t.hwnd), &own, identity).await
                }
                None => None,
            };
            match settled {
                // A window stabilized as the launched app's foreground — aim the
                // chain's later focus-dependent steps at it.  For AppsFolder /
                // Store launches identity is "unproven" (LaunchIdentity::Package
                // is an AUMID we don't PID-correlate), but we still retarget: the
                // per-primitive execution-time check in `execute_intent_now`
                // (foreground + pid must match `target` right before firing) is
                // the real gate, and a hard refusal here broke the flagship
                // "open <app> and send/type …" flow for every Store app (e.g.
                // "tell Claude to …").  `identity_proven` still gates UNDO
                // recording, so "undo that" never WM_CLOSEs an unproven window.
                Some(s) => {
                    target = Some(s.target);
                    target_unverified = false;
                }
                // Nothing stabilized (launch failed / no new window) — refuse
                // focus-dependent steps rather than fire into the wrong place.
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

/// A window that stabilized as the foreground after a launch, plus whether its
/// identity was PROVEN to be the launched app.
struct SettledWindow {
    target: crate::focus::WindowTarget,
    /// True only when the settled window's owning pid matched the launch's
    /// expected pid.  For AppsFolder launches (`LaunchIdentity::Package`, an
    /// AUMID we cannot cheaply correlate to a window) this is ALWAYS false — the
    /// caller must then NOT retarget focus-dependent steps at it or record undo
    /// (B2-4 residual limitation).
    identity_proven: bool,
}

/// Poll (up to ~4.2s) for a freshly launched app to take the foreground.
///
/// Accepts a candidate only when it is NOT `prev`, NOT one of OmniVox's own
/// windows, is a real visible titled app window, AND stays foreground across two
/// consecutive polls (H3: a transient notification / Alt-Tab flicker that wins a
/// single poll no longer hijacks the chain or poisons undo).  The settled
/// window's identity is `proven` only when its owning pid matches `expected`
/// (`LaunchIdentity::Pid`); an AUMID (`Package`) can't be correlated to a window,
/// so it is never proven (B2-4).  Returns `None` if no window stabilizes.
async fn settle_after_launch(
    prev: Option<isize>,
    own: &[isize],
    expected: &crate::actions::app_index::LaunchIdentity,
) -> Option<SettledWindow> {
    use crate::actions::app_index::LaunchIdentity;
    let mut candidate: Option<isize> = None;
    // 28 × 150ms ≈ 4.2s. Cold Store/Electron apps (the "tell claude to …" /
    // "open slack then …" chains) can take longer than the old ~2.4s ceiling to
    // present a stable window; without the headroom the next focus-dependent
    // chain step was refused with "couldn't confirm the launched app's window".
    // Warm apps still return on their first stable pair, so this costs nothing
    // in the common case.
    for _ in 0..28 {
        tokio::time::sleep(Duration::from_millis(150)).await;
        match capture_foreground_window() {
            Some(h)
                if Some(h) != prev && !own.contains(&h) && crate::focus::is_real_app_window(h) =>
            {
                if candidate == Some(h) {
                    // Stable across two polls → correlate its owning pid and bind.
                    let pid = crate::focus::pid_for_hwnd(h);
                    let identity_proven = match expected {
                        LaunchIdentity::Pid(p) => pid == Some(*p),
                        // AUMID can't be cheaply mapped to a window — unproven.
                        LaunchIdentity::Package(_) => false,
                    };
                    return Some(SettledWindow {
                        target: crate::focus::WindowTarget { hwnd: h, pid },
                        identity_proven,
                    });
                }
                candidate = Some(h);
            }
            _ => candidate = None,
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

/// How long a parked confirm stays executable, measured from when its command
/// was captured (`CommandContext::captured_at`).  Generous — the user may pause
/// to read the pill or edit a send — but bounded so a confirm accepted minutes
/// later (its target window long gone) is rejected instead of firing blind.
const CONFIRM_MAX_AGE: Duration = Duration::from_secs(120);

/// Execute the pending (confirmed) command — called by the `confirm_command`
/// Tauri command (with the pill's echoed `confirm_id`) or the trusted keyboard
/// path (`confirm_id = None`) when the user accepts a low-confidence app match,
/// a window close, or a chain containing a message send.
///
/// Executes ONLY when `confirm_id` matches the parked command's id (H4/M4): a
/// stale pill can't consume a newer command's confirm.  On a mismatch (or
/// nothing parked) it no-ops, emitting a terminal `idle` only when nothing newer
/// is active (generation-gated).
///
/// The compare-and-take is done under ONE mutex guard (B2-2): peeking the id
/// under one lock and then re-locking to `take()` whatever is present would let
/// a newer command's pending, parked in the gap, be consumed by this stale
/// confirm.  A `captured_at` freshness deadline also rejects a confirm arriving
/// long after the command was captured (its bound target is likely gone).
///
/// `edited_text`, when present, replaces the message of the chain's single
/// submitting TypeText — the pill's editable confirm lets the user fix a
/// mishearing before it sends.  Ignored for non-chain pendings.
pub async fn confirm_pending_command(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    confirm_id: Option<u64>,
    edited_text: Option<String>,
) {
    // Atomic compare-and-take: under a SINGLE guard, take the pending ONLY if
    // its id matches `confirm_id` (or the trusted keyboard `None`).  A newer
    // command that replaced the slot has a different id and is left untouched.
    let taken = match state.pending_command.lock() {
        Ok(mut g) => match g.as_ref() {
            Some((c, _)) if confirm_id_matches(c.id, confirm_id) => g.take(),
            _ => None,
        },
        Err(_) => None,
    };
    let Some((ctx, p)) = taken else {
        // Nothing matched — a stale confirm or an already-replaced slot.  Emit
        // idle only if this generation is still the latest (won't clobber a
        // newer command's pill).
        emit_command_idle_if_free(app_handle, state, confirm_id.unwrap_or(0));
        return;
    };
    crate::hotkey::set_confirm_pending(None);
    crate::llm::diaglog::log(&format!(
        "pipeline: confirm command id={} age={}ms",
        ctx.id,
        ctx.captured_at.elapsed().as_millis()
    ));

    // Freshness deadline: a confirm parked far in the past is acting on a stale
    // world (the bound target window is very likely gone).  Reject rather than
    // fire a consequential action against whatever exists now (B2-2).  The
    // per-primitive identity re-verify below is the second line of defense.
    if ctx.captured_at.elapsed() > CONFIRM_MAX_AGE {
        crate::llm::diaglog::log("pipeline: confirm rejected — command context too old");
        emit_command_result(app_handle, "error", "That command expired — say it again");
        return;
    }

    // A "stop"/"cancel" spoken between the pill showing and this confirm raised
    // the cancel floor to >= our id — honor it before any side effect (B2-1).
    if command_superseded(
        state
            .command_cancel_floor
            .load(std::sync::atomic::Ordering::Acquire),
        ctx.id,
    ) {
        emit_command_result(app_handle, "done", "Stopped");
        return;
    }

    match p {
        crate::state::PendingCommand::OpenApp { app_id, name } => {
            match crate::actions::app_index::launch(&app_id) {
                Ok(identity) => {
                    record_launch_for_undo(app_handle, name.clone(), ctx.target_hwnd, identity);
                    emit_command_result(app_handle, "done", format!("Opened {name}"))
                }
                Err(e) => emit_command_result(app_handle, "error", e),
            }
        }
        crate::state::PendingCommand::CloseWindow { hwnd, pid, title } => {
            let label = if title.trim().is_empty() {
                "Closed window".to_string()
            } else {
                format!("Closed {title}")
            };
            // Re-verify the window still exists and is the same process before
            // WM_CLOSE (M8: the handle may have been recycled since the confirm
            // was shown — the mouse path is unbounded in time).
            if !crate::focus::window_identity_ok(hwnd, pid) {
                emit_command_result(app_handle, "error", "That window is no longer open");
                return;
            }
            match crate::actions::executor::run_close_window(Some(hwnd)) {
                Ok(()) => emit_command_result(app_handle, "done", label),
                Err(e) => emit_command_result(app_handle, "error", e),
            }
        }
        crate::state::PendingCommand::ClearScratchpad => {
            match crate::commands::scratchpad::clear_scratchpad_impl(app_handle) {
                Ok(()) => emit_command_result(app_handle, "done", "Cleared the scratchpad"),
                Err(e) => emit_command_result(app_handle, "error", e),
            }
        }
        crate::state::PendingCommand::Chain { mut intents } => {
            // Apply an edit from the pill's textarea to the single submitting
            // TypeText (the only editable pending shape the pill offers).
            if let Some(new_text) = edited_text {
                let trimmed = new_text.trim();
                if !trimmed.is_empty() {
                    for intent in intents.iter_mut() {
                        if let crate::actions::CommandIntent::TypeText { text, submit: true } =
                            intent
                        {
                            *text = trimmed.to_string();
                        }
                    }
                }
            }
            // The user accepted the whole sequence up front, so the chain
            // runner may now execute its submitting type_text steps against the
            // originally bound target.
            run_chain(app_handle, state, ctx, intents).await;
        }
    }
}

/// Clear a pending command (user cancelled the confirm).  Like the confirm path,
/// only the matching `confirm_id` (or the trusted keyboard `None`) consumes it —
/// via the same single-guard compare-and-take so a newer command's pending
/// parked in the gap is never cleared by a stale cancel (B2-2).
pub fn cancel_pending_command(
    app_handle: &tauri::AppHandle,
    state: &AppState,
    confirm_id: Option<u64>,
) {
    let cleared = match state.pending_command.lock() {
        Ok(mut g) => match g.as_ref() {
            Some((c, _)) if confirm_id_matches(c.id, confirm_id) => {
                *g = None;
                true
            }
            _ => false,
        },
        Err(_) => false,
    };
    if cleared {
        crate::hotkey::set_confirm_pending(None);
        let _ = app_handle.emit("command-state-change", "idle");
    } else {
        // Nothing parked, or a stale id — no-op, gated idle so we don't clobber a
        // newer command.
        emit_command_idle_if_free(app_handle, state, confirm_id.unwrap_or(0));
    }
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
        Scratchpad(a) => match a {
            crate::actions::ScratchpadAction::Open => "Open the scratchpad".to_string(),
            crate::actions::ScratchpadAction::Close => "Close the scratchpad".to_string(),
            crate::actions::ScratchpadAction::Clear => {
                "Clear the scratchpad (will ask to confirm)".to_string()
            }
        },
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
