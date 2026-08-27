pub mod credentials;
pub mod provider;

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{Emitter, Manager};
#[cfg(windows)]
use windows_sys::Win32::System::Registry::HKEY;

use crate::asr::types::TranscriptionOptions;
use crate::audio::capture::AudioCapture;
use crate::state::AppState;
use crate::storage::meetings;

const CHUNK_SECONDS: u64 = 20;
const SAMPLE_RATE: u64 = 16_000;

pub struct MeetingManager {
    runtime: Mutex<Option<MeetingRuntime>>,
}

struct MeetingRuntime {
    meeting_id: String,
    started: Instant,
    paused: bool,
    mic: AudioCapture,
    system: AudioCapture,
    stop_requested: Arc<AtomicBool>,
    capture_finished: Arc<AtomicBool>,
    mic_audio_seen: Arc<AtomicBool>,
    system_audio_seen: Arc<AtomicBool>,
    last_audio_activity_ms: Arc<AtomicU64>,
}

unsafe impl Send for MeetingRuntime {}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingStatePayload {
    pub meeting_id: Option<String>,
    pub status: String,
    pub elapsed_ms: u64,
    pub mic_level: f32,
    pub system_level: f32,
    pub mic_signal_detected: bool,
    pub system_signal_detected: bool,
    pub capture_warning: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingAudioSourceReadiness {
    pub available: bool,
    pub signal_detected: bool,
    pub peak_level: f32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MeetingAudioReadiness {
    pub microphone: MeetingAudioSourceReadiness,
    pub system_audio: MeetingAudioSourceReadiness,
}

impl Default for MeetingManager {
    fn default() -> Self {
        Self::new()
    }
}

impl MeetingManager {
    pub fn new() -> Self {
        Self {
            runtime: Mutex::new(None),
        }
    }

    pub fn state(&self) -> MeetingStatePayload {
        let guard = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        match guard.as_ref() {
            Some(runtime) => {
                let elapsed_ms = runtime.started.elapsed().as_millis().min(u64::MAX as u128) as u64;
                let mic_level = runtime.mic.current_level();
                let system_level = runtime.system.current_level();
                if mic_level >= 0.035 {
                    runtime.mic_audio_seen.store(true, Ordering::Release);
                }
                if system_level >= 0.035 {
                    runtime.system_audio_seen.store(true, Ordering::Release);
                }
                if mic_level >= 0.035 || system_level >= 0.035 {
                    runtime
                        .last_audio_activity_ms
                        .store(elapsed_ms, Ordering::Release);
                }
                let stopped = runtime.stop_requested.load(Ordering::Acquire);
                let mic_signal_detected = runtime.mic_audio_seen.load(Ordering::Acquire);
                let system_signal_detected = runtime.system_audio_seen.load(Ordering::Acquire);
                let capture_warning = capture_health_warning(
                    elapsed_ms,
                    runtime.paused,
                    stopped,
                    mic_signal_detected,
                    system_signal_detected,
                    runtime.last_audio_activity_ms.load(Ordering::Acquire),
                    runtime.mic.stream_error().as_deref(),
                    runtime.system.stream_error().as_deref(),
                );
                MeetingStatePayload {
                    meeting_id: Some(runtime.meeting_id.clone()),
                    status: if stopped {
                        "transcribing".into()
                    } else if runtime.paused {
                        "paused".into()
                    } else {
                        "recording".into()
                    },
                    elapsed_ms,
                    mic_level,
                    system_level,
                    mic_signal_detected,
                    system_signal_detected,
                    capture_warning,
                }
            }
            None => MeetingStatePayload {
                meeting_id: None,
                status: "idle".into(),
                elapsed_ms: 0,
                mic_level: 0.0,
                system_level: 0.0,
                mic_signal_detected: false,
                system_signal_detected: false,
                capture_warning: None,
            },
        }
    }

    fn clear_if(&self, meeting_id: &str) {
        let mut guard = self.runtime.lock().unwrap_or_else(|p| p.into_inner());
        if guard.as_ref().is_some_and(|r| r.meeting_id == meeting_id) {
            guard.take();
        }
    }
}

pub fn start(
    app: &tauri::AppHandle,
    title: &str,
    source_app: Option<&str>,
    previous_meeting_id: Option<&str>,
) -> Result<meetings::Meeting, String> {
    let state = app.state::<AppState>();
    // Dictation ownership is deliberately not consulted: a meeting records
    // through its own capture streams, so it can start (and run) alongside an
    // in-flight dictation or Command Mode utterance.
    let mut meeting_guard = state
        .meeting
        .runtime
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if meeting_guard.is_some() {
        return Err("A meeting is already active".into());
    }
    let mic_config = state
        .audio
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .config()
        .clone();
    let transcribe_while_recording = meetings::get_provider_settings(&state.db)
        .map(|settings| settings.transcription_mode == "live")
        .unwrap_or(false);
    let meeting = meetings::create_meeting_with_previous(
        &state.db,
        title.trim().if_empty("Untitled meeting"),
        source_app,
        previous_meeting_id,
    )
    .map_err(|e| e.to_string())?;
    let mut mic = AudioCapture::new(mic_config);
    let mut system = AudioCapture::new_system_loopback();
    if let Err(error) = mic.start() {
        meetings::set_status(
            &state.db,
            &meeting.id,
            "failed",
            Some(&error.to_string()),
            true,
        )
        .map_err(|e| e.to_string())?;
        return Err(format!("Could not capture microphone audio: {error}"));
    }
    if let Err(error) = system.start() {
        mic.cancel();
        meetings::set_status(
            &state.db,
            &meeting.id,
            "failed",
            Some(&error.to_string()),
            true,
        )
        .map_err(|e| e.to_string())?;
        return Err(format!("Could not capture meeting audio: {error}"));
    }
    let stop_requested = Arc::new(AtomicBool::new(false));
    let capture_finished = Arc::new(AtomicBool::new(false));
    let mic_audio_seen = Arc::new(AtomicBool::new(false));
    let system_audio_seen = Arc::new(AtomicBool::new(false));
    let last_audio_activity_ms = Arc::new(AtomicU64::new(0));
    *meeting_guard = Some(MeetingRuntime {
        meeting_id: meeting.id.clone(),
        started: Instant::now(),
        paused: false,
        mic,
        system,
        stop_requested: stop_requested.clone(),
        capture_finished: capture_finished.clone(),
        mic_audio_seen: mic_audio_seen.clone(),
        system_audio_seen: system_audio_seen.clone(),
        last_audio_activity_ms: last_audio_activity_ms.clone(),
    });
    drop(meeting_guard);
    spawn_spooler(
        app.clone(),
        meeting.id.clone(),
        stop_requested.clone(),
        capture_finished.clone(),
        mic_audio_seen,
        system_audio_seen,
        last_audio_activity_ms,
    );
    spawn_transcriber(
        app.clone(),
        meeting.id.clone(),
        stop_requested,
        capture_finished,
        transcribe_while_recording,
    );
    emit_state(app, &state.meeting.state());
    Ok(meeting)
}

pub fn test_audio_readiness(app: &tauri::AppHandle) -> Result<MeetingAudioReadiness, String> {
    let state = app.state::<AppState>();
    // Match Meeting Mode's ownership order so a readiness probe cannot race a
    // dictation or meeting start and temporarily steal either WASAPI endpoint.
    let _capture_guard = state.capture.lock().unwrap_or_else(|p| p.into_inner());
    if _capture_guard.is_some() {
        return Err("Finish the current dictation before testing meeting audio".into());
    }
    let _meeting_guard = state
        .meeting
        .runtime
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    if _meeting_guard.is_some() {
        return Err("Meeting audio is already in use".into());
    }
    let mic_config = state
        .audio
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .config()
        .clone();
    let mut microphone = AudioCapture::new(mic_config);
    let mut system_audio = AudioCapture::new_system_loopback();
    let mic_start = microphone.start().map_err(|error| error.to_string());
    let system_start = system_audio.start().map_err(|error| error.to_string());
    let mut mic_peak = 0.0_f32;
    let mut system_peak = 0.0_f32;
    if mic_start.is_ok() || system_start.is_ok() {
        for _ in 0..15 {
            std::thread::sleep(Duration::from_millis(100));
            if mic_start.is_ok() {
                mic_peak = mic_peak.max(microphone.current_level());
            }
            if system_start.is_ok() {
                system_peak = system_peak.max(system_audio.current_level());
            }
        }
    }
    let microphone = finish_readiness_source(microphone, mic_start, mic_peak);
    let system_audio = finish_readiness_source(system_audio, system_start, system_peak);
    Ok(MeetingAudioReadiness {
        microphone,
        system_audio,
    })
}

fn finish_readiness_source(
    mut capture: AudioCapture,
    start: Result<(), String>,
    peak_level: f32,
) -> MeetingAudioSourceReadiness {
    let error = match start {
        Err(error) => Some(error),
        Ok(()) => capture.stop().err().map(|error| error.to_string()),
    };
    MeetingAudioSourceReadiness {
        available: error.is_none(),
        signal_detected: error.is_none() && peak_level >= 0.035,
        peak_level,
        error,
    }
}

pub fn pause(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (meeting_id, elapsed, mic, system, mic_seen, system_seen, last_activity, capture_error) = {
        let mut guard = state
            .meeting
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let runtime = guard.as_mut().ok_or("No meeting is active")?;
        if runtime.paused {
            return Ok(());
        }
        let (mic, mic_error) = stop_capture_preserving_audio(&mut runtime.mic);
        let (system, system_error) = stop_capture_preserving_audio(&mut runtime.system);
        runtime.paused = true;
        (
            runtime.meeting_id.clone(),
            runtime.started.elapsed().as_millis() as u64,
            mic,
            system,
            runtime.mic_audio_seen.clone(),
            runtime.system_audio_seen.clone(),
            runtime.last_audio_activity_ms.clone(),
            join_errors(mic_error, system_error),
        )
    };
    record_capture_activity(
        &mic_seen,
        &system_seen,
        &last_activity,
        elapsed,
        &mic,
        &system,
    );
    let spool_error = spool_pair(&state, &meeting_id, elapsed, mic, system).err();
    let error = join_errors(capture_error, spool_error);
    meetings::set_status(&state.db, &meeting_id, "paused", error.as_deref(), false)
        .map_err(|e| e.to_string())?;
    emit_state(app, &state.meeting.state());
    let _ = app.emit("meeting-updated", &meeting_id);
    error.map_or(Ok(()), Err)
}

pub fn resume(app: &tauri::AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let id = {
        let mut guard = state
            .meeting
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let runtime = guard.as_mut().ok_or("No meeting is active")?;
        if !runtime.paused {
            return Ok(());
        }
        runtime.mic.start().map_err(|e| e.to_string())?;
        if let Err(error) = runtime.system.start() {
            runtime.mic.cancel();
            return Err(error.to_string());
        }
        runtime.last_audio_activity_ms.store(
            runtime.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            Ordering::Release,
        );
        runtime.paused = false;
        runtime.meeting_id.clone()
    };
    meetings::set_status(&state.db, &id, "recording", None, false).map_err(|e| e.to_string())?;
    emit_state(app, &state.meeting.state());
    Ok(())
}

pub fn stop(app: &tauri::AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let (
        meeting_id,
        elapsed,
        mic,
        system,
        stop_requested,
        capture_finished,
        mic_seen,
        system_seen,
        last_activity,
        capture_error,
    ) = {
        let mut guard = state
            .meeting
            .runtime
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        let runtime = guard.as_mut().ok_or("No meeting is active")?;
        if runtime.stop_requested.swap(true, Ordering::AcqRel) {
            return Ok(runtime.meeting_id.clone());
        }
        let (mic, system, capture_error) = if runtime.paused {
            (Vec::new(), Vec::new(), None)
        } else {
            let (mic, mic_error) = stop_capture_preserving_audio(&mut runtime.mic);
            let (system, system_error) = stop_capture_preserving_audio(&mut runtime.system);
            (mic, system, join_errors(mic_error, system_error))
        };
        (
            runtime.meeting_id.clone(),
            runtime.started.elapsed().as_millis() as u64,
            mic,
            system,
            runtime.stop_requested.clone(),
            runtime.capture_finished.clone(),
            runtime.mic_audio_seen.clone(),
            runtime.system_audio_seen.clone(),
            runtime.last_audio_activity_ms.clone(),
            capture_error,
        )
    };
    record_capture_activity(
        &mic_seen,
        &system_seen,
        &last_activity,
        elapsed,
        &mic,
        &system,
    );
    let spool_error = spool_pair(&state, &meeting_id, elapsed, mic, system).err();
    let error = join_errors(capture_error, spool_error);
    capture_finished.store(true, Ordering::Release);
    stop_requested.store(true, Ordering::Release);
    meetings::set_status(
        &state.db,
        &meeting_id,
        "transcribing",
        error.as_deref(),
        true,
    )
    .map_err(|e| e.to_string())?;
    emit_state(app, &state.meeting.state());
    let _ = app.emit("meeting-updated", &meeting_id);
    if let Some(error) = error {
        let _ = app.emit("meeting-error", &error);
    }
    Ok(meeting_id)
}

fn spawn_spooler(
    app: tauri::AppHandle,
    meeting_id: String,
    stop: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    mic_audio_seen: Arc<AtomicBool>,
    system_audio_seen: Arc<AtomicBool>,
    last_audio_activity_ms: Arc<AtomicU64>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(CHUNK_SECONDS));
        interval.tick().await;
        while !stop.load(Ordering::Acquire) {
            interval.tick().await;
            if stop.load(Ordering::Acquire) {
                break;
            }
            let state = app.state::<AppState>();
            let drained = {
                let guard = state
                    .meeting
                    .runtime
                    .lock()
                    .unwrap_or_else(|p| p.into_inner());
                guard
                    .as_ref()
                    .filter(|r| r.meeting_id == meeting_id && !r.paused)
                    .map(|r| {
                        (
                            r.started.elapsed().as_millis() as u64,
                            r.mic.drain_samples(),
                            r.system.drain_samples(),
                        )
                    })
            };
            if let Some((elapsed, mic, system)) = drained {
                record_capture_activity(
                    &mic_audio_seen,
                    &system_audio_seen,
                    &last_audio_activity_ms,
                    elapsed,
                    &mic,
                    &system,
                );
                if let Err(error) = spool_pair(&state, &meeting_id, elapsed, mic, system) {
                    let _ =
                        meetings::set_status(&state.db, &meeting_id, "error", Some(&error), false);
                    let _ = app.emit("meeting-error", &error);
                    let _ = app.emit("meeting-updated", &meeting_id);
                } else {
                    let _ = app.emit("meeting-updated", &meeting_id);
                }
            }
        }
        finished.store(true, Ordering::Release);
    });
}

