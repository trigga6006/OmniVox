use tauri::{Manager, State};

use super::auth::{require_caller, WindowPolicy};
use crate::audio::capture::AudioCapture;
use crate::audio::types::AudioDevice;
use crate::state::AppState;

/// Open the OS-specific privacy settings for microphone access.
/// On macOS this opens System Settings → Privacy & Security → Microphone.
/// On Windows/Linux this is a no-op (permissions are granted at the OS level).
#[tauri::command]
pub async fn open_mic_settings(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn()
            .map_err(|e| format!("Failed to open System Settings: {e}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "ms-settings:privacy-microphone"])
            .spawn();
    }

    Ok(())
}

/// Open the OS-specific Accessibility settings.
/// On macOS the global hotkey requires Accessibility permission via rdev.
#[tauri::command]
pub async fn open_accessibility_settings(caller: tauri::WebviewWindow) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn()
            .map_err(|e| format!("Failed to open System Settings: {e}"))?;
    }

    Ok(())
}

/// Check the current platform and return permission guidance.
#[tauri::command]
pub async fn get_platform_info(caller: tauri::WebviewWindow) -> Result<PlatformInfo, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    Ok(PlatformInfo {
        os: std::env::consts::OS.to_string(),
        needs_mic_permission: cfg!(target_os = "macos"),
        needs_accessibility_permission: cfg!(target_os = "macos"),
    })
}

#[derive(serde::Serialize)]
pub struct PlatformInfo {
    pub os: String,
    pub needs_mic_permission: bool,
    pub needs_accessibility_permission: bool,
}

#[tauri::command]
pub async fn get_pipeline_traces(
    caller: tauri::WebviewWindow,
) -> Result<Vec<crate::perf::PipelineTrace>, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    Ok(crate::perf::recent())
}

#[tauri::command]
pub async fn start_recording(
    caller: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::AllAppWindows)?;
    // pipeline::start_recording is fully synchronous — a SQLite settings read,
    // auto-switch queries, cross-process COM ducking, and opening the mic device.
    // Run it on the blocking pool so it never stalls this tokio worker.
    let app = app_handle.clone();
    // Propagate a panic from the blocking body as a command error instead of
    // swallowing the JoinError — otherwise a mic/COM failure that unwinds would
    // return Ok(()) and the UI would show a stuck "recording" that never started.
    tokio::task::spawn_blocking(move || {
        let st = app.state::<AppState>();
        crate::pipeline::start_recording(&app, &st);
    })
    .await
    .map_err(|e| format!("start_recording failed: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(
    caller: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    require_caller(&caller, WindowPolicy::AllAppWindows)?;
    crate::pipeline::stop_recording(&app_handle, &state).await;
    Ok("ok".into())
}

#[tauri::command]
pub async fn cancel_recording(
    caller: tauri::WebviewWindow,
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    crate::pipeline::cancel_recording(&app_handle, &state);
    Ok(())
}

#[tauri::command]
pub async fn get_audio_devices(caller: tauri::WebviewWindow) -> Result<Vec<AudioDevice>, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    tokio::task::spawn_blocking(AudioCapture::enumerate_devices)
        .await
        .map_err(|e| format!("Audio device enumeration failed: {e}"))?
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_selected_audio_device(
    caller: tauri::WebviewWindow,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    require_caller(&caller, WindowPolicy::Main)?;
    let audio = state
        .audio
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Ok(audio.config().device_id.clone())
}

#[tauri::command]
pub async fn set_audio_device(
    caller: tauri::WebviewWindow,
    device_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    require_caller(&caller, WindowPolicy::Main)?;
    if device_id.trim().is_empty() || device_id.chars().count() > 1_000 {
        return Err("Invalid microphone identifier".into());
    }
    let capture_guard = state.capture.lock().unwrap_or_else(|p| p.into_inner());
    if capture_guard.is_some() {
        return Err("Cannot change microphones while recording or processing a stop".into());
    }
    // Skip pre-validation: the UI only lets users pick devices that came from
    // a fresh `get_audio_devices()` call, and the cpal backend will return a
    // clear error at `start()` time if the device is gone (e.g. unplugged).
    // The old enumerate-on-every-change pattern cost up to 3 seconds (device
    // enumeration timeout) every time the user selected a mic — on Windows
    // enumerating WASAPI devices can hang briefly, which made the Settings
    // dropdown feel laggy.
    let config = crate::audio::types::AudioConfig {
        device_id: Some(device_id.clone()),
        ..Default::default()
    };
    crate::storage::settings::set_audio_device_id(&state.db, &device_id)
        .map_err(|error| error.to_string())?;
    let mut audio = match state.audio.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    *audio = AudioCapture::new(config);
    drop(capture_guard);
    Ok(())
}
