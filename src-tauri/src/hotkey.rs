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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicU8, Ordering};
    use std::sync::OnceLock;
    use std::time::Instant;

    use tauri::Manager;

    /// The foreground window (as an isize handle) at the moment a confirm pill
    /// armed.  The Enter/Esc hijack fires ONLY while the live foreground still
    /// matches this — if the user tabbed to another app, their Enter is meant for
    /// that app, not our confirm (H4).  `0` = unknown / not on Windows.
    static CONFIRM_ARM_FG: AtomicIsize = AtomicIsize::new(0);

    /// The current foreground window as an isize handle (`0` when unavailable).
    #[cfg(target_os = "windows")]
    fn current_foreground_isize() -> isize {
        use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        (unsafe { GetForegroundWindow() }) as isize
    }

    #[cfg(not(target_os = "windows"))]
    fn current_foreground_isize() -> isize {
        0
    }

    /// Time window for a double-press to count as "toggle" mode.
    const DOUBLE_TAP_MS: u64 = 400;

    /// When true the hook passes all keys through without processing.
    pub static HOTKEY_SUSPENDED: AtomicBool = AtomicBool::new(false);

    /// Which capture path a hotkey drives.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
        pending_packed: AtomicU32,
        has_pending_packed: AtomicBool,
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
                pending_packed: AtomicU32::new(0),
                has_pending_packed: AtomicBool::new(false),
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

    /// The EXACT pending command id the pill was armed with (0 = none).  The
    /// keyboard Enter/Esc path echoes THIS id back to the pipeline so it can't
    /// confirm a newer command that replaced the parked one in the interim
    /// (B2-2b).
    static CONFIRM_PENDING_ID: AtomicU64 = AtomicU64::new(0);

    const VK_RETURN: u16 = 0x0D;
    const VK_ESCAPE: u16 = 0x1B;

    /// Arm (or clear) the hook's Enter/Esc confirm path.  Called wherever a
    /// `PendingCommand` is parked or consumed.  `Some(id)` arms for that exact
    /// pending command (and snapshots the current foreground so the hijack is
    /// bound to it — H4); `None` clears.
    pub fn set_confirm_pending(pending_id: Option<u64>) {
        match pending_id {
            Some(id) => {
                CONFIRM_PENDING_ID.store(id, Ordering::Release);
                CONFIRM_ARM_FG.store(current_foreground_isize(), Ordering::Release);
                // max(1): a pill armed in the very first millisecond after launch
                // must not encode as the "none" sentinel.
                CONFIRM_PENDING_SINCE_MS.store(now_ms().max(1), Ordering::Release);
            }
            None => {
                CONFIRM_PENDING_ID.store(0, Ordering::Release);
                CONFIRM_ARM_FG.store(0, Ordering::Release);
                CONFIRM_PENDING_SINCE_MS.store(0, Ordering::Release);
            }
        }
    }

    fn now_ms() -> u64 {
        let epoch = EPOCH.get_or_init(Instant::now);
        epoch.elapsed().as_millis() as u64
    }

    fn is_double_tap(now: u64, last: u64) -> bool {
        last != 0 && now.saturating_sub(last) <= DOUBLE_TAP_MS
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

    fn fire_start(action: Action) -> bool {
        let Some(handle) = APP_HANDLE.get() else {
            return false;
        };
        let h = handle.clone();
        // Claim ownership SYNCHRONOUSLY on this serialized hook thread, before
        // spawning the worker — so the matching release (fire_stop, later on the
        // same thread) can never be processed before the claim happens.
        let Some(generation) = crate::pipeline::try_claim_capture(&h, action_mode(action)) else {
            crate::llm::diaglog::log(&format!(
                "hotkey: fire_start action={:?} claim rejected",
                action_mode(action)
            ));
            return false;
        };
        crate::llm::diaglog::log(&format!(
            "hotkey: fire_start action={:?} generation={generation}",
            action_mode(action),
        ));
        tauri::async_runtime::spawn(async move {
            match action {
                Action::Dictation => {
                    // `start_recording_inner` is fully synchronous — a SQLite
                    // settings read, auto-switch queries, cross-process COM
                    // ducking, and opening the mic device. Run it on the blocking
                    // pool so it never stalls a tokio worker, matching
                    // `commands::audio::start_recording`.
                    let app = h.clone();
                    if let Err(e) = tauri::async_runtime::spawn_blocking(move || {
                        let st = app.state::<crate::state::AppState>();
                        crate::pipeline::start_recording_inner(&app, &st, generation);
                    })
                    .await
                    {
                        crate::llm::diaglog::log(&format!(
                            "hotkey: start_recording task failed: {e}"
                        ));
                    }
                }
                Action::Command => {
                    crate::pipeline::start_command_inner(&h, generation).await;
                }
            }
        });
        true
    }

    fn fire_stop(action: Action) {
        let Some(handle) = APP_HANDLE.get() else {
            return;
        };
        let h = handle.clone();
        // Decide the stop SYNCHRONOUSLY (records a deferred stop if the capture
        // is still starting); only spawn the worker for an immediate stop.
        let Some(generation) = crate::pipeline::should_stop_now(&h, action_mode(action)) else {
            return;
        };
        tauri::async_runtime::spawn(async move {
            let st = h.state::<crate::state::AppState>();
            match action {
                Action::Dictation => {
                    crate::pipeline::stop_and_transcribe_generation(&h, &st, generation).await
                }
                Action::Command => crate::pipeline::stop_and_run_command(&h, &st, generation).await,
            }
        });
    }

    /// Modifiers whose *bare* press+release Windows reads as the menu
    /// activation gesture: Alt (unsided + both sides) moves keyboard focus to
    /// the menu bar / KeyTips, Win pops Start.  Chromium/Electron follow the
    /// same rule, and the focus move is invisible to `GetForegroundWindow`.
    const MENU_MODIFIERS: [u16; 5] = [0x12, 0xA4, 0xA5, 0x5B, 0x5C];

    /// The combo key whose key-DOWN we passed through to the foreground app.
    /// Only the key that *completes* the chord is swallowed (`completing_bit`
    /// is its bit), so the other one reached the app.  `0` for a single-key
    /// combo, where the only key is the completing one.
    fn passed_through_key(key1: u16, key2: u16, completing_bit: u8) -> u16 {
        if completing_bit == 0x01 {
            key2
        } else {
            key1
        }
    }

    /// `Some(vk)` when the passed-through combo key is a menu-activating
    /// modifier — i.e. the app is mid-way through what looks to it like a bare
    /// Alt/Win tap and needs the masking keystroke below.
    fn passed_through_menu_modifier(key1: u16, key2: u16, completing_bit: u8) -> Option<u16> {
        let passed = passed_through_key(key1, key2, completing_bit);
        MENU_MODIFIERS.contains(&passed).then_some(passed)
    }

    /// What a chord activation / toggle-off needs reported, so the reporting
    /// (logging, masking) can happen off the hook thread.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ChordFired {
        phase: &'static str,
        action: Action,
        /// The key that completed the chord (this event's VK).
        completed_vk: u16,
        /// The combo key whose down passed through to the app (`0` if none).
        passed_vk: u16,
        /// `Some` when `passed_vk` is a menu modifier and must be masked.
        mask_vk: Option<u16>,
    }

    /// Signature stamped into `dwExtraInfo` of the keystroke we inject, so the
    /// hook can recognise it and pass it straight through.  ASCII "OMNI_MSK".
    #[cfg(target_os = "windows")]
    pub const MASK_EXTRA_INFO: usize = 0x4F4D_4E49_5F4D_534B;

    /// Emit one benign keystroke while the leaked modifier is still physically
    /// held, so its eventual release is no longer a bare Alt/Win gesture and
    /// menu focus never moves — the AutoHotkey `#MenuMaskKey` technique.
    #[cfg(target_os = "windows")]
    fn send_menu_mask_key() {
        use windows_sys::Win32::Foundation::GetLastError;
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
        };

        /// 0xE8 is unassigned (AutoHotkey's default mask key): bound in no app,
        /// and it matches no configurable combo key.
        const MASK_VK: u16 = 0xE8;

        let key = |dw_flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: MASK_VK,
                    wScan: 0,
                    dwFlags: dw_flags,
                    time: 0,
                    dwExtraInfo: MASK_EXTRA_INFO,
                },
            },
        };
        let inputs = [key(0), key(KEYEVENTF_KEYUP)];
        let sent = unsafe {
            SendInput(
                inputs.len() as u32,
                inputs.as_ptr(),
                std::mem::size_of::<INPUT>() as i32,
            )
        };
        if sent == 0 {
            crate::diag::log(&format!(
                "hotkey: menu mask SendInput sent 0 events error={}",
                unsafe { GetLastError() }
            ));
        }
    }

    fn reset(hk: &Hk) {
        hk.keys_down.store(0, Ordering::Release);
        hk.swallowed_down.store(0, Ordering::Release);
        hk.recording.store(false, Ordering::Release);
        hk.toggle_locked.store(false, Ordering::Release);
    }

    fn update_packed(hk: &Hk, packed: u32) {
        if hk.packed.load(Ordering::Acquire) == packed
            && !hk.has_pending_packed.load(Ordering::Acquire)
        {
            return;
        }
        if hk.recording.load(Ordering::Acquire) || hk.keys_down.load(Ordering::Acquire) != 0 {
            hk.pending_packed.store(packed, Ordering::Release);
            hk.has_pending_packed.store(true, Ordering::Release);
            return;
        }
        reset(hk);
        hk.packed.store(packed, Ordering::Release);
    }

    fn apply_pending_if_idle(hk: &Hk) {
        if hk.recording.load(Ordering::Acquire) || hk.keys_down.load(Ordering::Acquire) != 0 {
            return;
        }
        if hk
            .has_pending_packed
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let packed = hk.pending_packed.load(Ordering::Acquire);
            reset(hk);
            hk.packed.store(packed, Ordering::Release);
        }
    }

    /// Update the dictation hotkey keys at runtime.
    ///
    /// Release ordering on `packed` synchronizes with the Acquire load in
    /// `process_one` — so the hook thread, after observing the new packed value,
    /// also sees the reset latches that accompany the change.
    pub fn update_hotkey_keys(key1: u16, key2: u16) {
        let packed = (key2 as u32) << 16 | (key1 as u32);
        update_packed(&DICTATION, packed);
    }

    /// Update (or disable, with key1=0) the Command-Mode hotkey at runtime.
    pub fn update_command_hotkey_keys(key1: u16, key2: u16) {
        let packed = (key2 as u32) << 16 | (key1 as u32);
        update_packed(&COMMAND, packed);
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

    /// Bitmask of combo keys a HOLD-mode recording believes are still held.
    /// Layout: bit0/bit1 = dictation key1/key2, bit2/bit3 = command key1/key2;
    /// the matching VK codes are written into `out`.  Returns 0 when no
    /// hold-mode recording is active — idle, toggle-locked (double-tap), and
    /// mic-button sessions are never policed by the release watchdog.
    pub fn hold_believed_down(out: &mut [u16; 4]) -> u8 {
        let mut mask = 0u8;
        for (hk_idx, hk) in [&DICTATION, &COMMAND].into_iter().enumerate() {
            if !hk.recording.load(Ordering::Acquire) || hk.toggle_locked.load(Ordering::Acquire) {
                continue;
            }
            let packed = hk.packed.load(Ordering::Acquire);
            if packed == 0 {
                continue;
            }
            let keys = [(packed & 0xFFFF) as u16, ((packed >> 16) & 0xFFFF) as u16];
            let down = hk.keys_down.load(Ordering::Acquire);
            for (k_idx, &vk) in keys.iter().enumerate() {
                if vk != 0 && down & (1u8 << k_idx) != 0 {
                    let bit = hk_idx * 2 + k_idx;
                    out[bit] = vk;
                    mask |= 1 << bit;
                }
            }
        }
        mask
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
                    crate::pipeline::cancel_capture(&h, &st, crate::state::CaptureMode::Command);
                });
            }
            return true; // swallow — this Esc was aimed at OmniVox
        }

        // Enter/Esc while a confirm pill is pending drives it from the
        // keyboard.  Guard rails against confirming something the user never
        // saw: a 250ms arming debounce (an Enter finishing their typing must
        // not confirm), a 6s freshness window (a forgotten pill must not swallow
        // keys later — the mouse buttons keep working), AND an identity gate —
        // the hijack fires ONLY while the live foreground is still the window
        // that was foreground when the pill armed.  If the user tabbed away,
        // their Enter is meant for the current app, so we pass it through
        // untouched (H4).
        // Read the armed id FIRST as the anchor for a compare-and-consume.  The
        // pill's freshness window + arm-time foreground are validated below, but
        // the id (which the keyboard path echoes back) is consumed via
        // `compare_exchange` so a pending newly (re)armed in the sub-ms gap
        // between these separate atomic reads can't be confirmed by an Enter
        // meant for the PREVIOUS pill (B2-14).  `armed_id != 0` and `since != 0`
        // are set/cleared together in `set_confirm_pending`, so either gates the
        // "a pill is armed" check.
        let armed_id = CONFIRM_PENDING_ID.load(Ordering::Acquire);
        if armed_id != 0 && is_down && (vk == VK_RETURN || vk == VK_ESCAPE) {
            let since = CONFIRM_PENDING_SINCE_MS.load(Ordering::Acquire);
            let age = now_ms().saturating_sub(since);
            let arm_fg = CONFIRM_ARM_FG.load(Ordering::Acquire);
            // B2-7: `GetForegroundWindow` (via `current_foreground_isize`) is a
            // cheap, non-blocking user32 read of a value the window manager
            // already holds — it does NOT send cross-process messages the way
            // the banned `GetWindowText`/UIA calls do, so it's safe to call on
            // the serialized WH_KEYBOARD_LL hook thread.  No blocking Win32 call
            // is ever made here.
            let fg_matches = current_foreground_isize() == arm_fg;
            if since != 0 && (250..6_000).contains(&age) && fg_matches {
                // Atomically consume THIS exact arming.  A failed CAS means the
                // id changed since we anchored it (a newer pending armed, or
                // another path consumed it) — pass the key through rather than
                // confirm a command the user never saw (B2-14).  The residual
                // (validating the prior arm's fg/timestamp while its still-in-
                // flight store to the other two atomics races) is sub-µs and can
                // only *reject*, never confirm the wrong command — the pipeline
                // side re-checks the id under the pending lock regardless (B2-2).
                if CONFIRM_PENDING_ID
                    .compare_exchange(armed_id, 0, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    CONFIRM_PENDING_SINCE_MS.store(0, Ordering::Release);
                    CONFIRM_ARM_FG.store(0, Ordering::Release);
                    let confirm = vk == VK_RETURN;
                    if let Some(handle) = APP_HANDLE.get() {
                        let h = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            let st = h.state::<crate::state::AppState>();
                            if confirm {
                                // Keyboard confirm sends the message as heard —
                                // editing goes through the pill's textarea (mouse).
                                crate::pipeline::confirm_pending_command(
                                    &h,
                                    &st,
                                    Some(armed_id),
                                    None,
                                )
                                .await;
                            } else {
                                crate::pipeline::cancel_pending_command(&h, &st, Some(armed_id));
                            }
                        });
                    }
                    return true; // swallow
                }
            }
        }

        // Check both; swallow if either consumed the event. (The default combos
        // share no keys, so at most one fires per event.)
        let d = process_one(&DICTATION, Action::Dictation, vk, is_down, is_up);
        let c = process_one(&COMMAND, Action::Command, vk, is_down, is_up);
        d || c
    }

    fn process_one(hk: &Hk, action: Action, vk: u16, is_down: bool, is_up: bool) -> bool {
        process_one_with(
            hk,
            action,
            vk,
            is_down,
            is_up,
            fire_start,
            fire_stop,
            |fired: ChordFired| {
                // Nothing here may run inline: this is the serialized
                // WH_KEYBOARD_LL hook thread, which must return well inside
                // `LowLevelHooksTimeout`, and both the log (a lock + possible
                // 1 MiB rotation) and `SendInput` are too slow to risk.  The
                // utterance lasts far longer than this hop, so the modifier is
                // still held when the mask lands.
                tauri::async_runtime::spawn(async move {
                    crate::diag::log(&format!(
                        "hotkey: chord fired phase={} action={:?} completed_vk={:#06x} passed_vk={:#06x} mask={}",
                        fired.phase,
                        action_mode(fired.action),
                        fired.completed_vk,
                        fired.passed_vk,
                        if fired.mask_vk.is_some() { "yes" } else { "no" },
                    ));
                    #[cfg(target_os = "windows")]
                    if fired.mask_vk.is_some() {
                        send_menu_mask_key();
                    }
                });
            },
        )
    }

    /// The state machine proper, with its three side effects injected so the
    /// swallow/mask decisions can be unit-tested off Windows.
    #[allow(clippy::too_many_arguments)] // the event + the three injected effects
    fn process_one_with(
        hk: &Hk,
        action: Action,
        vk: u16,
        is_down: bool,
        is_up: bool,
        start: impl Fn(Action) -> bool,
        stop: impl Fn(Action),
        fired: impl Fn(ChordFired),
    ) -> bool {
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

        // Report a chord that just fired.  The chord's OTHER key reached the
        // app on its way down (only the completing key is swallowed).  When
        // that key is Alt or Win, the app is holding what looks to it like a
        // bare menu-activation gesture: on release it moves keyboard focus to
        // the menu bar, silently, so our paste lands in the menu instead of the
        // composer (and the next chord toggles it back — hence every *other*
        // dictation failing).  One benign keystroke while the key is still held
        // makes it "Alt + something" instead.
        let report = |phase: &'static str| {
            fired(ChordFired {
                phase,
                action,
                completed_vk: vk,
                passed_vk: passed_through_key(key1, key2, bit),
                mask_vk: passed_through_menu_modifier(key1, key2, bit),
            });
        };

        // ── Both/all keys just pressed ───────────────────────
        // Only a COMBO key may drive this: while both chord keys are held,
        // `all_down` stays true for every other key-down too (including the
        // mask keystroke we inject), and letting those through here would
        // toggle a capture off a key that has nothing to do with the hotkey.
        if all_down && is_down && (matches_key1 || matches_key2) {
            if !recording {
                let now = now_ms();
                let last = hk.last_activate_ms.swap(now, Ordering::Relaxed);
                let is_double_tap = is_double_tap(now, last);

                hk.recording.store(true, Ordering::Relaxed);
                hk.toggle_locked.store(is_double_tap, Ordering::Relaxed);
                // Remember we swallowed this key's press so we also swallow its
                // release — otherwise the lone modifier-up leaks to the app.
                hk.swallowed_down.fetch_or(bit, Ordering::Relaxed);
                if !start(action) {
                    // The other hotkey (or the mic button) owns capture. Roll
                    // back every optimistic latch so this combo cannot become a
                    // phantom recording/toggle session.
                    hk.recording.store(false, Ordering::Release);
                    hk.toggle_locked.store(false, Ordering::Release);
                    hk.last_activate_ms.store(0, Ordering::Release);
                    hk.swallowed_down.fetch_and(!bit, Ordering::AcqRel);
                    return false;
                }

                report("start");

                return true; // swallow
            } else if locked {
                // Toggle-off
                hk.recording.store(false, Ordering::Relaxed);
                hk.toggle_locked.store(false, Ordering::Relaxed);
                hk.last_activate_ms.store(0, Ordering::Relaxed);
                hk.swallowed_down.fetch_or(bit, Ordering::Relaxed);
                stop(action);
                // Same leak, worse timed: this is the chord immediately before
                // the transcript is pasted.
                report("stop");

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
        if is_down
            && (matches_key1 || matches_key2)
            && hk.swallowed_down.load(Ordering::Relaxed) & bit != 0
        {
            return true; // swallow the repeat
        }

        // ── Key released while hold-recording (non-locked) ──
        // Claim the recording→stopped transition with a CAS: the same release
        // edge can reach this branch from more than one thread (OS hook, DOM
        // bridge command, and the release watchdog all feed process_key_event),
        // and a plain load-then-store would let two of them observe `true` and
        // both spawn a stop pipeline.  Exactly one caller may fire the stop.
        if recording
            && !locked
            && is_up
            && (matches_key1 || matches_key2)
            && hk
                .recording
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        {
            stop(action);
        }

        // ── Swallow the release of any combo key whose activating press we
        //    swallowed, so the foreground app never sees a lone modifier
        //    release.  A leaked `Alt`-up pops the Windows menu/ribbon KeyTips
        //    overlay (the "letters across the top" the user hits in Notes);
        //    other leaked modifier releases can disturb the focused control.
        //    Balanced per-key: a modifier whose down we passed through (the
        //    first key of the combo, or a real Ctrl+C / Alt+Tab) is NOT in
        //    `swallowed_down`, so its up still passes — keeping those intact.
        if is_up
            && (matches_key1 || matches_key2)
            && hk.swallowed_down.fetch_and(!bit, Ordering::Relaxed) & bit != 0
        {
            apply_pending_if_idle(hk);
            return true; // swallow the balancing release
        }

        apply_pending_if_idle(hk);
        false
    }

    #[cfg(test)]
    mod tests {
        use std::cell::RefCell;

        use super::*;

        #[test]
        fn first_activation_is_never_a_double_tap() {
            assert!(!is_double_tap(100, 0));
        }

        #[test]
        fn double_tap_uses_saturating_elapsed_time() {
            assert!(is_double_tap(500, 150));
            assert!(!is_double_tap(700, 150));
            assert!(is_double_tap(10, 20));
        }

        const CTRL: u16 = 0xA2;
        const ALT: u16 = 0xA4;
        const WIN: u16 = 0x5B;
        const SHIFT: u16 = 0xA0;

        /// What the injected effects saw, in order.
        #[derive(Default)]
        struct Rec {
            /// `(vk, is_down, swallowed)` per event fed.
            events: Vec<(u16, bool, bool)>,
            starts: Vec<Action>,
            stops: Vec<Action>,
            fires: Vec<ChordFired>,
        }

        impl Rec {
            /// The events that reached the foreground app.
            fn delivered(&self) -> Vec<(u16, bool)> {
                self.events
                    .iter()
                    .filter(|&&(_, _, swallowed)| !swallowed)
                    .map(|&(vk, is_down, _)| (vk, is_down))
                    .collect()
            }

            /// The mask keystrokes the state machine asked for, in order.
            fn masks(&self) -> Vec<u16> {
                self.fires.iter().filter_map(|f| f.mask_vk).collect()
            }
        }

        /// An idle hotkey with `key1`+`key2` configured.
        fn hk(key1: u16, key2: u16) -> Hk {
            let hk = Hk::new();
            hk.packed
                .store((key2 as u32) << 16 | (key1 as u32), Ordering::Release);
            hk
        }

        fn feed(hk: &Hk, rec: &RefCell<Rec>, vk: u16, is_down: bool, start_ok: bool) -> bool {
            let swallowed = process_one_with(
                hk,
                Action::Dictation,
                vk,
                is_down,
                !is_down,
                |a| {
                    rec.borrow_mut().starts.push(a);
                    start_ok
                },
                |a| rec.borrow_mut().stops.push(a),
                |f| rec.borrow_mut().fires.push(f),
            );
            rec.borrow_mut().events.push((vk, is_down, swallowed));
            swallowed
        }

        /// One hold-mode chord: both keys down in `press` order, then up in
        /// `release` order.  `last_activate_ms` is cleared first so back-to-back
        /// cycles never read as a 400 ms double-tap (toggle mode).
        fn chord(hk: &Hk, rec: &RefCell<Rec>, press: [u16; 2], release: [u16; 2]) {
            hk.last_activate_ms.store(0, Ordering::Relaxed);
            for vk in press {
                feed(hk, rec, vk, true, true);
            }
            for vk in release {
                feed(hk, rec, vk, false, true);
            }
        }

        #[test]
        fn ctrl_first_chord_delivers_no_alt_events_to_the_app() {
            for release in [[CTRL, ALT], [ALT, CTRL]] {
                let h = hk(CTRL, ALT);
                let rec = RefCell::new(Rec::default());
                chord(&h, &rec, [CTRL, ALT], release);
                let r = rec.borrow();
                // Alt completes the chord, so both its edges are swallowed and
                // the app never sees a menu gesture — nothing to mask.
                assert_eq!(r.delivered(), vec![(CTRL, true), (CTRL, false)]);
                assert!(r.masks().is_empty(), "release={release:?}");
                assert_eq!(r.starts, vec![Action::Dictation]);
                assert_eq!(r.stops, vec![Action::Dictation]);
            }
        }

        #[test]
        fn alt_first_chord_emits_the_menu_mask_key() {
            for release in [[ALT, CTRL], [CTRL, ALT]] {
                let h = hk(CTRL, ALT);
                let rec = RefCell::new(Rec::default());
                chord(&h, &rec, [ALT, CTRL], release);
                let r = rec.borrow();
                // Alt's own edges pass through (its down was never swallowed),
                // which is the bare menu gesture — mask it exactly once.
                assert_eq!(r.delivered(), vec![(ALT, true), (ALT, false)]);
                assert_eq!(r.masks(), vec![ALT], "release={release:?}");
                assert_eq!(r.starts, vec![Action::Dictation]);
                assert_eq!(r.stops, vec![Action::Dictation]);
            }
        }

        #[test]
        fn alt_first_toggle_off_emits_the_menu_mask_key() {
            let h = hk(CTRL, ALT);
            let rec = RefCell::new(Rec::default());
            // Arm the double-tap window so the next chord locks toggle mode
            // instead of recording hold-style.
            h.last_activate_ms.store(now_ms().max(1), Ordering::Relaxed);
            for (vk, is_down) in [(ALT, true), (CTRL, true), (ALT, false), (CTRL, false)] {
                feed(&h, &rec, vk, is_down, true);
            }
            assert!(h.toggle_locked.load(Ordering::Relaxed), "should be locked");
            assert!(h.recording.load(Ordering::Relaxed));
            {
                let r = rec.borrow();
                assert_eq!(r.masks(), vec![ALT]);
                assert_eq!(r.starts, vec![Action::Dictation]);
                assert!(r.stops.is_empty(), "toggle mode outlives the release");
            }

            // The stopping chord leaks the same bare Alt tap — and this one
            // lands immediately before the paste.
            for (vk, is_down) in [(ALT, true), (CTRL, true), (ALT, false), (CTRL, false)] {
                feed(&h, &rec, vk, is_down, true);
            }
            let r = rec.borrow();
            assert_eq!(r.masks(), vec![ALT, ALT], "exactly one more mask");
            assert_eq!(
                r.fires.iter().map(|f| f.phase).collect::<Vec<_>>(),
                vec!["start", "stop"]
            );
            assert_eq!(r.starts, vec![Action::Dictation]);
            assert_eq!(r.stops, vec![Action::Dictation]);
            drop(r);
            assert!(!h.recording.load(Ordering::Relaxed));
            assert!(!h.toggle_locked.load(Ordering::Relaxed));
            assert_eq!(h.keys_down.load(Ordering::Relaxed), 0);
            assert_eq!(h.swallowed_down.load(Ordering::Relaxed), 0);
        }

        /// `recording`, `toggle_locked`, `keys_down`, `swallowed_down`.
        fn latches(hk: &Hk) -> (bool, bool, u8, u8) {
            (
                hk.recording.load(Ordering::Relaxed),
                hk.toggle_locked.load(Ordering::Relaxed),
                hk.keys_down.load(Ordering::Relaxed),
                hk.swallowed_down.load(Ordering::Relaxed),
            )
        }

        /// `(starts, stops, fires)` call counts.
        fn calls(rec: &RefCell<Rec>) -> (usize, usize, usize) {
            let r = rec.borrow();
            (r.starts.len(), r.stops.len(), r.fires.len())
        }

        /// Hold both chord keys (Alt first), optionally as a toggle-lock.
        fn hold_chord(hk: &Hk, rec: &RefCell<Rec>, lock: bool) {
            hk.last_activate_ms
                .store(if lock { now_ms().max(1) } else { 0 }, Ordering::Relaxed);
            feed(hk, rec, ALT, true, true);
            feed(hk, rec, CTRL, true, true);
            assert_eq!(hk.toggle_locked.load(Ordering::Relaxed), lock);
        }

        #[test]
        fn injected_mask_key_cannot_drive_the_state_machine() {
            // 0xE8 is what `send_menu_mask_key` injects.  The hook filters it
            // out by `dwExtraInfo` before this point, but the state machine
            // must be inert for it regardless: it arrives ~1 ms after the
            // chord fired, while both combo keys are still held.
            const MASK: u16 = 0xE8;
            for lock in [true, false] {
                let h = hk(CTRL, ALT);
                let rec = RefCell::new(Rec::default());
                hold_chord(&h, &rec, lock);
                let (before, before_calls) = (latches(&h), calls(&rec));

                assert!(
                    !feed(&h, &rec, MASK, true, true),
                    "mask down passes through"
                );
                assert!(!feed(&h, &rec, MASK, false, true), "mask up passes through");

                assert_eq!(calls(&rec), before_calls, "lock={lock}");
                assert_eq!(latches(&h), before, "lock={lock}");
            }
        }

        #[test]
        fn third_key_while_chord_held_passes_through() {
            const KEY_A: u16 = 0x41;
            let h = hk(CTRL, ALT);
            let rec = RefCell::new(Rec::default());
            hold_chord(&h, &rec, false);
            let (before, before_calls) = (latches(&h), calls(&rec));

            assert!(!feed(&h, &rec, KEY_A, true, true));
            assert!(!feed(&h, &rec, KEY_A, false, true));

            assert_eq!(calls(&rec), before_calls);
            assert_eq!(latches(&h), before);
        }

        #[cfg(target_os = "windows")]
        #[test]
        fn mask_extra_info_signature_is_nonzero_and_stable() {
            // The hook passes a keystroke through untouched when it carries
            // this exact value, so it must never be 0 — ordinary keys carry 0.
            assert_ne!(MASK_EXTRA_INFO, 0);
            assert_eq!(MASK_EXTRA_INFO, 0x4F4D_4E49_5F4D_534B);
        }

        #[test]
        fn win_first_chord_emits_the_menu_mask_key() {
            let h = hk(CTRL, WIN);
            let rec = RefCell::new(Rec::default());
            chord(&h, &rec, [WIN, CTRL], [WIN, CTRL]);
            assert_eq!(rec.borrow().masks(), vec![WIN]);
        }

        #[test]
        fn shift_first_chord_emits_no_mask() {
            let h = hk(CTRL, SHIFT);
            let rec = RefCell::new(Rec::default());
            chord(&h, &rec, [SHIFT, CTRL], [SHIFT, CTRL]);
            assert!(rec.borrow().masks().is_empty());
        }

        #[test]
        fn every_swallowed_down_has_exactly_one_swallowed_up() {
            for press in [[CTRL, ALT], [ALT, CTRL]] {
                for release in [[CTRL, ALT], [ALT, CTRL]] {
                    let h = hk(CTRL, ALT);
                    let rec = RefCell::new(Rec::default());
                    chord(&h, &rec, press, release);
                    let delivered = rec.borrow().delivered();
                    for vk in [CTRL, ALT] {
                        let downs = delivered.iter().filter(|&&e| e == (vk, true)).count();
                        let ups = delivered.iter().filter(|&&e| e == (vk, false)).count();
                        assert_eq!(
                            downs, ups,
                            "press={press:?} release={release:?} vk={vk:#06x}"
                        );
                    }
                    assert_eq!(h.swallowed_down.load(Ordering::Relaxed), 0);
                    assert_eq!(h.keys_down.load(Ordering::Relaxed), 0);
                }
            }
        }

        #[test]
        fn state_is_clean_between_consecutive_dictations() {
            let h = hk(CTRL, ALT);
            let mut expected = None;
            for _ in 0..5 {
                let rec = RefCell::new(Rec::default());
                chord(&h, &rec, [ALT, CTRL], [ALT, CTRL]);
                let r = rec.borrow();
                let this = (r.events.clone(), r.fires.clone());
                let baseline = expected.get_or_insert_with(|| this.clone());
                assert_eq!(*baseline, this);
                assert_eq!(r.starts.len(), 1);
                assert_eq!(r.stops.len(), 1);
            }
            assert_eq!(h.keys_down.load(Ordering::Relaxed), 0);
            assert_eq!(h.swallowed_down.load(Ordering::Relaxed), 0);
            assert!(!h.recording.load(Ordering::Relaxed));
            assert!(!h.toggle_locked.load(Ordering::Relaxed));
        }

        #[test]
        fn start_rejected_rolls_back_without_masking() {
            let h = hk(CTRL, ALT);
            let rec = RefCell::new(Rec::default());
            assert!(!feed(&h, &rec, ALT, true, false));
            // The chord fires but capture is owned elsewhere: the press passes
            // through and nothing — including the mask — may be emitted.
            assert!(!feed(&h, &rec, CTRL, true, false));
            let r = rec.borrow();
            assert_eq!(r.starts, vec![Action::Dictation]);
            assert!(r.masks().is_empty());
            assert!(r.stops.is_empty());
            drop(r);
            assert_eq!(h.swallowed_down.load(Ordering::Relaxed), 0);
            assert!(!h.recording.load(Ordering::Relaxed));
            assert!(!h.toggle_locked.load(Ordering::Relaxed));
            assert_eq!(h.last_activate_ms.load(Ordering::Relaxed), 0);
        }

        #[test]
        fn passed_through_menu_modifier_flags_only_leaked_menu_keys() {
            // (key1, key2, completing_bit) → the key that leaked, if it pops a menu.
            let cases = [
                ((CTRL, ALT, 0x01), Some(ALT)), // Alt-first: Ctrl completed
                ((CTRL, ALT, 0x02), None),      // Ctrl-first: Alt completed
                ((ALT, CTRL, 0x02), Some(ALT)), // same chord, keys stored swapped
                ((CTRL, WIN, 0x01), Some(WIN)),
                ((CTRL, 0xA5, 0x01), Some(0xA5)), // RAlt
                ((CTRL, 0x5C, 0x01), Some(0x5C)), // RWin
                ((0x12, CTRL, 0x02), Some(0x12)), // unsided VK_MENU
                ((CTRL, SHIFT, 0x01), None),      // Shift pops no menu
                ((0xA3, 0, 0x01), None),          // single-key combo leaks nothing
            ];
            for ((key1, key2, bit), want) in cases {
                assert_eq!(
                    passed_through_menu_modifier(key1, key2, bit),
                    want,
                    "key1={key1:#06x} key2={key2:#06x} bit={bit:#04x}"
                );
            }
        }
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
            // Our own menu-mask keystroke: pass it through without letting it
            // touch the state machine.  It arrives while both chord keys are
            // held, and the machine's "combo is all down" latches would read it
            // as another press of the hotkey — toggling off the very capture it
            // was sent to protect.  Only OUR injection is skipped: enigo's
            // paste keystrokes carry no signature and keep flowing through the
            // normal path (`LLKHF_INJECTED` is deliberately NOT consulted).
            if kb.dwExtraInfo == state_machine::MASK_EXTRA_INFO {
                return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
            }
            let vk = kb.vkCode as u16;
            let is_down = wparam == WM_KEYDOWN as usize || wparam == WM_SYSKEYDOWN as usize;
            let is_up = wparam == WM_KEYUP as usize || wparam == WM_SYSKEYUP as usize;

            if state_machine::process_key_event(vk, is_down, is_up) {
                return 1; // swallow
            }
        }

        unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
    }

    /// True when the current foreground window belongs to this process (one of
    /// OmniVox's own windows — main, scratchpad, overlay — is in front).
    fn foreground_is_self() -> bool {
        use windows_sys::Win32::System::Threading::GetCurrentProcessId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetAncestor, GetForegroundWindow, GetWindowThreadProcessId, GA_ROOT,
        };
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() {
                return false;
            }
            let root = GetAncestor(fg, GA_ROOT);
            let hwnd = if root.is_null() { fg } else { root };
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            pid == GetCurrentProcessId()
        }
    }

    /// Hold-release watchdog.  When one of OmniVox's own WebView windows is
    /// focused (e.g. dictating into the scratchpad), hotkey key-UP events can
    /// be lost: the WebView consumes input the OS hook path doesn't drive, and
    /// the DOM key bridge never receives a keyup for a key whose keydown was
    /// swallowed — or misses it entirely when the window loses focus mid-hold.
    /// A lost release leaves a hold-mode recording running forever, which reads
    /// as an unwanted "long-running dictation".
    ///
    /// This thread polls PHYSICAL key state while (and only while) a hold-mode
    /// recording is live and an OmniVox window is foreground.  If a combo key
    /// the state machine believes is held reads physically up on two
    /// consecutive polls, the missed key-up is synthesized through the same
    /// `process_key_event` path — firing the normal release-stop.
    ///
    /// Deliberately inert for every long-running mode: toggle-locked
    /// (double-tap) and mic-button sessions are excluded by
    /// `hold_believed_down`, and the foreground gate keeps it from ever
    /// second-guessing dictation into other apps (where the OS hook is
    /// reliable, and where UIPI can blank `GetAsyncKeyState` for elevated
    /// targets).
    fn start_release_watchdog() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

        thread::Builder::new()
            .name("omnivox-hotkey-watchdog".into())
            .spawn(|| {
                let key_up =
                    |vk: u16| unsafe { (GetAsyncKeyState(vk as i32) as u16) & 0x8000 == 0 };
                // Phantom bits seen last poll — a key must read stuck on two
                // consecutive polls before its release is synthesized.
                let mut prev_phantom = 0u8;
                // Keys observed physically DOWN at least once during the
                // current hold (cleared when the hold ends).  Only these may
                // ever count as phantom: a key-down the hook swallowed never
                // enters the async key-state table and reads "up" for its
                // whole legitimate hold — without this gate a single-key
                // combo (no passed-through sibling to veto the group) could
                // be false-stopped moments after it started.
                let mut seen_down = 0u8;
                loop {
                    let mut vks = [0u16; 4];
                    if state_machine::hold_believed_down(&mut vks) == 0 {
                        prev_phantom = 0;
                        seen_down = 0;
                        thread::sleep(std::time::Duration::from_millis(150));
                        continue;
                    }
                    thread::sleep(std::time::Duration::from_millis(30));
                    if !foreground_is_self() {
                        prev_phantom = 0;
                        continue;
                    }
                    // Re-read after the sleep so we compare fresh belief
                    // against fresh physical state.
                    let mut vks = [0u16; 4];
                    let believed = state_machine::hold_believed_down(&mut vks);
                    // A hotkey's hold counts as phantom only when ALL keys it
                    // believes are held read physically up, AND every one of
                    // them was seen physically down earlier in this hold.
                    // Per-key checks would false-positive: a key-down the hook
                    // SWALLOWED never reaches the async key-state table, so
                    // that key reads "up" for the entire (legitimate) hold —
                    // the seen-down gate means such a key can simply never
                    // trip the watchdog, in which case the hook that swallowed
                    // it also reliably delivers its release.
                    let mut physically_down = 0u8;
                    for bit in 0..4u8 {
                        if believed & (1 << bit) != 0 && !key_up(vks[bit as usize]) {
                            physically_down |= 1 << bit;
                        }
                    }
                    // Eligibility never outlives belief — a slot the machine no
                    // longer holds must re-earn seen-down in its next hold.
                    seen_down = (seen_down & believed) | physically_down;
                    let mut phantom = 0u8;
                    for group in [0b0011u8, 0b1100u8] {
                        let bits = believed & group;
                        if bits != 0 && bits & physically_down == 0 && bits & seen_down == bits {
                            phantom |= bits;
                        }
                    }
                    let confirmed = phantom & prev_phantom;
                    prev_phantom = phantom;
                    for bit in 0..4u8 {
                        if confirmed & (1 << bit) != 0 {
                            crate::llm::diaglog::log(&format!(
                                "hotkey: watchdog synthesizing missed key-up vk={:#06x}",
                                vks[bit as usize]
                            ));
                            state_machine::process_key_event(vks[bit as usize], false, true);
                        }
                    }
                    if confirmed != 0 {
                        prev_phantom = 0;
                    }
                }
            })
            .expect("Failed to spawn hotkey watchdog thread");
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

        start_release_watchdog();
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
/// confirm.  Call with `Some(id)` (the parked command's exact id) when a
/// `PendingCommand` is parked and the confirm pill shown; with `None` whenever
/// it is consumed or cleared.
pub fn set_confirm_pending(pending_id: Option<u64>) {
    state_machine::set_confirm_pending(pending_id);
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