fn spawn_transcriber(
    app: tauri::AppHandle,
    meeting_id: String,
    stop: Arc<AtomicBool>,
    capture_finished: Arc<AtomicBool>,
    transcribe_while_recording: bool,
) {
    tauri::async_runtime::spawn(async move {
        let mut session: Option<crate::asr::AsrSession> = None;
        let mut session_engine: Option<crate::asr::LoadedAsrEngine> = None;
        loop {
            // In the default low-overhead mode, keep Whisper entirely idle
            // during the call. Audio is still durably sealed every 20 seconds,
            // then processed as soon as recording stops.
            if !should_transcribe_now(transcribe_while_recording, stop.load(Ordering::Acquire)) {
                tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                continue;
            }
            let state = app.state::<AppState>();
            let chunk = meetings::claim_next_pending_chunk(&state.db, &meeting_id)
                .ok()
                .flatten();
            if let Some(chunk) = chunk {
                let engine = state
                    .engine
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                let Some(engine) = engine else {
                    let _ = meetings::requeue_chunk(&state.db, &chunk.id);
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    continue;
                };
                if session_engine
                    .as_ref()
                    .is_none_or(|old| !old.ptr_eq(&engine))
                {
                    match engine.create_session() {
                        Ok(new_session) => {
                            session = Some(new_session);
                            session_engine = Some(engine.clone());
                        }
                        Err(error) => {
                            let _ = meetings::fail_chunk(&state.db, &chunk.id, &error.to_string());
                            continue;
                        }
                    }
                }
                let Some(mut owned_session) = session.take() else {
                    let _ = meetings::requeue_chunk(&state.db, &chunk.id);
                    tokio::time::sleep(std::time::Duration::from_millis(350)).await;
                    continue;
                };
                let path = PathBuf::from(&chunk.audio_path);
                let result = tokio::task::spawn_blocking(move || {
                    let audio = read_pcm16(&path)?;
                    let result = engine
                        .transcribe_with_session(
                            &mut owned_session,
                            &audio,
                            &TranscriptionOptions::latency_first(false),
                        )
                        .map_err(|e| e.to_string());
                    Ok::<_, String>((owned_session, result))
                })
                .await;
                match result {
                    Ok(Ok((returned, Ok(transcription)))) => {
                        session = Some(returned);
                        let segments = transcription
                            .segments
                            .into_iter()
                            .map(|s| (s.start_ms, s.end_ms, s.text))
                            .collect::<Vec<_>>();
                        if let Err(error) = meetings::complete_chunk(&state.db, &chunk, &segments) {
                            let _ = meetings::fail_chunk(&state.db, &chunk.id, &error.to_string());
                        } else {
                            let _ = std::fs::remove_file(&chunk.audio_path);
                            let _ = app.emit("meeting-updated", &meeting_id);
                        }
                    }
                    Ok(Ok((returned, Err(error)))) => {
                        session = Some(returned);
                        let _ = meetings::fail_chunk(&state.db, &chunk.id, &error.to_string());
                    }
                    Ok(Err(error)) => {
                        // The task returned before handing the session back
                        // (e.g. `read_pcm16` failed) — it was dropped inside
                        // `spawn_blocking`. Clear the identity so the next
                        // iteration recreates a session instead of finding
                        // `session_engine` still matched and `session` stuck
                        // at `None` forever.
                        session_engine = None;
                        let _ = meetings::fail_chunk(&state.db, &chunk.id, &error);
                    }
                    Err(error) => {
                        // The blocking task panicked/was cancelled and the
                        // session was lost with it — same recovery as above.
                        session_engine = None;
                        let _ = meetings::fail_chunk(&state.db, &chunk.id, &error.to_string());
                    }
                }
                continue;
            }
            if stop.load(Ordering::Acquire) && capture_finished.load(Ordering::Acquire) {
                let state = app.state::<AppState>();
                if meetings::pending_count(&state.db, &meeting_id).unwrap_or(0) == 0 {
                    let failed = meetings::failed_count(&state.db, &meeting_id).unwrap_or(0);
                    let capture_error = meetings::get_meeting(&state.db, &meeting_id)
                        .ok()
                        .flatten()
                        .and_then(|meeting| meeting.error);
                    let (status, error) = if failed == 0 {
                        ("awaiting_summary", capture_error)
                    } else {
                        (
                            "error",
                            join_errors(
                                capture_error,
                                Some(format!(
                                    "{failed} audio chunk{} could not be transcribed",
                                    if failed == 1 { "" } else { "s" }
                                )),
                            ),
                        )
                    };
                    let _ = meetings::set_status(
                        &state.db,
                        &meeting_id,
                        status,
                        error.as_deref(),
                        true,
                    );
                    state.meeting.clear_if(&meeting_id);
                    emit_state(&app, &state.meeting.state());
                    crate::commands::meetings::hide_meeting_widget(&app);
                    let _ = app.emit("meeting-updated", &meeting_id);
                    let auto_summarize = meetings::get_provider_settings(&state.db)
                        .map(|settings| settings.auto_summarize)
                        .unwrap_or(false);
                    if failed == 0
                        && auto_summarize
                        && credentials::read_openrouter_key().ok().flatten().is_some()
                    {
                        let app_for_summary = app.clone();
                        let id = meeting_id.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = summarize_and_emit(&app_for_summary, &id, None).await;
                        });
                    }
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        }
    });
}

