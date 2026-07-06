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
//! There are **two** independent hotkeys sharing one hook: the dictation hotkey
//! (default LCtrl+LAlt) and the Command-Mode hotkey (default Right Ctrl, only
//! active when Command Mode is enabled).  Each is matched independently and
//! routes to its own pipeline entry point.
//!
//! On Windows the hotkey uses a low-level keyboard hook (`WH_KEYBOARD_LL`).
//! On macOS and Linux the hotkey uses `rdev` for global key event listening.
//!
//! Each hotkey's keys are stored in a packed `AtomicU32` so the hook callback
//! can read them lock-free.  Call [`update_hotkey_keys`] /
//! [`update_command_hotkey_keys`] to change a combo at runtime.

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

/// Default Command-Mode hotkey: Right Ctrl (VK_RCONTROL), a single key the user
/// almost never *initiates* shortcuts with, leaving CapsLock and the left
/// modifiers untouched.
pub const COMMAND_HOTKEY_VK: u16 = 0xA3;

// ── Shared state machine logic ───────────────────────────────────
//
// Both the Windows and rdev backends use the same atomic state machine.
// This avoids duplicating the hold/toggle logic, and now drives two
// independent hotkeys (dictation + command).

mod state_machine {
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;

    use tauri::Manager;

    /// Time window for a double-press to count as "toggle" mode.
    const DOUBLE_TAP_MS: u64 = 400;

    /// When true the hook passes all keys through without processing.
    pub static HOTKEY_SUSPENDED: AtomicBool = AtomicBool::new(false);

    /// Which capture path a hotkey drives.
    #[derive(Clone, Copy)]
    enum Action {
        Dictation,
        Command,
    }

    /// Per-hotkey atomic state.
    struct Hk {
        /// Packed combo: low u16 = key1 code, high u16 = key2 code (0 if single-key).
        packed: AtomicU32,
        /// Bitmask of which configured keys are currently held (bit0=key1, bit1=key2).
        keys_down: AtomicU8,
        /// Bitmask of combo keys whose activating key-DOWN we swallowed, so we
        /// can swallow the matching key-UP too.  Without this, the release of
        /// the hotkey modifier leaks to the foreground app — a lone `Alt`-up
        /// pops the Windows menu/ribbon KeyTips overlay, and any leaked
        /// modifier release can disturb the focused control.  Tracking it
        /// per-key keeps the swallow balanced: keys whose down we passed
        /// through (e.g. the first modifier, which may be the start of a real
        /// Ctrl+C / Alt+Tab) still get their up passed through.
        swallowed_down: AtomicU8,
        recording: AtomicBool,
        toggle_locked: AtomicBool,
        last_activate_ms: AtomicU64,
    }

    impl Hk {
        const fn new() -> Self {
            Self {
                packed: AtomicU32::new(0),
                keys_down: AtomicU8::new(0),
                swallowed_down: AtomicU8::new(0),
                recording: AtomicBool::new(false),
                toggle_locked: AtomicBool::new(false),
                last_activate_ms: AtomicU64::new(0),
            }
        }
    }

    static DICTATION: Hk = Hk::new();
    static COMMAND: Hk = Hk::new();

    static EPOCH: OnceLock<Instant> = OnceLock::new();
    pub static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();

    /// Epoch-ms timestamp when a command confirm pill was armed (0 = none).
    /// Written from the pipeline via [`set_confirm_pending`]; read lock-free
    /// on the hook thread so Enter/Esc can drive the confirm without focus.
    static CONFIRM_PENDING_SINCE_MS: AtomicU64 = AtomicU64::new(0);

    const VK_RETURN: u16 = 0x0D;
    const VK_ESCAPE: u16 = 0x1B;

    /// Arm (or clear) the hook's Enter/Esc confirm path.  Called wherever a
    /// `PendingCommand` is parked or consumed.
    pub fn set_confirm_pending(pending: bool) {
        // max(1): a pill armed in the very first millisecond after launch
        // must not encode as the "none" sentinel.
        let v = if pending { now_ms().max(1) } else { 0 };
        CONFIRM_PENDING_SINCE_MS.store(v, Ordering::Release);
    }

