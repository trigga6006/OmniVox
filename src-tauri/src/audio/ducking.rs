//! System volume ducking — lowers other audio while recording.
//!
//! Uses platform-specific audio APIs to snapshot the current master volume,
//! reduce it during recording, and restore it when recording stops.
//!
//! - **Windows**: `IAudioEndpointVolume` COM interface
//! - **macOS**: CoreAudio `AudioObjectSetPropertyData`
//! - **Linux**: Stubbed (planned: PulseAudio / PipeWire)

/// How much to reduce volume (0.30 = duck to 30% of current level).
#[cfg(any(target_os = "windows", target_os = "macos"))]
const DUCK_FACTOR: f32 = 0.30;

// ── Windows implementation ───────────────────────────────────────

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

    static ORIGINAL_VOLUME: Mutex<Option<f32>> = Mutex::new(None);

    fn get_endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;

            let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
            device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
        }
    }

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

            if let Ok(mut orig) = ORIGINAL_VOLUME.lock() {
                *orig = Some(current);
            }

            let ducked = (current * super::DUCK_FACTOR).max(0.0);
            if let Err(e) = vol.SetMasterVolumeLevelScalar(ducked, std::ptr::null()) {
                eprintln!("Volume duck: failed to set level: {e}");
            }
        }
    }

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

// ── macOS implementation ─────────────────────────────────────────

#[cfg(target_os = "macos")]
mod mac {
    use std::sync::Mutex;

    // CoreAudio constants and types
    const K_AUDIO_HARDWARE_SERVICE_DEVICE_PROPERTY_VIRTUAL_MAIN_VOLUME: u32 =
        u32::from_be_bytes(*b"vmvc");
    const K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT: u32 = u32::from_be_bytes(*b"outp");
    const K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN: u32 = 0;
    const K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE: u32 =
        u32::from_be_bytes(*b"dOut");
    const K_AUDIO_OBJECT_SYSTEM_OBJECT: u32 = 1;
    const K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL: u32 = u32::from_be_bytes(*b"glob");

    #[repr(C)]
    struct AudioObjectPropertyAddress {
        m_selector: u32,
        m_scope: u32,
        m_element: u32,
    }

    type AudioDeviceID = u32;

    extern "C" {
        fn AudioObjectGetPropertyData(
            in_object_id: u32,
            in_address: *const AudioObjectPropertyAddress,
            in_qualifier_data_size: u32,
            in_qualifier_data: *const std::ffi::c_void,
            io_data_size: *mut u32,
            out_data: *mut std::ffi::c_void,
        ) -> i32;

        fn AudioObjectSetPropertyData(
            in_object_id: u32,
            in_address: *const AudioObjectPropertyAddress,
            in_qualifier_data_size: u32,
            in_qualifier_data: *const std::ffi::c_void,
            in_data_size: u32,
            in_data: *const std::ffi::c_void,
        ) -> i32;
    }

    static ORIGINAL_VOLUME: Mutex<Option<f32>> = Mutex::new(None);

    fn get_default_output_device() -> Option<AudioDeviceID> {
        let address = AudioObjectPropertyAddress {
            m_selector: K_AUDIO_HARDWARE_PROPERTY_DEFAULT_OUTPUT_DEVICE,
            m_scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_GLOBAL,
            m_element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };

        let mut device_id: AudioDeviceID = 0;
        let mut size = std::mem::size_of::<AudioDeviceID>() as u32;

        let status = unsafe {
            AudioObjectGetPropertyData(
                K_AUDIO_OBJECT_SYSTEM_OBJECT,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut device_id as *mut _ as *mut std::ffi::c_void,
            )
        };

        if status == 0 && device_id != 0 {
            Some(device_id)
        } else {
            None
        }
    }

    fn get_volume(device_id: AudioDeviceID) -> Option<f32> {
        let address = AudioObjectPropertyAddress {
            m_selector: K_AUDIO_HARDWARE_SERVICE_DEVICE_PROPERTY_VIRTUAL_MAIN_VOLUME,
            m_scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            m_element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };

        let mut volume: f32 = 0.0;
        let mut size = std::mem::size_of::<f32>() as u32;

        let status = unsafe {
            AudioObjectGetPropertyData(
                device_id,
                &address,
                0,
                std::ptr::null(),
                &mut size,
                &mut volume as *mut _ as *mut std::ffi::c_void,
            )
        };

        if status == 0 {
            Some(volume)
        } else {
            None
        }
    }

    fn set_volume(device_id: AudioDeviceID, volume: f32) -> bool {
        let address = AudioObjectPropertyAddress {
            m_selector: K_AUDIO_HARDWARE_SERVICE_DEVICE_PROPERTY_VIRTUAL_MAIN_VOLUME,
            m_scope: K_AUDIO_OBJECT_PROPERTY_SCOPE_OUTPUT,
            m_element: K_AUDIO_OBJECT_PROPERTY_ELEMENT_MAIN,
        };

        let status = unsafe {
            AudioObjectSetPropertyData(
                device_id,
                &address,
                0,
                std::ptr::null(),
                std::mem::size_of::<f32>() as u32,
                &volume as *const _ as *const std::ffi::c_void,
            )
        };

        status == 0
    }

    pub fn duck() {
        let device_id = match get_default_output_device() {
            Some(id) => id,
            None => {
                eprintln!("Volume duck: no default output device");
                return;
            }
        };

        let current = match get_volume(device_id) {
            Some(v) => v,
            None => {
                eprintln!("Volume duck: failed to get current volume");
                return;
            }
        };

        if let Ok(mut orig) = ORIGINAL_VOLUME.lock() {
            *orig = Some(current);
        }

        let ducked = (current * super::DUCK_FACTOR).max(0.0);
        if !set_volume(device_id, ducked) {
            eprintln!("Volume duck: failed to set volume");
        }
    }

    pub fn unduck() {
        let original = match ORIGINAL_VOLUME.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };

        let Some(level) = original else { return };

        let device_id = match get_default_output_device() {
            Some(id) => id,
            None => {
                eprintln!("Volume unduck: no default output device");
                return;
            }
        };

        if !set_volume(device_id, level) {
            eprintln!("Volume unduck: failed to restore volume");
        }
    }
}

// ── Public API ───────────────────────────────────────────────────

/// Lower system volume while recording.
pub fn duck() {
    #[cfg(target_os = "windows")]
    win::duck();

    #[cfg(target_os = "macos")]
    mac::duck();
}

/// Restore system volume after recording.
pub fn unduck() {
    #[cfg(target_os = "windows")]
    win::unduck();

    #[cfg(target_os = "macos")]
    mac::unduck();
}