pub fn retry_transcription(app: &tauri::AppHandle, meeting_id: &str) -> Result<u64, String> {
    let state = app.state::<AppState>();
    if state.meeting.state().meeting_id.is_some() {
        return Err("Finish the active meeting before retrying an older transcript".into());
    }
    let retried =
        meetings::retry_failed_chunks(&state.db, meeting_id).map_err(|e| e.to_string())?;
    if retried == 0 {
        return Err("No recoverable failed audio chunks were found".into());
    }
    spawn_transcriber(
        app.clone(),
        meeting_id.to_string(),
        Arc::new(AtomicBool::new(true)),
        Arc::new(AtomicBool::new(true)),
        true,
    );
    let _ = app.emit("meeting-updated", meeting_id);
    Ok(retried)
}

/// Resume durable audio chunks that were sealed before a previous process
/// exited. Capture itself cannot survive a restart, but every completed spool
/// file can still be transcribed and summarized.
pub fn resume_recovered_chunks(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let ids = match meetings::pending_meeting_ids(&state.db) {
        Ok(ids) => ids,
        Err(error) => {
            eprintln!("Could not enumerate recoverable meeting audio: {error}");
            return;
        }
    };
    for meeting_id in ids {
        let _ = meetings::set_status(&state.db, &meeting_id, "transcribing", None, true);
        spawn_transcriber(
            app.clone(),
            meeting_id,
            Arc::new(AtomicBool::new(true)),
            Arc::new(AtomicBool::new(true)),
            true,
        );
    }
}

fn should_transcribe_now(transcribe_while_recording: bool, stop_requested: bool) -> bool {
    transcribe_while_recording || stop_requested
}

#[cfg(test)]
mod tests {
    use super::{capture_health_warning, finish_readiness_source, should_transcribe_now};
    use crate::audio::capture::AudioCapture;
    use crate::audio::types::AudioConfig;

    #[test]
    fn deferred_transcription_waits_until_recording_stops() {
        assert!(!should_transcribe_now(false, false));
        assert!(should_transcribe_now(false, true));
        assert!(should_transcribe_now(true, false));
    }

    #[test]
    fn readiness_distinguishes_endpoint_failure_from_silence() {
        let unavailable = finish_readiness_source(
            AudioCapture::new(AudioConfig::default()),
            Err("No capture device".into()),
            0.0,
        );
        assert!(!unavailable.available);
        assert!(!unavailable.signal_detected);
        assert_eq!(unavailable.error.as_deref(), Some("No capture device"));

        let silent =
            finish_readiness_source(AudioCapture::new(AudioConfig::default()), Ok(()), 0.01);
        assert!(silent.available);
        assert!(!silent.signal_detected);

        let audible =
            finish_readiness_source(AudioCapture::new(AudioConfig::default()), Ok(()), 0.08);
        assert!(audible.available);
        assert!(audible.signal_detected);
    }

    #[test]
    fn capture_health_waits_for_a_real_grace_period_and_identifies_the_missing_source() {
        assert_eq!(
            capture_health_warning(44_999, false, false, true, false, 0, None, None),
            None
        );
        assert!(
            capture_health_warning(45_000, false, false, true, false, 0, None, None)
                .unwrap()
                .contains("meeting audio")
        );
        assert!(
            capture_health_warning(45_000, false, false, false, true, 0, None, None)
                .unwrap()
                .contains("microphone audio")
        );
        assert_eq!(
            capture_health_warning(60_000, false, false, true, true, 60_000, None, None),
            None
        );
        assert_eq!(
            capture_health_warning(60_000, true, false, false, false, 0, None, None),
            None
        );
    }

    #[test]
    fn capture_backend_failures_are_reported_immediately() {
        let warning = capture_health_warning(
            1_000,
            false,
            false,
            false,
            false,
            0,
            None,
            Some("device invalidated"),
        )
        .unwrap();
        assert!(warning.contains("Meeting-audio capture stopped"));
        assert!(warning.contains("device invalidated"));
        assert_eq!(
            capture_health_warning(
                60_000,
                false,
                true,
                false,
                false,
                0,
                Some("ignored after stop"),
                None,
            ),
            None
        );
    }

    #[test]
    fn capture_health_warns_after_three_minutes_of_total_inactivity() {
        assert_eq!(
            capture_health_warning(239_999, false, false, true, true, 60_000, None, None,),
            None
        );
        let warning =
            capture_health_warning(240_000, false, false, true, true, 60_000, None, None).unwrap();
        assert!(warning.contains("three minutes"));
        assert!(warning.contains("end the meeting"));
    }
}

