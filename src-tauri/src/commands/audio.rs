use tauri::State;

use crate::audio::capture::AudioCapture;
use crate::audio::types::AudioDevice;
use crate::state::AppState;

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