    fn now_ms() -> u64 {
        let epoch = EPOCH.get_or_init(Instant::now);
        epoch.elapsed().as_millis() as u64
    }

    pub fn init_epoch() {
        let _ = EPOCH.get_or_init(Instant::now);
    }

    fn action_mode(action: Action) -> crate::state::CaptureMode {
        match action {
            Action::Dictation => crate::state::CaptureMode::Dictation,
            Action::Command => crate::state::CaptureMode::Command,
        }
    }

    fn fire_start(action: Action) {
        let Some(handle) = APP_HANDLE.get() else {
            return;
        };
        let h = handle.clone();
        // Claim ownership SYNCHRONOUSLY on this serialized hook thread, before
        // spawning the worker — so the matching release (fire_stop, later on the
        // same thread) can never be processed before the claim happens.
        let claimed = crate::pipeline::try_claim_capture(&h, action_mode(action));
        crate::llm::diaglog::log(&format!(
            "hotkey: fire_start action={:?} claimed={claimed}",
            action_mode(action)
        ));
        if !claimed {
            return;
        }
        tauri::async_runtime::spawn(async move {
            match action {
                Action::Dictation => {
                    let st = h.state::<crate::state::AppState>();
                    crate::pipeline::start_recording_inner(&h, &st);
                }
                Action::Command => {
                    crate::pipeline::start_command_inner(&h).await;
                }
            }
        });
    }

    fn fire_stop(action: Action) {
        let Some(handle) = APP_HANDLE.get() else {
            return;
        };
        let h = handle.clone();
        // Decide the stop SYNCHRONOUSLY (records a deferred stop if the capture
        // is still starting); only spawn the worker for an immediate stop.
        if !crate::pipeline::should_stop_now(&h, action_mode(action)) {
            return;
        }
        tauri::async_runtime::spawn(async move {
            let st = h.state::<crate::state::AppState>();
            match action {
                Action::Dictation => crate::pipeline::stop_and_transcribe(&h, &st).await,
                Action::Command => crate::pipeline::stop_and_run_command(&h, &st).await,
            }
        });
    }

    fn reset(hk: &Hk) {
        hk.keys_down.store(0, Ordering::Release);
        hk.swallowed_down.store(0, Ordering::Release);
        hk.recording.store(false, Ordering::Release);
        hk.toggle_locked.store(false, Ordering::Release);
    }

    /// Update the dictation hotkey keys at runtime.
    ///
    /// Release ordering on `packed` synchronizes with the Acquire load in
    /// `process_one` — so the hook thread, after observing the new packed value,
    /// also sees the reset latches that accompany the change.
    pub fn update_hotkey_keys(key1: u16, key2: u16) {
        let packed = (key2 as u32) << 16 | (key1 as u32);
        reset(&DICTATION);
        DICTATION.packed.store(packed, Ordering::Release);
    }

    /// Update (or disable, with key1=0) the Command-Mode hotkey at runtime.
    pub fn update_command_hotkey_keys(key1: u16, key2: u16) {
        let packed = (key2 as u32) << 16 | (key1 as u32);
        reset(&COMMAND);
        COMMAND.packed.store(packed, Ordering::Release);
    }

    pub fn dictation_packed() -> u32 {
        DICTATION.packed.load(Ordering::Acquire)
    }

    /// Suspend or resume the hook, clearing latch state so a suspended-while-
    /// active hotkey doesn't wake up stuck "already recording".
    pub fn set_suspended(suspended: bool) {
        HOTKEY_SUSPENDED.store(suspended, Ordering::Release);
        if suspended {
            reset(&DICTATION);
            reset(&COMMAND);
        }
    }