pub async fn summarize_and_emit(
    app: &tauri::AppHandle,
    meeting_id: &str,
    model: Option<&str>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    if !meetings::begin_summary(&state.db, meeting_id).map_err(|e| e.to_string())? {
        return Err("AI notes are already being generated for this meeting".into());
    }
    let _ = app.emit("meeting-updated", meeting_id);
    match provider::summarize_and_save(&state.db, meeting_id, model).await {
        Ok(_) => {
            let _ = app.emit("meeting-updated", meeting_id);
            let _ = app.emit("meeting-summary-ready", meeting_id);
            if let Some(widget) = app.get_webview_window("meeting-widget") {
                let _ = widget.hide();
            }
            Ok(())
        }
        Err(error) => {
            meetings::set_status(
                &state.db,
                meeting_id,
                "awaiting_summary",
                Some(&error),
                true,
            )
            .map_err(|e| e.to_string())?;
            let _ = app.emit("meeting-updated", meeting_id);
            let _ = app.emit("meeting-error", &error);
            Err(error)
        }
    }
}

fn spool_pair(
    state: &AppState,
    meeting_id: &str,
    elapsed_ms: u64,
    mic: Vec<f32>,
    system: Vec<f32>,
) -> Result<(), String> {
    let mic_error = spool_source(state, meeting_id, "mic", elapsed_ms, mic).err();
    let system_error = spool_source(state, meeting_id, "system", elapsed_ms, system).err();
    join_errors(mic_error, system_error).map_or(Ok(()), Err)
}

fn stop_capture_preserving_audio(capture: &mut AudioCapture) -> (Vec<f32>, Option<String>) {
    let mut samples = capture.drain_samples();
    match capture.stop() {
        Ok(tail) => {
            samples.extend(tail);
            (samples, None)
        }
        Err(error) => (samples, Some(error.to_string())),
    }
}

fn join_errors(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(first), Some(second)) if first != second => Some(format!("{first}; {second}")),
        (Some(error), _) | (_, Some(error)) => Some(error),
        (None, None) => None,
    }
}

fn spool_source(
    state: &AppState,
    meeting_id: &str,
    source: &str,
    elapsed_ms: u64,
    samples: Vec<f32>,
) -> Result<(), String> {
    if samples.len() < SAMPLE_RATE as usize / 2 || rms(&samples) < 0.001 {
        return Ok(());
    }
    let duration_ms = samples.len() as u64 * 1000 / SAMPLE_RATE;
    let start_ms = elapsed_ms.saturating_sub(duration_ms);
    let dir = state.data_dir.join("meetings").join(meeting_id);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        "{}-{}-{}.pcm",
        start_ms,
        source,
        uuid::Uuid::new_v4()
    ));
    write_pcm16(&path, &samples)?;
    if let Err(error) = meetings::add_chunk(
        &state.db,
        meeting_id,
        source,
        start_ms,
        elapsed_ms,
        &path.to_string_lossy(),
    ) {
        let _ = std::fs::remove_file(&path);
        return Err(error.to_string());
    }
    Ok(())
}

fn rms(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    (samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32).sqrt()
}

fn record_capture_activity(
    mic_seen: &AtomicBool,
    system_seen: &AtomicBool,
    last_activity_ms: &AtomicU64,
    elapsed_ms: u64,
    mic: &[f32],
    system: &[f32],
) {
    let mic_active = !mic.is_empty() && rms(mic) >= 0.001;
    let system_active = !system.is_empty() && rms(system) >= 0.001;
    if mic_active && mic.len() >= SAMPLE_RATE as usize / 2 {
        mic_seen.store(true, Ordering::Release);
    }
    if system_active && system.len() >= SAMPLE_RATE as usize / 2 {
        system_seen.store(true, Ordering::Release);
    }
    if mic_active || system_active {
        last_activity_ms.store(elapsed_ms, Ordering::Release);
    }
}

fn capture_health_warning(
    elapsed_ms: u64,
    paused: bool,
    stopped: bool,
    mic_seen: bool,
    system_seen: bool,
    last_audio_activity_ms: u64,
    mic_error: Option<&str>,
    system_error: Option<&str>,
) -> Option<String> {
    if stopped {
        return None;
    }
    if let Some(error) = mic_error {
        return Some(format!("Microphone capture stopped: {error}"));
    }
    if let Some(error) = system_error {
        return Some(format!("Meeting-audio capture stopped: {error}"));
    }
    if paused || elapsed_ms < 45_000 {
        return None;
    }
    match (mic_seen, system_seen) {
        (false, false) => Some("No microphone or meeting audio has been detected yet. Check your Windows input and output devices.".into()),
        (false, true) => Some("No microphone audio has been detected yet. Check that the correct input device is selected in Windows.".into()),
        (true, false) => Some("No meeting audio has been detected yet. Check that the call is playing through the Windows default output device.".into()),
        (true, true)
            if elapsed_ms.saturating_sub(last_audio_activity_ms) >= 3 * 60 * 1_000 =>
        {
            Some("No audio has been detected for three minutes. If the call ended, end the meeting to stop recording and finish the notes.".into())
        }
        (true, true) => None,
    }
}

fn write_pcm16(path: &Path, samples: &[f32]) -> Result<(), String> {
    let mut file = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    for sample in samples {
        file.write_all(&((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes())
            .map_err(|e| e.to_string())?;
    }
    file.flush().map_err(|e| e.to_string())
}

fn read_pcm16(path: &Path) -> Result<Vec<f32>, String> {
    let mut bytes = Vec::new();
    std::io::BufReader::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes
        .chunks_exact(2)
        .map(|pair| i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0)
        .collect())
}

fn emit_state(app: &tauri::AppHandle, payload: &MeetingStatePayload) {
    let _ = app.emit("meeting-state", payload);
}

trait IfEmpty {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str;
}
impl IfEmpty for str {
    fn if_empty<'a>(&'a self, fallback: &'a str) -> &'a str {
        if self.trim().is_empty() {
            fallback
        } else {
            self
        }
    }
}

/// A live call worth suggesting, resolved from the microphone-in-use signal
/// combined with a scan of every visible window title.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CallDetection {
    app: String,
    suggested_title: String,
    signature: String,
}

impl CallDetection {
    /// A window title named the service, so the call's own title is usable.
    fn from_window(app: &str, window_title: &str) -> Self {
        Self {
            app: app.to_string(),
            suggested_title: suggested_meeting_title(app, window_title),
            // The signature identifies the CALL, not the window: titles churn
            // with every clicked channel/tab (and a detection flaps between
            // the window and microphone paths), and a title-bearing signature
            // made each churn look like a brand-new call — resurfacing the
            // banner past both the cooldown and the user's dismissal. One app
            // has one live call at a time; the gate's miss streak separates
            // consecutive calls.
            signature: app.to_string(),
        }
    }

    /// Microphone-only hit: the app is capturing but no window title names the
    /// call (a Discord voice channel, a backgrounded Meet tab).
    fn from_microphone(app: &str) -> Self {
        Self {
            app: app.to_string(),
            // A label that already names the call kind stands on its own.
            suggested_title: if app.ends_with("Huddle") {
                app.to_string()
            } else {
                format!("{app} call")
            },
            signature: app.to_string(),
        }
    }
}

/// A visible top-level window seen by the detection scan.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CallWindow {
    process: String,
    title: String,
    /// Whether this is the window the user is actually looking at.
    foreground: bool,
}

/// How long a suggestion the user IGNORED (left open, never dismissed) stays
/// suppressed for the same call. An explicitly dismissed call never resurfaces
/// — see [`SuggestionGate::decline`].
const SUGGESTION_COOLDOWN: Duration = Duration::from_secs(600);

/// Decides when a polled detection is worth announcing. Kept separate from the
/// poll loop so the suppression rules are testable. Lives in a module static
/// ([`SUGGESTION_GATE`]) so the banner's dismiss command can record a decline.
#[derive(Default)]
struct SuggestionGate {
    last_signature: Option<String>,
    misses: u32,
    /// The user dismissed the banner for the current call: nothing with this
    /// signature is announced again until the call actually ends.
    declined: bool,
}

impl SuggestionGate {
    const fn new() -> Self {
        Self {
            last_signature: None,
            misses: 0,
            declined: false,
        }
    }

    /// Empty polls in a row before the remembered call is forgotten. The window
    /// scan can drop a title for a single tick, so a short streak (~75s at the
    /// 25s poll) separates a flap from a call that actually ended.
    const MISS_LIMIT: u32 = 3;

