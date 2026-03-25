//! Dynamic keyboard hook for customizable hotkeys.
//!
//! Two interaction modes:
//!
//! **Hold mode** — Press and hold the hotkey combo to record.  Release either
//! key to stop recording and begin transcription.
//!
//! **Toggle mode** — Double-press the combo (within 400 ms) to lock recording
//! on.  Press the combo again to stop and transcribe.
//!
//! On Windows the hotkey uses a low-level keyboard hook (`WH_KEYBOARD_LL`).
//! On macOS and Linux the hotkey uses `rdev` for global key event listening.
//!
//! The hotkey keys are stored in a packed `AtomicU32` so the hook callback
//! can read them lock-free.  Call [`update_hotkey_keys`] to change the combo
//! at runtime (e.g. after the user remaps from Settings).

use serde::{Deserialize, Serialize};

/// Persisted hotkey configuration — keys + display labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Platform key codes for the 1–2 keys in the combo.
    /// On Windows these are VK codes; on macOS they map to rdev key identifiers.
    pub keys: Vec<u16>,
    /// Human-readable display names, parallel to `keys`.
    pub labels: Vec<String>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        // Default: LCtrl + LAlt (VK_LCONTROL + VK_LMENU).
        // Same VK codes on all platforms — mapped via vk_to_rdev_key() on macOS/Linux.
        Self {
            keys: vec![0xA2, 0xA4],
            labels: vec!["LCtrl".into(), "LAlt".into()],
        }
    }
}

// ── Shared state machine logic ───────────────────────────────────
//
// Both the Windows and rdev backends use the same atomic state machine.
// This avoids duplicating the hold/toggle logic.

mod state_machine {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;

    /// Time window for a double-press to count as "toggle" mode.
    const DOUBLE_TAP_MS: u64 = 400;

    /// Packed hotkey: low u16 = key1 code, high u16 = key2 code (0 if single-key).
    pub static HOTKEY_PACKED: AtomicU32 = AtomicU32::new(0);
    /// Bitmask of which configured keys are currently held.
    /// bit 0 = key1 down, bit 1 = key2 down.
    pub static KEYS_DOWN: AtomicU8 = AtomicU8::new(0);
    /// When true the hook passes all keys through without processing.
    pub static HOTKEY_SUSPENDED: AtomicBool = AtomicBool::new(false);

    // ── Recording state machine ──────────────────────────────────
    static RECORDING: AtomicBool = AtomicBool::new(false);
    static TOGGLE_LOCKED: AtomicBool = AtomicBool::new(false);
    static LAST_ACTIVATE_MS: AtomicU64 = AtomicU64::new(0);
    static EPOCH: OnceLock<Instant> = OnceLock::new();

    pub static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

    fn now_ms() -> u64 {
        let epoch = EPOCH.get_or_init(Instant::now);
        epoch.elapsed().as_millis() as u64
    }

    pub fn init_epoch() {
        let _ = EPOCH.get_or_init(Instant::now);
    }