    /// Process a key event against both hotkeys. Returns true if the event
    /// should be swallowed.
    pub fn process_key_event(vk: u16, is_down: bool, is_up: bool) -> bool {
        if HOTKEY_SUSPENDED.load(Ordering::Acquire) {
            // Log only the keys that belong to a configured combo, so we can
            // tell a "suspended swallowed my hotkey" case from ordinary typing.
            let d = DICTATION.packed.load(Ordering::Acquire);
            if d != 0 && (vk == (d & 0xFFFF) as u16 || vk == ((d >> 16) & 0xFFFF) as u16) {
                crate::llm::diaglog::log(&format!(
                    "hotkey: SUSPENDED — passing through combo key vk={vk:#06x} down={is_down}"
                ));
            }
            return false;
        }

        // ── Phase A control keys ─────────────────────────────
        // Esc while a command capture is live cancels it (the overlay window
        // is focused(false), so DOM key events can never arrive — the hook is
        // the only path).  Scoped to COMMAND on purpose: Esc during dictation
        // is often meant for the app the user is dictating into.
        if vk == VK_ESCAPE && is_down && COMMAND.recording.load(Ordering::Relaxed) {
            reset(&COMMAND);
            if let Some(handle) = APP_HANDLE.get() {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let st = h.state::<crate::state::AppState>();
                    crate::pipeline::cancel_recording(&h, &st);
                });
            }
            return true; // swallow — this Esc was aimed at OmniVox
        }

        // Enter/Esc while a confirm pill is pending drives it from the
        // keyboard.  Guard rails against confirming something the user never
        // saw: a 250ms arming debounce (an Enter finishing their typing must
        // not confirm) and a 15s freshness window (a forgotten pill must not
        // swallow keys minutes later — the mouse buttons keep working).
        let since = CONFIRM_PENDING_SINCE_MS.load(Ordering::Acquire);
        if since != 0 && is_down && (vk == VK_RETURN || vk == VK_ESCAPE) {
            let age = now_ms().saturating_sub(since);
            if (250..15_000).contains(&age) {
                CONFIRM_PENDING_SINCE_MS.store(0, Ordering::Release);
                let confirm = vk == VK_RETURN;
                if let Some(handle) = APP_HANDLE.get() {
                    let h = handle.clone();
                    tauri::async_runtime::spawn(async move {
                        let st = h.state::<crate::state::AppState>();
                        if confirm {
                            // Keyboard confirm sends the message as heard —
                            // editing goes through the pill's textarea (mouse).
                            crate::pipeline::confirm_pending_command(&h, &st, None).await;
                        } else {
                            crate::pipeline::cancel_pending_command(&h, &st);
                        }
                    });
                }
                return true; // swallow
            }
        }

        // Check both; swallow if either consumed the event. (The default combos
        // share no keys, so at most one fires per event.)
        let d = process_one(&DICTATION, Action::Dictation, vk, is_down, is_up);
        let c = process_one(&COMMAND, Action::Command, vk, is_down, is_up);
        d || c
    }

    fn process_one(hk: &Hk, action: Action, vk: u16, is_down: bool, is_up: bool) -> bool {
        let packed = hk.packed.load(Ordering::Acquire);
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
                hk.keys_down.fetch_or(bit, Ordering::Relaxed);
            } else if is_up {
                hk.keys_down.fetch_and(!bit, Ordering::Relaxed);
            }
        }

        let keys_down = hk.keys_down.load(Ordering::Relaxed);
        let all_down = if is_two_key {
            keys_down == 0x03
        } else {
            keys_down == 0x01
        };

        let recording = hk.recording.load(Ordering::Relaxed);
        let locked = hk.toggle_locked.load(Ordering::Relaxed);

        // The combo-key bit for the CURRENT event (used to balance the
        // swallowed-down / swallowed-up bookkeeping below).
        let bit: u8 = if matches_key1 { 0x01 } else { 0x02 };

        // ── Both/all keys just pressed ───────────────────────
        if all_down && is_down {
            if !recording {
                let now = now_ms();
                let last = hk.last_activate_ms.swap(now, Ordering::Relaxed);
                let is_double_tap = (now - last) <= DOUBLE_TAP_MS;

                hk.recording.store(true, Ordering::Relaxed);
                hk.toggle_locked.store(is_double_tap, Ordering::Relaxed);
                // Remember we swallowed this key's press so we also swallow its
                // release — otherwise the lone modifier-up leaks to the app.
                hk.swallowed_down.fetch_or(bit, Ordering::Relaxed);
                fire_start(action);

                return true; // swallow
            } else if locked {
                // Toggle-off
                hk.recording.store(false, Ordering::Relaxed);
                hk.toggle_locked.store(false, Ordering::Relaxed);
                hk.last_activate_ms.store(0, Ordering::Relaxed);
                hk.swallowed_down.fetch_or(bit, Ordering::Relaxed);
                fire_stop(action);

                return true; // swallow
            }
        }

        // ── Swallow auto-repeat key-downs while held ──────────
        //    Holding a hotkey makes Windows stream WM_KEYDOWN repeats.  If we
        //    swallowed a key's activating press we must swallow its repeats too —
        //    otherwise a held single-key hotkey (Right Ctrl for Command Mode)
        //    leaks a flood of Ctrl-DOWN repeats to the foreground app while its
        //    key-UP is swallowed below, leaving the modifier stuck "down" with no
        //    release (only a reboot clears it).  Balanced per-key: a key whose
        //    down we passed through (the first modifier of a combo, e.g. LCtrl in
        //    LCtrl+LAlt) is NOT in `swallowed_down`, so its repeats still pass —
        //    keeping real Ctrl+C / Alt+Tab intact.
        if is_down && (matches_key1 || matches_key2) {
            if hk.swallowed_down.load(Ordering::Relaxed) & bit != 0 {
                return true; // swallow the repeat
            }
        }

        // ── Key released while hold-recording (non-locked) ──
        if recording && !locked && is_up && (matches_key1 || matches_key2) {
            hk.recording.store(false, Ordering::Relaxed);
            fire_stop(action);
        }

        // ── Swallow the release of any combo key whose activating press we
        //    swallowed, so the foreground app never sees a lone modifier
        //    release.  A leaked `Alt`-up pops the Windows menu/ribbon KeyTips
        //    overlay (the "letters across the top" the user hits in Notes);
        //    other leaked modifier releases can disturb the focused control.
        //    Balanced per-key: a modifier whose down we passed through (the
        //    first key of the combo, or a real Ctrl+C / Alt+Tab) is NOT in
        //    `swallowed_down`, so its up still passes — keeping those intact.
        if is_up && (matches_key1 || matches_key2) {
            if hk.swallowed_down.fetch_and(!bit, Ordering::Relaxed) & bit != 0 {
                return true; // swallow the balancing release
            }
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
        CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG,
        WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    use super::state_machine;

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let kb = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
            let vk = kb.vkCode as u16;
            let is_down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
            let is_up = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

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

        // If no dictation hotkey was loaded from settings yet, use the default
        // (Ctrl+LAlt).  The command hotkey is governed by Command Mode being
        // enabled (set via apply_persisted_settings), so it is NOT defaulted here.
        if state_machine::dictation_packed() == 0 {
            state_machine::update_hotkey_keys(0xA2, 0xA4); // VK_LCONTROL, VK_LMENU
        }

        thread::Builder::new()
            .name("omnivox-hotkey".into())
            .spawn(|| unsafe {
                let hook =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0);
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
            ControlLeft => 0xA2,  // VK_LCONTROL
            ControlRight => 0xA3, // VK_RCONTROL
            Alt => 0xA4,          // VK_LMENU
            AltGr => 0xA5,        // VK_RMENU
            ShiftLeft => 0xA0,    // VK_LSHIFT
            ShiftRight => 0xA1,   // VK_RSHIFT
            MetaLeft => 0x5B,     // VK_LWIN (Cmd on macOS)
            MetaRight => 0x5C,    // VK_RWIN

            // Function keys
            F1 => 0x70,
            F2 => 0x71,
            F3 => 0x72,
            F4 => 0x73,
            F5 => 0x74,
            F6 => 0x75,
            F7 => 0x76,
            F8 => 0x77,
            F9 => 0x78,
            F10 => 0x79,
            F11 => 0x7A,
            F12 => 0x7B,

            // Common keys
            Space => 0x20,
            Return => 0x0D,
            Escape => 0x1B,
            Tab => 0x09,
            Backspace => 0x08,
            CapsLock => 0x14,

            // Letters (A–Z)
            KeyA => 0x41,
            KeyB => 0x42,
            KeyC => 0x43,
            KeyD => 0x44,
            KeyE => 0x45,
            KeyF => 0x46,
            KeyG => 0x47,
            KeyH => 0x48,
            KeyI => 0x49,
            KeyJ => 0x4A,
            KeyK => 0x4B,
            KeyL => 0x4C,
            KeyM => 0x4D,
            KeyN => 0x4E,
            KeyO => 0x4F,
            KeyP => 0x50,
            KeyQ => 0x51,
            KeyR => 0x52,
            KeyS => 0x53,
            KeyT => 0x54,
            KeyU => 0x55,
            KeyV => 0x56,
            KeyW => 0x57,
            KeyX => 0x58,
            KeyY => 0x59,
            KeyZ => 0x5A,

            // Number row
            Num0 => 0x30,
            Num1 => 0x31,
            Num2 => 0x32,
            Num3 => 0x33,
            Num4 => 0x34,
            Num5 => 0x35,
            Num6 => 0x36,
            Num7 => 0x37,
            Num8 => 0x38,
            Num9 => 0x39,

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

        // If no dictation hotkey was loaded from settings yet, use the default.
        if state_machine::dictation_packed() == 0 {
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

/// Update the dictation hotkey keys at runtime.
pub fn update_hotkey_keys(key1: u16, key2: u16) {
    state_machine::update_hotkey_keys(key1, key2);
}

/// Update the Command-Mode hotkey keys at runtime. Pass `key1 = 0` to disable.
pub fn update_command_hotkey_keys(key1: u16, key2: u16) {
    state_machine::update_command_hotkey_keys(key1, key2);
}

/// Enable or disable the Command-Mode hotkey (Right Ctrl) based on whether
/// Command Mode is on.  Disabling sets the combo to 0 so Right Ctrl passes
/// through untouched.
pub fn set_command_mode_enabled(enabled: bool) {
    if enabled {
        update_command_hotkey_keys(COMMAND_HOTKEY_VK, 0);
    } else {
        update_command_hotkey_keys(0, 0);
    }
}

/// Suspend or resume the hook.
pub fn set_suspended(suspended: bool) {
    state_machine::set_suspended(suspended);
}

/// Arm (or clear) the hook's Enter/Esc handling for a pending command
/// confirm.  Call with `true` when a `PendingCommand` is parked and the
/// confirm pill shown; with `false` whenever it is consumed or cleared.
pub fn set_confirm_pending(pending: bool) {
    state_machine::set_confirm_pending(pending);
}

/// Feed a key event from the frontend (WebView) into the hotkey state machine.
///
/// When one of OmniVox's own windows has focus, the global OS keyboard hook
/// receives no key events — the WebView2 consumes them first — so dictation and
/// command hotkeys would never fire while the user is inside the app.  The
/// frontend listens for the relevant modifier keys and forwards them here so the
/// same hold/toggle state machine drives both paths.  The two are mutually
/// exclusive by focus (OS hook when another app is forward, this when ours is),
/// and the state machine's latches make an occasional overlapping event safe.
pub fn feed_key_event(vk: u16, is_down: bool) {
    let _ = state_machine::process_key_event(vk, is_down, !is_down);
}