    /// `cooled_down` reports whether [`SUGGESTION_COOLDOWN`] has elapsed since
    /// the last emit. Returns true when the suggestion should be shown.
    fn observe(&mut self, signature: Option<&str>, cooled_down: bool) -> bool {
        let Some(signature) = signature else {
            self.misses += 1;
            if self.misses >= Self::MISS_LIMIT {
                // The call ended: a new one with the same signature is a new
                // call and must be suggested without waiting out the cooldown
                // or inheriting the old call's dismissal.
                self.last_signature = None;
                self.declined = false;
            }
            return false;
        };
        self.misses = 0;
        if self.last_signature.as_deref() == Some(signature) {
            if self.declined || !cooled_down {
                return false;
            }
        }
        self.last_signature = Some(signature.to_string());
        self.declined = false;
        true
    }

    /// The user dismissed the banner: keep the current call suppressed for as
    /// long as it lasts. No-op when no suggestion is outstanding.
    fn decline(&mut self) {
        if self.last_signature.is_some() {
            self.declined = true;
        }
    }
}

/// Shared with [`decline_current_suggestion`] so the banner's dismiss command
/// reaches the same gate the poll loop consults.
static SUGGESTION_GATE: Mutex<SuggestionGate> = Mutex::new(SuggestionGate::new());

/// Called by the suggestion banner's dismiss command: the current call stays
/// suppressed until it ends, instead of resurfacing on the next title change
/// or cooldown expiry.
pub fn decline_current_suggestion() {
    SUGGESTION_GATE
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .decline();
}

/// How often the idle call detector polls.
///
/// Every tick costs a microphone-consent registry walk, an `EnumWindows` sweep
/// and a full process snapshot — real work the app performs forever in the
/// background, on battery, whether or not a call ever happens.  25s still
/// notices a call within the first half-minute of it starting.
const CALL_DETECTION_POLL_SECS: u64 = 25;

/// Cached `auto_suggest`, read by the idle call-detection poll.
///
/// Idle cost: `get_provider_settings` takes the process-wide `Database`
/// connection mutex, so polling it contended with real work (history writes,
/// settings commits) for a flag that only changes when the user edits Meeting
/// settings.  [`invalidate_auto_suggest_cache`] is called whenever those are
/// saved, so the poll still observes every change.
static AUTO_SUGGEST_CACHE: RwLock<Option<bool>> = RwLock::new(None);

/// Drop the cached `auto_suggest`; the next poll re-reads it from SQLite.
pub fn invalidate_auto_suggest_cache() {
    if let Ok(mut cached) = AUTO_SUGGEST_CACHE.write() {
        *cached = None;
    }
}

fn auto_suggest_enabled(db: &crate::storage::database::Database) -> bool {
    if let Some(cached) = AUTO_SUGGEST_CACHE.read().ok().and_then(|cached| *cached) {
        return cached;
    }
    let read = || {
        meetings::get_provider_settings(db)
            .map(|s| s.auto_suggest)
            .unwrap_or(true)
    };
    // Hold the WRITE guard across the DB read on a miss. Releasing it in between
    // let an `invalidate_auto_suggest_cache` that landed mid-read be overwritten
    // by the value we had already fetched — the user's just-saved setting stayed
    // invisible until the next invalidation. Safe against deadlock: the only
    // invalidator (`save_meeting_provider_settings`) releases the `Database`
    // mutex before it calls in here, so the two locks are never held crosswise.
    let Ok(mut cached) = AUTO_SUGGEST_CACHE.write() else {
        return read();
    };
    if let Some(value) = *cached {
        return value;
    }
    let enabled = read();
    *cached = Some(enabled);
    enabled
}

pub fn start_call_detection(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        // `None` means "nothing suggested yet", which permits the first
        // suggestion immediately. NOT `Instant::now() - SUGGESTION_COOLDOWN`:
        // on Windows `Instant` is boot-relative (QPC since system start), so
        // subtracting the cooldown PANICS when the app launches within
        // `SUGGESTION_COOLDOWN` of boot — i.e. exactly on a start-with-Windows
        // install.
        let mut last_emitted: Option<Instant> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(CALL_DETECTION_POLL_SECS)).await;
            let state = app.state::<AppState>();
            // Ordinary dictation owns the microphone too — never read our own
            // capture as somebody else's call.
            let dictating = state
                .capture
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .is_some();
            if state.meeting.state().meeting_id.is_some()
                || dictating
                || !auto_suggest_enabled(&state.db)
            {
                continue;
            }
            let detection = tokio::task::spawn_blocking(detect_active_call)
                .await
                .ok()
                .flatten();
            let should_emit = SUGGESTION_GATE
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .observe(
                    detection.as_ref().map(|found| found.signature.as_str()),
                    last_emitted.is_none_or(|at| at.elapsed() > SUGGESTION_COOLDOWN),
                );
            let Some(detection) = detection.filter(|_| should_emit) else {
                continue;
            };
            last_emitted = Some(Instant::now());
            let payload = serde_json::json!({
                "app": detection.app,
                "suggested_title": detection.suggested_title
            });
            if show_suggestion_window(&app).await.is_ok() {
                tokio::time::sleep(std::time::Duration::from_millis(220)).await;
                let _ = app.emit("meeting-suggestion", payload);
            }
        }
    });
}

fn detect_active_call() -> Option<CallDetection> {
    resolve_call_detection(
        &microphone_processes_in_use(),
        &visible_windows(),
        &running_process_names(),
        own_process_name().as_deref(),
    )
}

/// Decide whether a call is live. Microphone ownership is the primary signal —
/// it fires for a backgrounded browser tab that never puts "meeting" in a window
/// title — and the window scan names the service. Every rule here demands two
/// pieces of evidence, because each signal on its own has a standing false
/// positive: a consent-store entry survives the app that opened it, apps that
/// idle-hold the microphone never release it, and a call-service window title
/// says nothing about whether the user is in that call.
fn resolve_call_detection(
    mic_processes: &[String],
    windows: &[CallWindow],
    running_processes: &[String],
    own_process: Option<&str>,
) -> Option<CallDetection> {
    let titled = windows.iter().find_map(|window| {
        detected_call_app(&window.process, &window.title).map(|app| (app, window))
    });
    let mut browser_on_mic = false;
    for process in mic_processes {
        if is_own_process(process, own_process) || !mic_owner_is_running(process, running_processes)
        {
            continue;
        }
        if let Some(app) = native_call_app_for_process(process) {
            if !microphone_proves_a_call(app, titled.map(|(app, _)| app), windows) {
                continue;
            }
            return Some(match titled.filter(|(detected, _)| *detected == app) {
                Some((app, window)) => CallDetection::from_window(app, &window.title),
                None => CallDetection::from_microphone(app),
            });
        }
        browser_on_mic |= is_browser_process(process);
    }
    // A browser holding the microphone is only a call once some window names the
    // service. Web apps keep persistent microphone permission for dictation and
    // voice notes, so browser capture on its own is not evidence of a call.
    if browser_on_mic {
        if let Some((app, window)) = titled {
            return Some(CallDetection::from_window(app, &window.title));
        }
    }
    // Nothing corroborates the title, so fall back to the pre-rework bar: the
    // call has to be the window the user is looking at. A background window
    // merely titled after a call service is not a call in progress.
    titled
        .filter(|(_, window)| window.foreground)
        .map(|(app, window)| CallDetection::from_window(app, &window.title))
}

/// Discord opens a microphone capture session for as long as it is running, not
/// only while the user is in a voice channel, so its consent-store entry alone
/// proves nothing. Its window title tracks the selected text channel and never
/// names the voice call, which leaves "the user is looking at Discord" as the
/// only usable corroboration. Every other native call app opens the microphone
/// only for an actual call.
fn microphone_proves_a_call(app: &str, titled_app: Option<&str>, windows: &[CallWindow]) -> bool {
    if app != "Discord" {
        return true;
    }
    titled_app == Some("Discord")
        || windows
            .iter()
            .any(|window| window.foreground && process_stem(&window.process).starts_with("discord"))
}

