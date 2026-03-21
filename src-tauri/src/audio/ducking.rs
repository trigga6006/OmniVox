//! System volume ducking — lowers other audio while recording.
//!
//! Uses the Windows Core Audio `IAudioEndpointVolume` COM interface to
//! snapshot the current master volume, reduce it during recording, and
//! restore it when recording stops.

#[cfg(target_os = "windows")]
mod win {
    use std::sync::Mutex;

    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    /// How much to reduce volume (0.30 = duck to 30% of current level).
    const DUCK_FACTOR: f32 = 0.30;

    static ORIGINAL_VOLUME: Mutex<Option<f32>> = Mutex::new(None);

    fn get_endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
        unsafe {
            // CoInitializeEx is safe to call multiple times — it just bumps the ref count.
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
        }
    }

    /// Lower the system volume. Call when recording starts.
    pub fn duck() {
        let vol = match get_endpoint_volume() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Volume duck: failed to get endpoint: {e}");
                return;
            }
        };

        unsafe {
            let current = match vol.GetMasterVolumeLevelScalar() {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Volume duck: failed to get level: {e}");
                    return;
                }
            };

            // Save the original volume for restoration
            if let Ok(mut orig) = ORIGINAL_VOLUME.lock() {
                *orig = Some(current);
            }

            let ducked = (current * DUCK_FACTOR).max(0.0);
            if let Err(e) = vol.SetMasterVolumeLevelScalar(ducked, std::ptr::null()) {
                eprintln!("Volume duck: failed to set level: {e}");
            }
        }
    }

    /// Restore the system volume to its pre-duck level. Call when recording stops.
    pub fn unduck() {
        let original = match ORIGINAL_VOLUME.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };

        let Some(level) = original else { return };

        let vol = match get_endpoint_volume() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("Volume unduck: failed to get endpoint: {e}");
                return;
            }
        };

        unsafe {
            if let Err(e) = vol.SetMasterVolumeLevelScalar(level, std::ptr::null()) {
                eprintln!("Volume unduck: failed to restore level: {e}");
            }
        }
    }
}

/// Lower system volume while recording.
pub fn duck() {
    #[cfg(target_os = "windows")]
    win::duck();
}

/// Restore system volume after recording.
pub fn unduck() {
    #[cfg(target_os = "windows")]
    win::unduck();
}