    fn fire_start() {
        if let Some(handle) = APP_HANDLE.get() {
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                crate::pipeline::start_if_idle(&h).await;
            });
        }
    }

    fn fire_stop() {
        if let Some(handle) = APP_HANDLE.get() {
            let h = handle.clone();
            tauri::async_runtime::spawn(async move {
                crate::pipeline::stop_if_recording(&h).await;
            });
        }
    }

    /// Update the hotkey keys at runtime.
    pub fn update_hotkey_keys(key1: u16, key2: u16) {
        let packed = (key2 as u32) << 16 | (key1 as u32);
        HOTKEY_PACKED.store(packed, Ordering::SeqCst);
        KEYS_DOWN.store(0, Ordering::SeqCst);
        RECORDING.store(false, Ordering::SeqCst);
        TOGGLE_LOCKED.store(false, Ordering::SeqCst);
    }

    /// Suspend or resume the hotkey hook.
    pub fn set_suspended(suspended: bool) {
        HOTKEY_SUSPENDED.store(suspended, Ordering::SeqCst);
        if suspended {
            KEYS_DOWN.store(0, Ordering::SeqCst);
        }
    }

    /// Process a key event. Returns true if the event should be swallowed.
    ///
    /// `vk` is the virtual key code, `is_down` / `is_up` indicate the event type.
    pub fn process_key_event(vk: u16, is_down: bool, is_up: bool) -> bool {
        if HOTKEY_SUSPENDED.load(Ordering::SeqCst) {
            return false;
        }

        let packed = HOTKEY_PACKED.load(Ordering::SeqCst);
        if packed == 0 {
            return false;
        }

        let key1 = (packed & 0xFFFF) as u16;
        let key2 = ((packed >> 16) & 0xFFFF) as u16;
        let is_two_key = key2 != 0;

        let matches_key1 = vk == key1;
        let matches_key2 = is_two_key && vk == key2;

        if matches_key1 || matches_key2 {
            let bit: u8 = if matches_key1 { 0x01 } else { 0x02 };

            if is_down {
                KEYS_DOWN.fetch_or(bit, Ordering::SeqCst);
            } else if is_up {
                KEYS_DOWN.fetch_and(!bit, Ordering::SeqCst);
            }
        }

        let keys_down = KEYS_DOWN.load(Ordering::SeqCst);
        let all_down = if is_two_key {
            keys_down == 0x03
        } else {
            keys_down == 0x01
        };

        let recording = RECORDING.load(Ordering::SeqCst);
        let locked = TOGGLE_LOCKED.load(Ordering::SeqCst);

        // ── Both/all keys just pressed ───────────────────────
        if all_down && is_down {
            if !recording {
                let now = now_ms();
                let last = LAST_ACTIVATE_MS.swap(now, Ordering::SeqCst);
                let is_double_tap = (now - last) <= DOUBLE_TAP_MS;

                RECORDING.store(true, Ordering::SeqCst);
                TOGGLE_LOCKED.store(is_double_tap, Ordering::SeqCst);
                fire_start();

                return true; // swallow
            } else if locked {
                // Toggle-off
                RECORDING.store(false, Ordering::SeqCst);
                TOGGLE_LOCKED.store(false, Ordering::SeqCst);
                LAST_ACTIVATE_MS.store(0, Ordering::SeqCst);
                fire_stop();

                return true; // swallow
            }
        }

        // ── Key released while hold-recording (non-locked) ──
        if recording && !locked && is_up && (matches_key1 || matches_key2) {
            RECORDING.store(false, Ordering::SeqCst);
            fire_stop();
        }

        false
    }
}

// ── Windows implementation ───────────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    use std::thread;

    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT,
        MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::state_machine;

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
            let vk = kb.vkCode as u16;
            let is_down =
                wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
            let is_up =
                wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

            if state_machine::process_key_event(vk, is_down, is_up) {
                return 1; // swallow
            }
        }

        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    /// Spawn the hook thread with a Windows message pump.
    pub fn start(app_handle: tauri::AppHandle) {
        let _ = state_machine::APP_HANDLE.set(app_handle);
        state_machine::init_epoch();

        // If no hotkey was loaded from settings yet, use the default (Ctrl+LAlt).
        if state_machine::HOTKEY_PACKED.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            state_machine::update_hotkey_keys(0xA2, 0xA4); // VK_LCONTROL, VK_LMENU
        }

        thread::Builder::new()
            .name("omnivox-hotkey".into())
            .spawn(|| unsafe {
                let hook = SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(hook_proc),
                    std::ptr::null_mut(),
                    0,
                );
                if hook.is_null() {
                    eprintln!("Failed to install keyboard hook");
                    return;
                }

                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    DispatchMessageW(&msg);
                }
            })
            .expect("Failed to spawn hotkey thread");
    }
}

// ── rdev-based implementation (macOS & Linux) ────────────────────

#[cfg(not(target_os = "windows"))]
mod rdev_impl {
    use std::thread;

    use super::state_machine;