/// Windows never stamps `LastUsedTimeStop` for a process that died while
/// capturing, so a crashed app looks microphone-active forever. Require the app
/// behind the entry to still be running. Packaged entries are package family
/// names rather than executables, so they match on the call app they resolve to
/// (`MSTeams_8wekyb3d8bbwe` against a running `ms-teams.exe`).
fn mic_owner_is_running(process: &str, running_processes: &[String]) -> bool {
    let stem = process_stem(process);
    if running_processes
        .iter()
        .any(|running| process_stem(running) == stem)
    {
        return true;
    }
    match native_call_app_for_process(process) {
        Some(app) => running_processes
            .iter()
            .any(|running| native_call_app_for_process(running) == Some(app)),
        None => false,
    }
}

/// Lowercased executable name without its directory or `.exe` suffix. Packaged
/// apps have no path, so their package family name is the stem.
fn process_stem(process: &str) -> String {
    let lower = process.to_ascii_lowercase();
    let leaf = lower.rsplit(['\\', '/']).next().unwrap_or(lower.as_str());
    leaf.strip_suffix(".exe").unwrap_or(leaf).to_string()
}

/// OmniVox's own dictation holds the microphone, so our consent-store entry must
/// never be read as somebody else's call.
fn is_own_process(process: &str, own_process: Option<&str>) -> bool {
    let stem = process_stem(process);
    own_process.is_some_and(|own| process_stem(own) == stem)
        || matches!(stem.as_str(), "omnivox" | "omnivoice")
}

/// The call app behind a microphone-capturing process, matched on the
/// executable or package name rather than a window title.
fn native_call_app_for_process(process: &str) -> Option<&'static str> {
    let stem = process_stem(process);
    if stem.starts_with("discord") {
        Some("Discord")
    } else if stem.starts_with("zoom") || stem == "cpthost" {
        Some("Zoom")
    } else if stem.starts_with("teams") || stem.starts_with("ms-teams") || stem.contains("msteams")
    {
        Some("Microsoft Teams")
    } else if stem.starts_with("slack") {
        Some("Slack Huddle")
    } else if stem.starts_with("skype") || stem.contains("skypeapp") {
        Some("Skype")
    } else if stem.contains("webex") || stem.starts_with("atmgr") {
        Some("Webex")
    } else {
        None
    }
}

/// Browsers hold the microphone for web calls, so the process only proves a call
/// is live — a window title has to name the service.
fn is_browser_process(process: &str) -> bool {
    matches!(
        process_stem(process).as_str(),
        "chrome"
            | "chromium"
            | "msedge"
            | "firefox"
            | "brave"
            | "arc"
            | "opera"
            | "opera_gx"
            | "vivaldi"
    )
}

/// Consent-store subkeys encode an executable path with `#` in place of `\`.
#[cfg(any(windows, test))]
fn consent_key_process_name(key_name: &str) -> String {
    key_name
        .replace('#', "\\")
        .rsplit('\\')
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Windows leaves `LastUsedTimeStop` at zero for as long as a process is
/// actually capturing, and only stamps it once capture ends.
#[cfg(any(windows, test))]
fn microphone_in_use(last_used_start: Option<u64>, last_used_stop: Option<u64>) -> bool {
    matches!((last_used_start, last_used_stop), (Some(start), Some(0)) if start != 0)
}

#[cfg(windows)]
const MICROPHONE_CONSENT_STORE: &str = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone";

/// Executables and packages that hold the microphone right now, read from the
/// Capability Access Manager consent store. This is the cheap, COM-free way to
/// see that a call is live even when nothing about it is on screen.
#[cfg(windows)]
fn microphone_processes_in_use() -> Vec<String> {
    use windows_sys::Win32::System::Registry::HKEY_CURRENT_USER;

    let mut processes = Vec::new();
    let Some(root) = open_registry_key(HKEY_CURRENT_USER, MICROPHONE_CONSENT_STORE) else {
        return processes;
    };
    for name in registry_subkey_names(root) {
        let Some(key) = open_registry_key(root, &name) else {
            continue;
        };
        if name.eq_ignore_ascii_case("NonPackaged") {
            for exe in registry_subkey_names(key) {
                let Some(child) = open_registry_key(key, &exe) else {
                    continue;
                };
                if microphone_key_in_use(child) {
                    processes.push(consent_key_process_name(&exe));
                }
                close_registry_key(child);
            }
        } else if microphone_key_in_use(key) {
            processes.push(consent_key_process_name(&name));
        }
        close_registry_key(key);
    }
    close_registry_key(root);
    processes
}

#[cfg(windows)]
fn microphone_key_in_use(key: HKEY) -> bool {
    microphone_in_use(
        registry_qword(key, "LastUsedTimeStart"),
        registry_qword(key, "LastUsedTimeStop"),
    )
}

#[cfg(windows)]
fn open_registry_key(parent: HKEY, path: &str) -> Option<HKEY> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegOpenKeyExW, KEY_READ};

    let path = wide(path);
    let mut key: HKEY = std::ptr::null_mut();
    let status = unsafe { RegOpenKeyExW(parent, path.as_ptr(), 0, KEY_READ, &mut key) };
    (status == ERROR_SUCCESS).then_some(key)
}

#[cfg(windows)]
fn registry_subkey_names(key: HKEY) -> Vec<String> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::RegEnumKeyExW;

    let mut names = Vec::new();
    for index in 0..512u32 {
        let mut buffer = [0u16; 512];
        let mut length = buffer.len() as u32;
        let status = unsafe {
            RegEnumKeyExW(
                key,
                index,
                buffer.as_mut_ptr(),
                &mut length,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        if status != ERROR_SUCCESS {
            break;
        }
        names.push(String::from_utf16_lossy(&buffer[..length as usize]));
    }
    names
}

#[cfg(windows)]
fn registry_qword(key: HKEY, name: &str) -> Option<u64> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::RegQueryValueExW;

    let name = wide(name);
    let mut value = 0u64;
    let mut size = std::mem::size_of::<u64>() as u32;
    let status = unsafe {
        RegQueryValueExW(
            key,
            name.as_ptr(),
            std::ptr::null(),
            std::ptr::null_mut(),
            (&mut value as *mut u64).cast::<u8>(),
            &mut size,
        )
    };
    (status == ERROR_SUCCESS && size as usize == std::mem::size_of::<u64>()).then_some(value)
}

#[cfg(windows)]
fn close_registry_key(key: HKEY) {
    use windows_sys::Win32::System::Registry::RegCloseKey;
    let _ = unsafe { RegCloseKey(key) };
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn own_process_name() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
}

/// Every visible, titled top-level window, the process that owns it, and whether
/// it is the foreground window. The full scan is what names a call the user has
/// tabbed away from; the foreground flag is what keeps a background window's
/// title from being read as a call in progress. OmniVox's own windows are
/// skipped.
#[cfg(windows)]
fn visible_windows() -> Vec<CallWindow> {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Scan {
        windows: Vec<CallWindow>,
        foreground: HWND,
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let scan = unsafe { &mut *(lparam as *mut Scan) };
        if scan.windows.len() >= 400 {
            return 0;
        }
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let length = GetWindowTextLengthW(hwnd);
            if length <= 0 {
                return 1;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 || pid == GetCurrentProcessId() {
                return 1;
            }
            let mut buffer = vec![0u16; length as usize + 1];
            let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            if copied <= 0 {
                return 1;
            }
            let Some(process) = crate::focus::get_process_name_from_hwnd(hwnd as isize) else {
                return 1;
            };
            scan.windows.push(CallWindow {
                process,
                title: String::from_utf16_lossy(&buffer[..copied as usize]),
                foreground: hwnd == scan.foreground,
            });
        }
        1
    }

    let mut scan = Scan {
        windows: Vec::new(),
        foreground: unsafe { GetForegroundWindow() },
    };
    unsafe {
        EnumWindows(Some(collect), &mut scan as *mut _ as LPARAM);
    }
    scan.windows
}

/// Executable names of every live process, used to discard consent-store entries
/// whose owner is gone.
#[cfg(windows)]
fn running_process_names() -> Vec<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let mut names = Vec::new();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let end = entry
                    .szExeFile
                    .iter()
                    .position(|unit| *unit == 0)
                    .unwrap_or(entry.szExeFile.len());
                names.push(String::from_utf16_lossy(&entry.szExeFile[..end]));
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
    }
    names
}

