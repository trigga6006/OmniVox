use tauri::State;

use crate::audio::capture::AudioCapture;
use crate::audio::types::AudioDevice;
use crate::state::AppState;

/// Open the OS-specific privacy settings for microphone access.
/// On macOS this opens System Settings → Privacy & Security → Microphone.
/// On Windows/Linux this is a no-op (permissions are granted at the OS level).
#[tauri::command]
pub async fn open_mic_settings() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
            .spawn()
            .map_err(|e| format!("Failed to open System Settings: {e}"))?;
    }

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("ms-settings:privacy-microphone")
            .spawn();
    }

    Ok(())
}

/// Open the OS-specific Accessibility settings.
/// On macOS the global hotkey requires Accessibility permission via rdev.
#[tauri::command]
pub async fn open_accessibility_settings() -> Result<(), String> {
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
pub async fn get_platform_info() -> Result<PlatformInfo, String> {
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
pub async fn start_recording(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::pipeline::start_recording(&app_handle, &state);
    Ok(())
}

#[tauri::command]
pub async fn stop_recording(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    crate::pipeline::stop_and_transcribe(&app_handle, &state).await;
    Ok("ok".into())
}

#[tauri::command]
pub async fn cancel_recording(
    app_handle: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    crate::pipeline::cancel_recording(&app_handle, &state);
    Ok(())
}

#[tauri::command]
pub async fn get_audio_devices() -> Result<Vec<AudioDevice>, String> {
    AudioCapture::enumerate_devices().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_audio_device(
    device_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Validate device exists before accepting it
    let devices = AudioCapture::enumerate_devices().map_err(|e| e.to_string())?;
    if !devices.iter().any(|d| d.id == device_id) {
        return Err(format!("Audio device '{}' not found", device_id));
    }

    let mut audio = match state.audio.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };

    if audio.is_recording() {
        let _ = audio.stop();
    }

    let config = crate::audio::types::AudioConfig {
        device_id: Some(device_id),
        ..Default::default()
    };
    *audio = AudioCapture::new(config);
    Ok(())
}