    /// Convert an `rdev::Key` to the VK code used in our hotkey storage.
    /// We reuse Windows VK codes as our canonical key identifiers across
    /// platforms so persisted settings remain portable.
    fn rdev_key_to_vk(key: &rdev::Key) -> Option<u16> {
        use rdev::Key::*;
        Some(match key {
            // Modifier keys
            ControlLeft  => 0xA2, // VK_LCONTROL
            ControlRight => 0xA3, // VK_RCONTROL
            Alt          => 0xA4, // VK_LMENU
            AltGr        => 0xA5, // VK_RMENU
            ShiftLeft    => 0xA0, // VK_LSHIFT
            ShiftRight   => 0xA1, // VK_RSHIFT
            MetaLeft     => 0x5B, // VK_LWIN (Cmd on macOS)
            MetaRight    => 0x5C, // VK_RWIN

            // Function keys
            F1  => 0x70, F2  => 0x71, F3  => 0x72, F4  => 0x73,
            F5  => 0x74, F6  => 0x75, F7  => 0x76, F8  => 0x77,
            F9  => 0x78, F10 => 0x79, F11 => 0x7A, F12 => 0x7B,

            // Common keys
            Space     => 0x20,
            Return    => 0x0D,
            Escape    => 0x1B,
            Tab       => 0x09,
            Backspace => 0x08,
            CapsLock  => 0x14,

            // Letters (A–Z)
            KeyA => 0x41, KeyB => 0x42, KeyC => 0x43, KeyD => 0x44,
            KeyE => 0x45, KeyF => 0x46, KeyG => 0x47, KeyH => 0x48,
            KeyI => 0x49, KeyJ => 0x4A, KeyK => 0x4B, KeyL => 0x4C,
            KeyM => 0x4D, KeyN => 0x4E, KeyO => 0x4F, KeyP => 0x50,
            KeyQ => 0x51, KeyR => 0x52, KeyS => 0x53, KeyT => 0x54,
            KeyU => 0x55, KeyV => 0x56, KeyW => 0x57, KeyX => 0x58,
            KeyY => 0x59, KeyZ => 0x5A,

            // Number row
            Num0 => 0x30, Num1 => 0x31, Num2 => 0x32, Num3 => 0x33,
            Num4 => 0x34, Num5 => 0x35, Num6 => 0x36, Num7 => 0x37,
            Num8 => 0x38, Num9 => 0x39,

            _ => return None,
        })
    }

    fn handle_event(event: rdev::Event) {
        let (key, is_down, is_up) = match event.event_type {
            rdev::EventType::KeyPress(k) => (k, true, false),
            rdev::EventType::KeyRelease(k) => (k, false, true),
            _ => return,
        };

        if let Some(vk) = rdev_key_to_vk(&key) {
            state_machine::process_key_event(vk, is_down, is_up);
        }
    }

    pub fn start(app_handle: tauri::AppHandle) {
        let _ = state_machine::APP_HANDLE.set(app_handle);
        state_machine::init_epoch();

        // If no hotkey was loaded from settings yet, use the default (Ctrl+LAlt).
        if state_machine::HOTKEY_PACKED.load(std::sync::atomic::Ordering::SeqCst) == 0 {
            state_machine::update_hotkey_keys(0xA2, 0xA4); // LControl + LAlt
        }

        thread::Builder::new()
            .name("omnivox-hotkey".into())
            .spawn(|| {
                // rdev::listen blocks the thread and runs the callback for every key event.
                // On macOS this requires Accessibility permissions (System Preferences →
                // Privacy & Security → Accessibility → enable OmniVox).
                if let Err(e) = rdev::listen(handle_event) {
                    eprintln!("Failed to start global key listener: {:?}", e);
                    eprintln!("On macOS, grant Accessibility permission in System Settings → Privacy & Security");
                }
            })
            .expect("Failed to spawn hotkey thread");
    }
}

// ── Public API ───────────────────────────────────────────────────

/// Install the global hotkey hook.
pub fn install(app_handle: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    win::start(app_handle);

    #[cfg(not(target_os = "windows"))]
    rdev_impl::start(app_handle);
}

/// Update the hotkey keys at runtime.
pub fn update_hotkey_keys(key1: u16, key2: u16) {
    state_machine::update_hotkey_keys(key1, key2);
}

/// Suspend or resume the hook.
pub fn set_suspended(suspended: bool) {
    state_machine::set_suspended(suspended);
}