/// Call detection reads Windows-only signals (the Capability Access Manager
/// consent store and the desktop window list); elsewhere it stays silent.
#[cfg(not(windows))]
fn microphone_processes_in_use() -> Vec<String> {
    Vec::new()
}

#[cfg(not(windows))]
fn visible_windows() -> Vec<CallWindow> {
    Vec::new()
}

#[cfg(not(windows))]
fn running_process_names() -> Vec<String> {
    Vec::new()
}

#[cfg(not(windows))]
fn own_process_name() -> Option<String> {
    None
}

fn detected_call_app(process: &str, title: &str) -> Option<&'static str> {
    let process = process.to_lowercase();
    let haystack = format!("{} {}", process, title.to_lowercase());
    if haystack.contains("meet.google") || haystack.contains("google meet") {
        Some("Google Meet")
    } else if haystack.contains("zoom")
        && (haystack.contains("meeting") || process.contains("zoom"))
    {
        Some("Zoom")
    } else if haystack.contains("teams")
        && (haystack.contains("meeting") || haystack.contains("call"))
    {
        Some("Microsoft Teams")
    } else if haystack.contains("discord")
        && (haystack.contains("voice") || haystack.contains("call"))
    {
        Some("Discord")
    } else if haystack.contains("skype") && (haystack.contains("call") || process.contains("skype"))
    {
        Some("Skype")
    } else if haystack.contains("webex")
        && (haystack.contains("meeting") || haystack.contains("call"))
    {
        Some("Webex")
    } else if haystack.contains("slack") && haystack.contains("huddle") {
        Some("Slack Huddle")
    } else {
        None
    }
}

fn suggested_meeting_title(app: &str, window_title: &str) -> String {
    let mut title = window_title
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .to_string();
    if title.starts_with('(') {
        if let Some(end) = title.find(") ") {
            title = title[end + 2..].trim().to_string();
        }
    }
    for suffix in [
        " - Google Meet",
        " | Google Meet",
        " - Zoom Meeting",
        " | Zoom Meeting",
        " - Zoom",
        " | Zoom",
        " - Microsoft Teams",
        " | Microsoft Teams",
        " - Skype",
        " | Skype",
        " - Webex",
        " | Webex",
        " - Slack",
        " | Slack",
    ] {
        if title.to_lowercase().ends_with(&suffix.to_lowercase()) {
            title.truncate(title.len() - suffix.len());
            title = title.trim().to_string();
            break;
        }
    }
    let generic = title.is_empty()
        || title.eq_ignore_ascii_case(app)
        || matches!(
            title.to_ascii_lowercase().as_str(),
            "google meet"
                | "zoom"
                | "zoom workplace"
                | "microsoft teams"
                | "skype"
                | "webex"
                | "slack huddle"
        );
    if generic {
        format!("{app} meeting")
    } else {
        title.chars().take(96).collect()
    }
}

#[cfg(test)]
mod meeting_detection_tests {
    use super::{
        consent_key_process_name, detected_call_app, is_browser_process, is_own_process,
        mic_owner_is_running, microphone_in_use, native_call_app_for_process,
        resolve_call_detection, suggested_meeting_title, CallDetection, CallWindow, SuggestionGate,
    };

    /// Background windows: nothing here is the one the user is looking at.
    fn windows(entries: &[(&str, &str)]) -> Vec<CallWindow> {
        entries
            .iter()
            .map(|(process, title)| CallWindow {
                process: process.to_string(),
                title: title.to_string(),
                foreground: false,
            })
            .collect()
    }

    /// The same list with the last entry marked as the foreground window.
    fn windows_focused_on_last(entries: &[(&str, &str)]) -> Vec<CallWindow> {
        let mut scanned = windows(entries);
        if let Some(last) = scanned.last_mut() {
            last.foreground = true;
        }
        scanned
    }

    fn mic(processes: &[&str]) -> Vec<String> {
        processes.iter().map(|value| value.to_string()).collect()
    }

