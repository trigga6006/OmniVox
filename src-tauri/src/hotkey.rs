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
//! The hotkey keys are stored in a packed `AtomicU32` so the hook callback
//! can read them lock-free.  Call [`update_hotkey_keys`] to change the combo
//! at runtime (e.g. after the user remaps from Settings).

use serde::{Deserialize, Serialize};

/// Persisted hotkey configuration — keys + display labels.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HotkeyConfig {
    /// Windows VK codes for the 1–2 keys in the combo.
    pub keys: Vec<u16>,
    /// Human-readable display names, parallel to `keys`.
    pub labels: Vec<String>,
}

impl Default for HotkeyConfig {
    fn default() -> Self {
        Self {
            keys: vec![0xA2, 0xA4], // VK_LCONTROL, VK_LMENU
            labels: vec!["LCtrl".into(), "LAlt".into()],
        }
    }
}

// ── Platform-gated implementation ────────────────────────────────

#[cfg(target_os = "windows")]
mod win {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
    use std::sync::OnceLock;
    use std::thread;
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT,
        MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    /// Time window for a double-press to count as "toggle" mode.
    const DOUBLE_TAP_MS: u64 = 400;

    // ── Dynamic hotkey storage ───────────────────────────────────
    /// Packed hotkey: low u16 = key1 VK code, high u16 = key2 VK code (0 if single-key).
    static HOTKEY_PACKED: AtomicU32 = AtomicU32::new(0);
    /// Bitmask of which configured keys are currently held.
    /// bit 0 = key1 down, bit 1 = key2 down.
    static KEYS_DOWN: AtomicU8 = AtomicU8::new(0);
    /// When true the hook passes all keys through without processing.
    static HOTKEY_SUSPENDED: AtomicBool = AtomicBool::new(false);

    // ── Recording state machine ──────────────────────────────────
    static RECORDING: AtomicBool = AtomicBool::new(false);
    static TOGGLE_LOCKED: AtomicBool = AtomicBool::new(false);
    static LAST_ACTIVATE_MS: AtomicU64 = AtomicU64::new(0);
    static EPOCH: OnceLock<Instant> = OnceLock::new();

    static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

    fn now_ms() -> u64 {
        let epoch = EPOCH.get_or_init(Instant::now);
        epoch.elapsed().as_millis() as u64
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

    /// Update the hotkey keys at runtime. Safe to call before the hook thread
    /// starts — the hook will read the correct value on its first key event.
    pub fn update_hotkey_keys(key1: u16, key2: u16) {
        let packed = (key2 as u32) << 16 | (key1 as u32);
        HOTKEY_PACKED.store(packed, Ordering::SeqCst);
        // Reset state so stale key-down bits don't stick.
        KEYS_DOWN.store(0, Ordering::SeqCst);
        RECORDING.store(false, Ordering::SeqCst);
        TOGGLE_LOCKED.store(false, Ordering::SeqCst);
    }

    /// Suspend or resume the hotkey hook. While suspended the hook passes all
    /// key events through to the OS without processing.
    pub fn set_suspended(suspended: bool) {
        HOTKEY_SUSPENDED.store(suspended, Ordering::SeqCst);
        if suspended {
            // Reset state in case keys were held when we suspended.
            KEYS_DOWN.store(0, Ordering::SeqCst);
        }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 && !HOTKEY_SUSPENDED.load(Ordering::SeqCst) {
            let kb = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
            let vk = kb.vkCode as u16;
            let is_down =
                wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
            let is_up =
                wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

            // Read the configured keys.
            let packed = HOTKEY_PACKED.load(Ordering::SeqCst);
            if packed == 0 {
                // No hotkey configured yet.
                return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
            }
            let key1 = (packed & 0xFFFF) as u16;
            let key2 = ((packed >> 16) & 0xFFFF) as u16;
            let is_two_key = key2 != 0;

            // Which slot does this VK match?
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

                    return 1; // swallow
                } else if locked {
                    // Toggle-off
                    RECORDING.store(false, Ordering::SeqCst);
                    TOGGLE_LOCKED.store(false, Ordering::SeqCst);
                    LAST_ACTIVATE_MS.store(0, Ordering::SeqCst);
                    fire_stop();

                    return 1; // swallow
                }
            }

            // ── Key released while hold-recording (non-locked) ──
            if recording && !locked && is_up && (matches_key1 || matches_key2) {
                RECORDING.store(false, Ordering::SeqCst);
                fire_stop();
            }
        }

        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    /// Spawn the hook thread with a Windows message pump.
    pub fn start(app_handle: tauri::AppHandle) {
        let _ = APP_HANDLE.set(app_handle);
        let _ = EPOCH.get_or_init(Instant::now);

        // If no hotkey was loaded from settings yet, use the default (Ctrl+LAlt).
        if HOTKEY_PACKED.load(Ordering::SeqCst) == 0 {
            update_hotkey_keys(0xA2, 0xA4); // VK_LCONTROL, VK_LMENU
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

// ── Public API (cross-platform stubs) ────────────────────────────

/// Install the global hotkey hook.
pub fn install(app_handle: tauri::AppHandle) {
    #[cfg(target_os = "windows")]
    win::start(app_handle);

    #[cfg(not(target_os = "windows"))]
    let _ = app_handle;
}

/// Update the hotkey keys at runtime.
pub fn update_hotkey_keys(key1: u16, key2: u16) {
    #[cfg(target_os = "windows")]
    win::update_hotkey_keys(key1, key2);

    #[cfg(not(target_os = "windows"))]
    { let _ = (key1, key2); }
}

/// Suspend or resume the hook.
pub fn set_suspended(suspended: bool) {
    #[cfg(target_os = "windows")]
    win::set_suspended(suspended);

    #[cfg(not(target_os = "windows"))]
    let _ = suspended;
}