    fn running(processes: &[&str]) -> Vec<String> {
        processes.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn recognizes_supported_call_surfaces_without_generic_false_positives() {
        assert_eq!(
            detected_call_app("chrome.exe", "Product sync - meet.google.com"),
            Some("Google Meet")
        );
        assert_eq!(
            detected_call_app("Zoom.exe", "Zoom Workplace"),
            Some("Zoom")
        );
        assert_eq!(
            detected_call_app("ms-teams.exe", "Project call"),
            Some("Microsoft Teams")
        );
        assert_eq!(detected_call_app("Skype.exe", "Skype"), Some("Skype"));
        assert_eq!(
            detected_call_app("slack.exe", "Design huddle"),
            Some("Slack Huddle")
        );
        assert_eq!(detected_call_app("chrome.exe", "Zoom pricing page"), None);
    }

    #[test]
    fn meeting_suggestions_use_the_call_title_without_app_chrome() {
        assert_eq!(
            suggested_meeting_title("Google Meet", "Weekly Product Sync - Google Meet"),
            "Weekly Product Sync"
        );
        assert_eq!(
            suggested_meeting_title("Microsoft Teams", "(2) Launch review | Microsoft Teams"),
            "Launch review"
        );
        assert_eq!(
            suggested_meeting_title("Zoom", "Zoom Workplace"),
            "Zoom meeting"
        );
        assert_eq!(suggested_meeting_title("Skype", "\n\t"), "Skype meeting");
    }

    #[test]
    fn consent_store_keys_decode_back_to_an_executable_name() {
        assert_eq!(
            consent_key_process_name("C:#Program Files#Discord#app-1.0.9013#Discord.exe"),
            "Discord.exe"
        );
        assert_eq!(
            consent_key_process_name("Microsoft.SkypeApp_kzf8qxf38zg5c"),
            "Microsoft.SkypeApp_kzf8qxf38zg5c"
        );
    }

    #[test]
    fn a_zero_stop_stamp_with_a_real_start_stamp_means_the_microphone_is_live() {
        assert!(microphone_in_use(Some(133_700_000_000_000_000), Some(0)));
        assert!(!microphone_in_use(
            Some(133_700_000_000_000_000),
            Some(133_700_000_000_000_001)
        ));
        assert!(!microphone_in_use(Some(0), Some(0)));
        assert!(!microphone_in_use(None, Some(0)));
        assert!(!microphone_in_use(Some(133_700_000_000_000_000), None));
    }

    #[test]
    fn microphone_owners_are_classified_by_executable_not_window_title() {
        assert_eq!(native_call_app_for_process("Discord.exe"), Some("Discord"));
        assert_eq!(native_call_app_for_process("Zoom.exe"), Some("Zoom"));
        assert_eq!(
            native_call_app_for_process("ms-teams.exe"),
            Some("Microsoft Teams")
        );
        assert_eq!(
            native_call_app_for_process("MSTeams_8wekyb3d8bbwe"),
            Some("Microsoft Teams")
        );
        assert_eq!(
            native_call_app_for_process("slack.exe"),
            Some("Slack Huddle")
        );
        assert_eq!(
            native_call_app_for_process("Microsoft.SkypeApp_kzf8qxf38zg5c"),
            Some("Skype")
        );
        assert_eq!(native_call_app_for_process("chrome.exe"), None);
        assert!(is_browser_process("C:\\Program Files\\Google\\chrome.exe"));
        assert!(is_browser_process("msedge.exe"));
        assert!(!is_browser_process("Discord.exe"));
        assert!(is_own_process("omnivoice.exe", Some("OmniVoice.exe")));
        assert!(is_own_process("OmniVox.exe", None));
        assert!(!is_own_process("Discord.exe", Some("omnivoice.exe")));
    }

    #[test]
    fn a_microphone_owner_is_suggested_even_when_no_window_title_names_the_call() {
        // A Zoom call the user has tabbed away from: the window title is
        // generic, but Zoom only opens the microphone for an actual call.
        let detection = resolve_call_detection(
            &mic(&["Zoom.exe"]),
            &windows(&[("explorer.exe", "Downloads")]),
            &running(&["Zoom.exe", "explorer.exe"]),
            Some("omnivoice.exe"),
        )
        .unwrap();
        assert_eq!(detection.app, "Zoom");
        assert_eq!(detection.suggested_title, "Zoom call");
        // Signatures identify the call by app, stable across detection paths.
        assert_eq!(detection.signature, "Zoom");
    }

    #[test]
    fn discord_holding_the_microphone_while_idle_is_not_a_call() {
        // Discord keeps a capture session open the whole time it runs, so the
        // consent-store entry on its own must never fire.
        assert_eq!(
            resolve_call_detection(
                &mic(&["Discord.exe"]),
                &windows(&[("Discord.exe", "#general | Omni Impact - Discord")]),
                &running(&["Discord.exe"]),
                Some("omnivoice.exe"),
            ),
            None
        );

        // The user looking at Discord while it captures is the usable signal.
        let focused = resolve_call_detection(
            &mic(&["Discord.exe"]),
            &windows_focused_on_last(&[("Discord.exe", "#general | Omni Impact - Discord")]),
            &running(&["Discord.exe"]),
            Some("omnivoice.exe"),
        )
        .unwrap();
        assert_eq!(focused.app, "Discord");
        assert_eq!(focused.suggested_title, "Discord call");

        // So is a title that names the voice call, even in the background.
        let titled = resolve_call_detection(
            &mic(&["Discord.exe"]),
            &windows(&[("Discord.exe", "Voice connected - Discord")]),
            &running(&["Discord.exe"]),
            Some("omnivoice.exe"),
        )
        .unwrap();
        assert_eq!(titled.app, "Discord");
    }

    #[test]
    fn a_consent_entry_whose_process_has_died_is_ignored() {
        // Windows leaves LastUsedTimeStop at zero when a capturing app crashes,
        // so the entry outlives the process forever.
        assert_eq!(
            resolve_call_detection(
                &mic(&["Zoom.exe"]),
                &windows(&[("explorer.exe", "Downloads")]),
                &running(&["explorer.exe"]),
                Some("omnivoice.exe"),
            ),
            None
        );

        // A packaged entry is a family name, so it matches the running app it
        // resolves to rather than an executable of the same name.
        assert!(mic_owner_is_running(
            "MSTeams_8wekyb3d8bbwe",
            &running(&["ms-teams.exe"])
        ));
        assert!(mic_owner_is_running(
            "chrome.exe",
            &running(&["C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"])
        ));
        assert!(!mic_owner_is_running(
            "chrome.exe",
            &running(&["msedge.exe"])
        ));
    }

    #[test]
    fn a_browser_on_the_microphone_is_named_by_any_window_title_not_just_the_foreground() {
        let detection = resolve_call_detection(
            &mic(&["chrome.exe"]),
            &windows(&[
                ("explorer.exe", "Downloads"),
                ("chrome.exe", "Product sync - meet.google.com"),
            ]),
            &running(&["chrome.exe", "explorer.exe"]),
            Some("omnivoice.exe"),
        )
        .unwrap();
        assert_eq!(detection.app, "Google Meet");
        assert_eq!(detection.suggested_title, "Product sync - meet.google.com");

        // Browser capture with no call service named anywhere is a dictation
        // extension or a voice-note site, not a meeting.
        assert_eq!(
            resolve_call_detection(
                &mic(&["chrome.exe"]),
                &windows(&[("chrome.exe", "Inbox")]),
                &running(&["chrome.exe"]),
                Some("omnivoice.exe"),
            ),
            None
        );
    }

    #[test]
    fn an_uncorroborated_title_only_fires_for_the_window_the_user_is_looking_at() {
        // A backgrounded tab left on a call service is not a call in progress.
        assert_eq!(
            resolve_call_detection(
                &[],
                &windows(&[
                    ("chrome.exe", "Weekly Product Sync - Google Meet"),
                    ("code.exe", "mod.rs - OmniVox"),
                ]),
                &running(&["chrome.exe", "code.exe"]),
                Some("omnivoice.exe"),
            ),
            None
        );

        // The same window in front of the user still counts, exactly as before.
        let detection = resolve_call_detection(
            &[],
            &windows_focused_on_last(&[
                ("explorer.exe", "Downloads"),
                ("chrome.exe", "Weekly Product Sync - Google Meet"),
            ]),
            &running(&["chrome.exe", "explorer.exe"]),
            Some("omnivoice.exe"),
        )
        .unwrap();
        assert_eq!(detection.app, "Google Meet");
        assert_eq!(detection.suggested_title, "Weekly Product Sync");
    }

    #[test]
    fn omnivox_holding_the_microphone_is_never_a_call() {
        assert_eq!(
            resolve_call_detection(
                &mic(&["omnivoice.exe"]),
                &windows(&[("explorer.exe", "Downloads")]),
                &running(&["omnivoice.exe", "explorer.exe"]),
                Some("omnivoice.exe"),
            ),
            None
        );
    }

    #[test]
    fn a_call_that_ended_is_suggested_again_without_waiting_out_the_cooldown() {
        let mut gate = SuggestionGate::default();
        assert!(gate.observe(Some("Zoom:microphone"), false));

        // The same live call stays suppressed while it keeps being detected,
        // including after the user dismissed the banner.
        assert!(!gate.observe(Some("Zoom:microphone"), false));
        assert!(!gate.observe(Some("Zoom:microphone"), false));

        // A one-tick flap of the window scan must not rearm the suggestion.
        assert!(!gate.observe(None, false));
        assert!(!gate.observe(Some("Zoom:microphone"), false));

        // The call really ended, so the next one re-emits immediately.
        for _ in 0..SuggestionGate::MISS_LIMIT {
            assert!(!gate.observe(None, false));
        }
        assert!(gate.observe(Some("Zoom:microphone"), false));

        // A call still running when the cooldown expires is announced again.
        assert!(!gate.observe(Some("Zoom:microphone"), false));
        assert!(gate.observe(Some("Zoom:microphone"), true));

        // A different call is always announced.
        assert!(gate.observe(Some("Discord:microphone"), false));
    }

    #[test]
    fn a_dismissed_call_never_resurfaces_until_it_ends() {
        let mut gate = SuggestionGate::default();
        assert!(gate.observe(Some("Discord"), false));
        gate.decline();

        // Neither continued detection nor cooldown expiry resurfaces a
        // dismissed call — this was the "banner comes back every time I click
        // the app" bug.
        assert!(!gate.observe(Some("Discord"), true));
        assert!(!gate.observe(Some("Discord"), true));

        // A one-tick scan flap doesn't clear the decline either.
        assert!(!gate.observe(None, true));
        assert!(!gate.observe(Some("Discord"), true));

        // Once the call really ends, the decline is forgotten and the next
        // call announces immediately.
        for _ in 0..SuggestionGate::MISS_LIMIT {
            assert!(!gate.observe(None, false));
        }
        assert!(gate.observe(Some("Discord"), false));
    }

    #[test]
    fn declining_with_no_outstanding_suggestion_does_not_suppress_the_next_call() {
        let mut gate = SuggestionGate::default();
        gate.decline();
        assert!(gate.observe(Some("Zoom"), false));
    }

    #[test]
    fn call_signatures_are_stable_across_title_churn_and_detection_paths() {
        // Discord's title tracks the selected text channel and a detection
        // alternates between the window and microphone paths — none of that
        // may mint a "new call".
        assert_eq!(
            CallDetection::from_window("Discord", "#general — Dev Server").signature,
            CallDetection::from_window("Discord", "#voice-chat — Dev Server").signature,
        );
        assert_eq!(
            CallDetection::from_window("Discord", "#general — Dev Server").signature,
            CallDetection::from_microphone("Discord").signature,
        );
    }
}

async fn show_suggestion_window(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};
    if let Some(win) = app.get_webview_window("meeting-suggestion") {
        let _ = win.show();
        return Ok(());
    }
    let (w, h) = (372.0, 124.0);
    let (x, y) = if let Ok(Some(monitor)) = app.primary_monitor() {
        let scale = monitor.scale_factor();
        let area = monitor.work_area();
        let ax = area.position.x as f64 / scale;
        let ay = area.position.y as f64 / scale;
        let aw = area.size.width as f64 / scale;
        (ax + aw - w - 18.0, ay + 18.0)
    } else {
        (700.0, 30.0)
    };
    WebviewWindowBuilder::new(
        app,
        "meeting-suggestion",
        WebviewUrl::App("/meeting-suggestion.html".into()),
    )
    .title("")
    .inner_size(w, h)
    .position(x, y)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(true)
    .build()
    .map_err(|e| e.to_string())?;
    Ok(())
}
