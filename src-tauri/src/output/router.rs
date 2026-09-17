use std::borrow::Cow;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use arboard::{Clipboard, ImageData};
use enigo::{Axis, Button, Direction, Enigo, Key, Keyboard, Mouse, Settings};

/// The modifier key used for paste (Ctrl+V on Windows/Linux, Cmd+V on macOS).
#[cfg(target_os = "macos")]
const PASTE_MODIFIER: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const PASTE_MODIFIER: Key = Key::Control;

/// The modifier key used for delete-word (Ctrl+Backspace on Windows/Linux,
/// Option+Backspace on macOS).
#[cfg(target_os = "macos")]
const DELETE_WORD_MODIFIER: Key = Key::Alt;
#[cfg(not(target_os = "macos"))]
const DELETE_WORD_MODIFIER: Key = Key::Control;

const CLIPBOARD_VERIFY_TIMEOUT_MS: u64 = 750;
const CLIPBOARD_VERIFY_INTERVAL_MS: u64 = 10;
// Audited 2026-07-06: this guard does double duty — clipboard stability for
// deferred-read apps AND a paste-before-next-keystroke ordering barrier
// (dropping it between a Text and a following Command segment can land the
// keystroke before the app processed Ctrl+V).  Since resolve_list_segments
// merges adjacent text segments, plain dictations pay it once; don't shrink
// it per-segment without testing slow targets (Word, browser textareas).
const POST_PASTE_GUARD_MS: u64 = 250;

/// Process-wide serialization for clipboard and synthetic-input transactions.
/// This must be static because Command Mode constructs short-lived routers.
static OUTPUT_TRANSACTION: Mutex<()> = Mutex::new(());

use crate::error::{AppError, AppResult};
use crate::output::types::{OutputConfig, OutputMode};
use crate::postprocess::voice_commands::{
    segments_to_string, ComboKey, KeyModifier, OutputSegment, VoiceCommand,
};

/// Routes transcribed text to the user's focused application.
///
/// Supports three output modes:
/// - **Clipboard**: Copies dictation to the clipboard. User pastes manually.
/// - **TypeSimulation**: Pastes via Ctrl+V, then restores the user's prior
///   clipboard so a pre-copied snippet survives the dictation. Paste itself
///   still uses the clipboard internally — the restore happens after the
///   250 ms deferred-read guard, by which point well-behaved apps have
///   already consumed the pasted text.
/// - **Both**: Pastes AND leaves the dictation on the clipboard for repeat
///   pasting. The explicit "I want a copy too" mode.
pub struct OutputRouter;

impl Default for OutputRouter {
    fn default() -> Self {
        Self::new()
    }
}

enum ClipboardSnapshot {
    Text(String),
    Image {
        width: usize,
        height: usize,
        bytes: Vec<u8>,
    },
}

#[derive(Clone, Copy)]
enum TargetPolicy {
    CallerVerified,
    Verify(Option<crate::focus::WindowTarget>),
}

impl OutputRouter {
    pub fn new() -> Self {
        Self
    }

    pub fn send(&self, text: &str, config: &OutputConfig) -> AppResult<()> {
        if text.is_empty() {
            return Ok(());
        }

        Self::with_transaction(|| self.send_unlocked(text, config, TargetPolicy::CallerVerified))
    }

    /// Target-bound variant for the dictation pipeline. Verification happens
    /// inside the serialized transaction immediately before Ctrl+V.
    pub fn send_to_target(
        &self,
        text: &str,
        config: &OutputConfig,
        target: Option<crate::focus::WindowTarget>,
    ) -> AppResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        Self::with_transaction(|| self.send_unlocked(text, config, TargetPolicy::Verify(target)))
    }

    /// Atomically paste a plain dictation and submit it with Enter after the
    /// caller-selected settle interval. A concrete target is required: Ship
    /// Mode is consequential and never falls back to firing into an unknown
    /// foreground window. The process-wide transaction remains held from the
    /// clipboard write through the final Enter, so no Command Mode or dictation
    /// output can interleave between them.
    pub fn send_to_target_with_submit(
        &self,
        text: &str,
        config: &OutputConfig,
        target: crate::focus::WindowTarget,
        submit_settle: Duration,
    ) -> AppResult<()> {
        if text.is_empty() {
            return Ok(());
        }
        Self::require_submit_capable_mode(config)?;
        Self::with_transaction(|| {
            self.send_unlocked(text, config, TargetPolicy::Verify(Some(target)))?;
            Self::submit_to_target_unlocked(target, submit_settle)
        })
    }

    fn send_unlocked(
        &self,
        text: &str,
        config: &OutputConfig,
        target_policy: TargetPolicy,
    ) -> AppResult<()> {
        match config.mode {
            OutputMode::Clipboard => {
                self.set_clipboard(text)?;
            }
            OutputMode::TypeSimulation => {
                self.paste_text_unlocked(text, true, target_policy)?;
            }
            OutputMode::Both => {
                self.paste_text_unlocked(text, false, target_policy)?;
            }
        }

        Ok(())
    }

    /// Send a sequence of text segments and voice commands to the focused app.
    ///
    /// In **TypeSimulation** mode, text segments are pasted and commands execute
    /// keystrokes; the user's prior clipboard is restored at the end.
    /// In **Clipboard** mode, segments are collapsed to a string and copied.
    /// In **Both** mode, segments are pasted/executed and the concatenated
    /// dictation is left on the clipboard.
    ///
    /// `target` is the window this dictation was aimed at (bound + pid): before
    /// each CONSEQUENTIAL inline command (Send/Enter, mouse, key combo) the
    /// router re-verifies the foreground is still that window, so a stray
    /// mishearing can't fire OS input into whatever grabbed focus (H5).
    /// `allow_launch` gates the disruptive `LaunchApp` command (from the
    /// `launch_app_voice_commands_enabled` setting, default ON).
    pub fn send_segments(
        &self,
        segments: &[OutputSegment],
        config: &OutputConfig,
        target: Option<crate::focus::WindowTarget>,
        allow_launch: bool,
    ) -> AppResult<()> {
        if segments.is_empty() {
            return Ok(());
        }

        Self::with_transaction(|| {
            self.send_segments_unlocked(segments, config, target, allow_launch)
        })
    }

    /// Atomic Ship Mode variant for a dictation containing inline voice
    /// command segments. See [`send_to_target_with_submit`] for the ordering
    /// and target-identity guarantees.
    pub fn send_segments_to_target_with_submit(
        &self,
        segments: &[OutputSegment],
        config: &OutputConfig,
        target: crate::focus::WindowTarget,
        allow_launch: bool,
        submit_settle: Duration,
    ) -> AppResult<()> {
        if segments.is_empty() {
            return Ok(());
        }
        Self::require_submit_capable_mode(config)?;
        Self::with_transaction(|| {
            self.send_segments_unlocked(segments, config, Some(target), allow_launch)?;
            Self::submit_to_target_unlocked(target, submit_settle)
        })
    }

    fn send_segments_unlocked(
        &self,
        segments: &[OutputSegment],
        config: &OutputConfig,
        target: Option<crate::focus::WindowTarget>,
        allow_launch: bool,
    ) -> AppResult<()> {
        match config.mode {
            OutputMode::Clipboard => {
                let text = segments_to_string(segments);
                if !text.is_empty() {
                    self.set_clipboard(&text)?;
                }
            }
            OutputMode::TypeSimulation => {
                self.execute_segments(segments, true, target, allow_launch)?;
            }
            OutputMode::Both => {
                self.execute_segments(segments, false, target, allow_launch)?;
            }
        }

        Ok(())
    }

    /// Execute a sequence of text + command segments via paste + keystrokes.
    ///
    /// When `restore_prior_clipboard` is true the user's prior clipboard text
    /// (if any) is restored after the final deferred-read guard, so the
    /// dictation does not linger in clipboard. When false, the concatenated
    /// dictation is written to the clipboard for re-pasting (Both mode).
    fn execute_segments(
        &self,
        segments: &[OutputSegment],
        restore_prior_clipboard: bool,
        target: Option<crate::focus::WindowTarget>,
        allow_launch: bool,
    ) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

        // Snapshot the user's clipboard BEFORE we overwrite it. Only used in
        // TypeSimulation mode; captured eagerly so it's still the *prior*
        // contents (not a half-written dictation) by the time we restore.
        let saved_clipboard = if restore_prior_clipboard {
            Self::capture_clipboard(&mut clipboard)
        } else {
            None
        };

        let mut last_router_clipboard: Option<String> = None;
        let mut command_owns_clipboard = false;
        let mut last_paste_at: Option<Instant> = None;
        let operation_result = (|| {
            for seg in segments {
                match seg {
                    OutputSegment::Text(s) => {
                        if !s.is_empty() {
                            Self::require_foreground_target(target, "paste")?;
                            Self::set_clipboard_verified(&mut clipboard, s)?;
                            last_router_clipboard = Some(s.clone());
                            command_owns_clipboard = false;
                            Self::require_foreground_target(target, "paste")?;
                            Self::paste_keystroke(&mut enigo)?;
                            last_paste_at = Some(Instant::now());
                            thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));
                        }
                    }
                    OutputSegment::Command(cmd) => {
                        Self::run_command(&mut enigo, cmd, target, allow_launch, last_paste_at)?;
                        if Self::command_transfers_clipboard(cmd) {
                            command_owns_clipboard = true;
                            last_router_clipboard = None;
                        }
                    }
                }
            }
            Ok(())
        })();

        let cleanup_result = if !command_owns_clipboard && restore_prior_clipboard {
            // TypeSimulation: return the clipboard to whatever the user had
            // before dictating. If we couldn't read text (image, empty, error)
            // we leave the last-pasted segment in place — safer than clearing
            // their (possibly non-text) clipboard.
            match (saved_clipboard, last_router_clipboard.as_deref()) {
                (Some(prior), Some(owned)) => {
                    Self::restore_clipboard_if_owned(&mut clipboard, prior, owned)
                }
                _ => Ok(()),
            }
        } else if !command_owns_clipboard {
            // Both: write the full concatenated dictation so the user can
            // re-paste it. Also defends deferred-read apps from seeing only
            // the trailing segment.
            let final_text = segments_to_string(segments);
            if let Some(owned) = last_router_clipboard.as_deref() {
                if !final_text.is_empty()
                    && Self::clipboard_contains_owned_text(&mut clipboard, owned)
                {
                    Self::set_clipboard_verified(&mut clipboard, &final_text)
                } else {
                    Ok(())
                }
            } else {
                Ok(())
            }
        } else {
            Ok(())
        };

        // Cleanup always runs, but an execution failure remains the primary
        // error if restoration also fails.
        operation_result.and(cleanup_result)
    }

    /// Command Mode's atomic "type, then optionally submit" operation. The
    /// paste, clipboard restoration, cancellation gate, final target check,
    /// and Enter all run under the same process-wide output transaction so a
    /// second dictation cannot interleave between paste and submit.
    pub(crate) fn paste_text_to_target_with_submit(
        &self,
        text: &str,
        restore_prior_clipboard: bool,
        target: crate::focus::WindowTarget,
        submit: bool,
        should_cancel: impl Fn() -> bool,
    ) -> AppResult<()> {
        Self::with_transaction(|| {
            // The caller's pre-check happened before it could wait on this
            // transaction. Re-check after acquisition so a queued cancellation
            // prevents even a non-submitting paste.
            if should_cancel() {
                return Err(AppError::Output("stopped".into()));
            }
            self.paste_text_unlocked(
                text,
                restore_prior_clipboard,
                TargetPolicy::Verify(Some(target)),
            )?;
            if submit {
                // This intentionally runs after the paste guard and before the
                // consequential Enter, while the transaction is still held.
                if should_cancel() {
                    return Err(AppError::Output("stopped".into()));
                }
                let mut enigo = Enigo::new(&Settings::default()).map_err(|e| {
                    AppError::Output(format!("Failed to init keystroke engine: {e}"))
                })?;
                Self::require_foreground_target(Some(target), "submit pasted text")?;
                enigo
                    .key(Key::Return, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Send (Enter) failed: {e}")))?;
            }
            Ok(())
        })
    }

    /// Collapse a selection after restoring a window without letting these
    /// synthetic arrows overlap a clipboard paste or another command.
    pub(crate) fn deselect_target(&self, target: crate::focus::WindowTarget) -> AppResult<()> {
        Self::with_transaction(|| {
            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;
            Self::require_foreground_target(Some(target), "collapse selection")?;
            enigo
                .key(Key::RightArrow, Direction::Click)
                .map_err(|e| AppError::Output(format!("Selection collapse failed: {e}")))?;
            thread::sleep(Duration::from_millis(2));
            Self::require_foreground_target(Some(target), "finish collapsing selection")?;
            enigo
                .key(Key::LeftArrow, Direction::Click)
                .map_err(|e| AppError::Output(format!("Selection collapse failed: {e}")))
        })
    }

    fn paste_text_unlocked(
        &self,
        text: &str,
        restore_prior_clipboard: bool,
        target_policy: TargetPolicy,
    ) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

        let saved_clipboard = if restore_prior_clipboard {
            Self::capture_clipboard(&mut clipboard)
        } else {
            None
        };

        // Refuse to press Ctrl+V unless the clipboard readback matches this
        // dictation. Otherwise a transient clipboard race can paste the user's
        // previous clipboard into the target app.
        Self::set_clipboard_verified(&mut clipboard, text)?;

        let operation_result = (|| {
            self.send_paste_keystroke(target_policy)?;

            // Keep the clipboard stable long enough for target apps that read it
            // on a deferred tick after Ctrl+V.
            thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));
            Ok(())
        })();

        let cleanup_result = if restore_prior_clipboard {
            match saved_clipboard {
                Some(prior) => Self::restore_clipboard_if_owned(&mut clipboard, prior, text),
                None => Ok(()),
            }
        } else {
            Ok(())
        };

        operation_result.and(cleanup_result)
    }

    fn submit_to_target_unlocked(
        target: crate::focus::WindowTarget,
        settle: Duration,
    ) -> AppResult<()> {
        thread::sleep(settle);
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;
        // Verify after both the settle and Enigo initialization, immediately
        // before the consequential Enter primitive.
        Self::require_foreground_target(Some(target), "submit pasted text")?;
        enigo
            .key(Key::Return, Direction::Click)
            .map_err(|e| AppError::Output(format!("Send (Enter) failed: {e}")))
    }

    fn require_submit_capable_mode(config: &OutputConfig) -> AppResult<()> {
        if matches!(config.mode, OutputMode::Clipboard) {
            Err(AppError::Output(
                "Ship Mode requires Type Simulation or Both output mode".into(),
            ))
        } else {
            Ok(())
        }
    }

    /// Best-effort snapshot of supported clipboard contents prior to a paste.
    /// Text and RGBA images are restorable. Returns `None` for unsupported
    /// formats (such as file lists) or read errors, in which case the caller
    /// leaves the router-written text in place rather than clearing data it
    /// cannot faithfully restore.
    fn with_transaction<T>(operation: impl FnOnce() -> AppResult<T>) -> AppResult<T> {
        Self::with_output_transaction(operation)
    }

    /// Serialize a non-router synthetic-input implementation (currently the
    /// Command Mode executor) with every clipboard/paste transaction.
    pub(crate) fn with_output_transaction<T, E>(
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let _guard = OUTPUT_TRANSACTION
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        operation()
    }

    fn capture_clipboard(clipboard: &mut Clipboard) -> Option<ClipboardSnapshot> {
        if let Ok(text) = clipboard.get_text() {
            return Some(ClipboardSnapshot::Text(text));
        }
        clipboard
            .get_image()
            .ok()
            .map(|image| ClipboardSnapshot::Image {
                width: image.width,
                height: image.height,
                bytes: image.bytes.into_owned(),
            })
    }

    fn set_clipboard(&self, text: &str) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;
        Self::set_clipboard_verified(&mut clipboard, text)
    }

    fn set_clipboard_verified(clipboard: &mut Clipboard, text: &str) -> AppResult<()> {
        let start = Instant::now();
        let timeout = Duration::from_millis(CLIPBOARD_VERIFY_TIMEOUT_MS);
        let interval = Duration::from_millis(CLIPBOARD_VERIFY_INTERVAL_MS);
        let mut last_error: Option<String> = None;

        while start.elapsed() <= timeout {
            match clipboard.set_text(text) {
                Ok(()) => {
                    thread::sleep(interval);
                    match clipboard.get_text() {
                        Ok(current) if Self::clipboard_text_matches(&current, text) => {
                            return Ok(());
                        }
                        Ok(current) => {
                            last_error = Some(format!(
                                "clipboard still held different text ({} chars)",
                                current.chars().count()
                            ));
                        }
                        Err(e) => {
                            last_error = Some(format!("clipboard readback failed: {e}"));
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(format!("clipboard write failed: {e}"));
                }
            }

            thread::sleep(interval);
        }

        Err(AppError::Output(format!(
            "Clipboard did not contain the dictation after write; refusing to paste stale clipboard{}",
            last_error
                .map(|e| format!(" ({e})"))
                .unwrap_or_default()
        )))
    }

    fn clipboard_text_matches(actual: &str, expected: &str) -> bool {
        fn normalize(s: &str) -> String {
            s.replace("\r\n", "\n")
        }

        actual == expected || normalize(actual) == normalize(expected)
    }

    fn clipboard_contains_owned_text(clipboard: &mut Clipboard, owned: &str) -> bool {
        clipboard
            .get_text()
            .map(|current| Self::clipboard_text_matches(&current, owned))
            .unwrap_or(false)
    }

    fn restore_clipboard_if_owned(
        clipboard: &mut Clipboard,
        prior: ClipboardSnapshot,
        owned: &str,
    ) -> AppResult<()> {
        if !Self::clipboard_contains_owned_text(clipboard, owned) {
            crate::llm::diaglog::log("router: clipboard restore skipped because ownership changed");
            return Ok(());
        }

        match prior {
            ClipboardSnapshot::Text(text) => Self::set_clipboard_verified(clipboard, &text),
            ClipboardSnapshot::Image {
                width,
                height,
                bytes,
            } => clipboard
                .set_image(ImageData {
                    width,
                    height,
                    bytes: Cow::Owned(bytes),
                })
                .map_err(|e| AppError::Output(format!("Failed to restore clipboard image: {e}"))),
        }
    }

    fn command_transfers_clipboard(cmd: &VoiceCommand) -> bool {
        matches!(cmd, VoiceCommand::Copy | VoiceCommand::Cut)
    }

    fn remaining_paste_guard(elapsed: Option<Duration>) -> Duration {
        let full = Duration::from_millis(POST_PASTE_GUARD_MS);
        elapsed
            .map(|elapsed| full.saturating_sub(elapsed))
            .unwrap_or(full)
    }

    /// Simulates Ctrl+V.
    fn send_paste_keystroke(&self, target_policy: TargetPolicy) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;
        if let TargetPolicy::Verify(target) = target_policy {
            Self::require_foreground_target(target, "paste")?;
        }
        Self::paste_keystroke(&mut enigo)
    }

    /// Modifier keys that silently corrupt an injected Ctrl+V, paired with the
    /// label used in diagnostics.  Ctrl is absent on purpose: Ctrl+V needs it,
    /// and `with_modifier` presses and releases its own.
    #[cfg(target_os = "windows")]
    const CONFLICTING_MODIFIERS: [(u16, &str); 6] = {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
            VK_LMENU, VK_LSHIFT, VK_LWIN, VK_RMENU, VK_RSHIFT, VK_RWIN,
        };
        [
            (VK_LMENU, "LAlt"),
            (VK_RMENU, "RAlt"),
            (VK_LWIN, "LWin"),
            (VK_RWIN, "RWin"),
            (VK_LSHIFT, "LShift"),
            (VK_RSHIFT, "RShift"),
        ]
    };

    /// Which conflicting modifiers are physically down right now.
    ///
    /// A key-down the hotkey hook SWALLOWED never reaches the async key-state
    /// table, so this only ever reports modifiers the rest of the system —
    /// including `SendInput`'s composition and the target app — also believes
    /// are held.  That is exactly the set that can corrupt our paste.
    #[cfg(target_os = "windows")]
    fn held_conflicting_modifiers() -> Vec<&'static str> {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

        Self::CONFLICTING_MODIFIERS
            .iter()
            .filter(|(vk, _)| (unsafe { GetAsyncKeyState(*vk as i32) } as u16) & 0x8000 != 0)
            .map(|(_, label)| *label)
            .collect()
    }

    /// Give a still-held modifier a brief moment to come up before we inject
    /// Ctrl+V, and record what we saw either way.
    ///
    /// WHY THIS EXISTS.  The dictation hotkey is itself a modifier chord
    /// (default LCtrl+LAlt) and the hook only swallows the key-DOWN of the key
    /// that COMPLETED the chord — the first key is deliberately passed through
    /// so a real Ctrl+C / Alt+Tab keeps working.  Releasing EITHER key ends a
    /// hold-mode dictation, so with an Alt-first chord the user can release
    /// Ctrl (firing the stop) while Alt stays down.  `SendInput` composes with
    /// live keyboard state, so the Ctrl+V we inject a second later arrives at
    /// the target as Ctrl+Alt+V, which Chromium/Electron do not treat as a
    /// paste.  Nothing errors: the injection succeeds, the clipboard holds the
    /// dictation, and nothing lands in the composer.
    ///
    /// WHY WE ONLY WAIT, NEVER INJECT A RELEASE.  Synthesizing the key-up is
    /// unsafe here in two separate ways.  (1) In the Alt-first case this
    /// targets, Alt is NOT in the hook's `swallowed_down` set, so a synthetic
    /// Alt-up reaches the app and completes a standalone Alt gesture — popping
    /// the Windows/Electron menu bar, which takes keyboard focus while leaving
    /// the foreground HWND unchanged: the very silent failure we are trying to
    /// fix (`hotkey.rs` already documents being bitten by a leaked Alt-up).
    /// (2) Capture ownership is released before transcription finishes, so a
    /// second dictation can be recording while this one pastes; the hook does
    /// not distinguish injected events, so our synthetic up would clear that
    /// recording's latch and stop it early.
    ///
    /// Waiting costs nothing in the common case (one key-state sweep) and
    /// rescues the realistic case where the user is mid-release.  If the
    /// modifier is still held when the budget expires we paste anyway — never
    /// worse than today's behaviour — and the log says exactly what was held.
    #[cfg(target_os = "windows")]
    fn settle_conflicting_modifiers() {
        const BUDGET: Duration = Duration::from_millis(400);
        const POLL: Duration = Duration::from_millis(20);

        let initial = Self::held_conflicting_modifiers();
        if initial.is_empty() {
            return;
        }

        let start = Instant::now();
        while start.elapsed() < BUDGET {
            thread::sleep(POLL);
            if Self::held_conflicting_modifiers().is_empty() {
                crate::diag::log(&format!(
                    "output: modifier(s) [{}] released after {}ms; pasting clean",
                    initial.join("+"),
                    start.elapsed().as_millis()
                ));
                return;
            }
        }

        // Still held.  Paste regardless (refusing would lose the dictation), but
        // say so — a Ctrl+V that arrives as Ctrl+Alt+V is silently ignored by
        // Chromium/Electron targets and is otherwise indistinguishable from a
        // successful paste.
        crate::diag::log(&format!(
            "output: modifier(s) [{}] STILL HELD at paste — target may ignore Ctrl+V",
            Self::held_conflicting_modifiers().join("+")
        ));
    }

    #[cfg(not(target_os = "windows"))]
    fn settle_conflicting_modifiers() {}

    fn paste_keystroke(enigo: &mut Enigo) -> AppResult<()> {
        Self::settle_conflicting_modifiers();
        Self::with_modifier(enigo, PASTE_MODIFIER, |enigo| {
            enigo
                .key(Key::Unicode('v'), Direction::Click)
                .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))
        })
    }

    /// Send Shift+Enter (line break that works in chat apps too).
    fn shift_enter(enigo: &mut Enigo) -> AppResult<()> {
        Self::with_modifier(enigo, Key::Shift, |enigo| {
            enigo
                .key(Key::Return, Direction::Click)
                .map_err(|e| AppError::Output(format!("Newline failed: {e}")))
        })
    }

    /// Press `modifier`, run `body` (which clicks the actual key), then ALWAYS
    /// release `modifier` — even if `body` fails.
    ///
    /// A modifier left pressed mid-sequence leaks into Windows' system-wide key
    /// state via SendInput, which is what made Ctrl appear "stuck down" across
    /// the whole OS after a paste. The release must run on every path, so it is
    /// deliberately NOT propagated through `?`.
    fn with_modifier<F>(enigo: &mut Enigo, modifier: Key, body: F) -> AppResult<()>
    where
        F: FnOnce(&mut Enigo) -> AppResult<()>,
    {
        enigo
            .key(modifier, Direction::Press)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;
        let result = body(enigo);
        // Always release, even if `body` errored — never leave a modifier down.
        let _ = enigo.key(modifier, Direction::Release);
        result
    }

    /// Multi-modifier variant of [`with_modifier`]: press each modifier in
    /// order, run `body`, then ALWAYS release them in reverse order — even if a
    /// press or `body` fails partway. Guarantees no modifier is ever left down.
    fn with_modifiers<F>(enigo: &mut Enigo, modifiers: &[Key], body: F) -> AppResult<()>
    where
        F: FnOnce(&mut Enigo) -> AppResult<()>,
    {
        let mut pressed: Vec<Key> = Vec::with_capacity(modifiers.len());
        let mut press_result = Ok(());
        for m in modifiers {
            match enigo.key(*m, Direction::Press) {
                Ok(()) => pressed.push(*m),
                Err(e) => {
                    press_result = Err(AppError::Output(format!("Keystroke failed: {e}")));
                    break;
                }
            }
        }
        // Only run the body if every modifier went down cleanly.
        let result = if press_result.is_ok() {
            body(enigo)
        } else {
            press_result
        };
        // Release whatever we actually pressed, in reverse order, on every path.
        for m in pressed.iter().rev() {
            let _ = enigo.key(*m, Direction::Release);
        }
        result
    }

    /// Identity gate for a consequential inline command (a submit, a click, an
    /// arbitrary chord): the bound dictation target must still be the live
    /// foreground window.  A `None` target can't be proven, so it is REFUSED —
    /// a consequential action never fires blind (B2-3).  Logs the skip so the
    /// diagnostics show why nothing happened.  Called immediately before each
    /// side-effecting primitive (for Send, AFTER its paste guard) so a focus
    /// change in the intervening window can't redirect the input.
    fn foreground_is_target(target: Option<crate::focus::WindowTarget>) -> bool {
        match target {
            Some(t) if crate::focus::verify_foreground_target(t.hwnd, t.pid) => true,
            _ => {
                crate::llm::diaglog::log(
                    "router: inline command skipped — no verified target window in focus",
                );
                false
            }
        }
    }

    fn require_foreground_target(
        target: Option<crate::focus::WindowTarget>,
        primitive: &str,
    ) -> AppResult<()> {
        if Self::foreground_is_target(target) {
            Ok(())
        } else {
            Err(AppError::Output(format!(
                "Target window is not in focus; refusing to {primitive}"
            )))
        }
    }

    /// Whether a command fires OS input at (and so depends on) the foreground
    /// window, and must therefore only run when the bound dictation target is
    /// still that foreground window (B2-12).
    ///
    /// Every keystroke/pointer/edit that lands in the focused control is
    /// focus-dependent: line breaks, the submit/Enter, pointer clicks + scrolls,
    /// arbitrary key combos, AND the Ctrl-chord edits (select-all, copy, cut,
    /// undo, redo, delete-word) plus bare Tab/Escape — a mishearing must not
    /// fire any of these into whatever window grabbed focus. Exempt: list
    /// markers (resolved to text earlier, no keystroke), and `LaunchApp` (spawns
    /// a process, not a focus-dependent keystroke — gated separately by
    /// `allow_launch`).
    fn command_needs_foreground(cmd: &VoiceCommand) -> bool {
        match cmd {
            VoiceCommand::LaunchApp(_)
            | VoiceCommand::BulletItem
            | VoiceCommand::NumberedItem
            | VoiceCommand::EndList => false,
            VoiceCommand::NewLine
            | VoiceCommand::NewParagraph
            | VoiceCommand::DeleteLastWord
            | VoiceCommand::Send
            | VoiceCommand::SelectAll
            | VoiceCommand::Copy
            | VoiceCommand::Cut
            | VoiceCommand::Undo
            | VoiceCommand::Redo
            | VoiceCommand::PressTab
            | VoiceCommand::PressEscape
            | VoiceCommand::PressEnter
            | VoiceCommand::KeyCombo { .. }
            | VoiceCommand::MouseClick
            | VoiceCommand::MouseRightClick
            | VoiceCommand::MouseDoubleClick
            | VoiceCommand::ScrollUp
            | VoiceCommand::ScrollDown => true,
        }
    }

    /// Execute a single voice command as OS-level input. Every modifier-bearing
    /// action routes through [`with_modifier`]/[`with_modifiers`] so a failure
    /// mid-chord can never leak a stuck Ctrl/Shift/Alt into the OS.
    ///
    /// `target` is the window the dictation was aimed at: a consequential command
    /// (Send/Enter, mouse, key combo) re-verifies via [`foreground_is_target`]
    /// immediately before each primitive and is refused when the target is absent
    /// or no longer foreground, so a mishearing can't fire into an unverified
    /// window (B2-3).  `allow_launch` gates the disruptive `LaunchApp` (default
    /// governed by the `launch_app_voice_commands_enabled` setting).
    fn run_command(
        enigo: &mut Enigo,
        cmd: &VoiceCommand,
        target: Option<crate::focus::WindowTarget>,
        allow_launch: bool,
        last_paste_at: Option<Instant>,
    ) -> AppResult<()> {
        // Identity gate for EVERY focus-dependent primitive (B2-12): the bound
        // dictation target must still be the live foreground before we fire OS
        // input at it.  `Send` re-checks AGAIN after its own paste-guard sleep
        // (below), since that 250 ms is an extra window for focus to change.
        // A `None`/mismatched target is refused (logged) — never fire blind.
        if Self::command_needs_foreground(cmd) {
            Self::require_foreground_target(target, "execute inline command")?;
        }
        match cmd {
            VoiceCommand::NewLine => {
                Self::shift_enter(enigo)?;
            }
            VoiceCommand::NewParagraph => {
                Self::shift_enter(enigo)?;
                Self::require_foreground_target(target, "insert paragraph break")?;
                Self::shift_enter(enigo)?;
            }
            VoiceCommand::DeleteLastWord => {
                Self::with_modifier(enigo, DELETE_WORD_MODIFIER, |enigo| {
                    enigo
                        .key(Key::Backspace, Direction::Click)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))
                })?;
            }
            VoiceCommand::Send => {
                // Wait only for the unelapsed portion of the paste guard. Text
                // segments already paid this interval, so trailing Send no
                // longer adds a redundant fixed 250 ms.
                let elapsed = last_paste_at.map(|at| at.elapsed());
                thread::sleep(Self::remaining_paste_guard(elapsed));
                // Re-verify AFTER the guard — the 250 ms sleep is a window in
                // which focus could change; the Enter must only fire while the
                // bound target is still foreground (B2-3).
                Self::require_foreground_target(target, "send")?;
                enigo
                    .key(Key::Return, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Send (Enter) failed: {e}")))?;
            }
            VoiceCommand::SelectAll => {
                Self::with_modifier(enigo, PASTE_MODIFIER, |enigo| {
                    enigo
                        .key(Key::Unicode('a'), Direction::Click)
                        .map_err(|e| AppError::Output(format!("Select all failed: {e}")))
                })?;
            }
            VoiceCommand::Copy => {
                Self::with_modifier(enigo, PASTE_MODIFIER, |enigo| {
                    enigo
                        .key(Key::Unicode('c'), Direction::Click)
                        .map_err(|e| AppError::Output(format!("Copy failed: {e}")))
                })?;
            }
            VoiceCommand::Cut => {
                Self::with_modifier(enigo, PASTE_MODIFIER, |enigo| {
                    enigo
                        .key(Key::Unicode('x'), Direction::Click)
                        .map_err(|e| AppError::Output(format!("Cut failed: {e}")))
                })?;
            }
            VoiceCommand::Undo => {
                Self::with_modifier(enigo, PASTE_MODIFIER, |enigo| {
                    enigo
                        .key(Key::Unicode('z'), Direction::Click)
                        .map_err(|e| AppError::Output(format!("Undo failed: {e}")))
                })?;
            }
            VoiceCommand::Redo => {
                Self::with_modifiers(enigo, &[PASTE_MODIFIER, Key::Shift], |enigo| {
                    enigo
                        .key(Key::Unicode('z'), Direction::Click)
                        .map_err(|e| AppError::Output(format!("Redo failed: {e}")))
                })?;
            }
            VoiceCommand::PressTab => {
                enigo
                    .key(Key::Tab, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Tab failed: {e}")))?;
            }
            VoiceCommand::PressEscape => {
                enigo
                    .key(Key::Escape, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Escape failed: {e}")))?;
            }
            VoiceCommand::PressEnter => {
                enigo
                    .key(Key::Return, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Enter failed: {e}")))?;
            }
            VoiceCommand::KeyCombo { modifiers, key } => {
                Self::run_key_combo(enigo, modifiers, key)?;
            }
            VoiceCommand::MouseClick => {
                enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Mouse click failed: {e}")))?;
            }
            VoiceCommand::MouseRightClick => {
                enigo
                    .button(Button::Right, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Mouse right click failed: {e}")))?;
            }
            VoiceCommand::MouseDoubleClick => {
                // enigo has no native double-click: two quick Left clicks are
                // interpreted as a double-click by the OS.
                enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Mouse double click failed: {e}")))?;
                Self::require_foreground_target(target, "double-click")?;
                enigo
                    .button(Button::Left, Direction::Click)
                    .map_err(|e| AppError::Output(format!("Mouse double click failed: {e}")))?;
            }
            VoiceCommand::ScrollUp => {
                // enigo 0.2: with Axis::Vertical a negative length scrolls UP.
                enigo
                    .scroll(-1, Axis::Vertical)
                    .map_err(|e| AppError::Output(format!("Scroll up failed: {e}")))?;
            }
            VoiceCommand::ScrollDown => {
                enigo
                    .scroll(1, Axis::Vertical)
                    .map_err(|e| AppError::Output(format!("Scroll down failed: {e}")))?;
            }
            VoiceCommand::LaunchApp(command_line) => {
                // Gated on the `launch_app_voice_commands_enabled` setting
                // (default ON): when the user has turned it off, a misheard
                // custom "launch …" command must not spawn a process.
                if allow_launch {
                    Self::launch_app(command_line);
                } else {
                    crate::llm::diaglog::log("router: inline LaunchApp skipped — setting disabled");
                }
            }
            // List markers are resolved into literal text segments by
            // `resolve_list_segments` before segments reach the router; an
            // unresolved one has nothing to execute.
            VoiceCommand::BulletItem | VoiceCommand::NumberedItem | VoiceCommand::EndList => {}
        }
        Ok(())
    }

    /// Execute a user-defined key combination: press each modifier, click the
    /// main key, then release the modifiers in reverse order — always, via
    /// [`with_modifiers`]. `Ctrl` maps literally to `Key::Control` (unlike the
    /// built-in clipboard commands which use the platform `PASTE_MODIFIER`).
    fn run_key_combo(
        enigo: &mut Enigo,
        modifiers: &[KeyModifier],
        key: &ComboKey,
    ) -> AppResult<()> {
        let mod_keys: Vec<Key> = modifiers
            .iter()
            .map(|m| match m {
                KeyModifier::Ctrl => Key::Control,
                KeyModifier::Alt => Key::Alt,
                KeyModifier::Shift => Key::Shift,
                KeyModifier::Meta => Key::Meta,
            })
            .collect();
        let main = match key {
            ComboKey::Char(c) => Key::Unicode(*c),
            ComboKey::Tab => Key::Tab,
            ComboKey::Escape => Key::Escape,
            ComboKey::Enter => Key::Return,
            ComboKey::Space => Key::Space,
            ComboKey::Backspace => Key::Backspace,
        };
        Self::with_modifiers(enigo, &mod_keys, |enigo| {
            enigo
                .key(main, Direction::Click)
                .map_err(|e| AppError::Output(format!("Key combo failed: {e}")))
        })
    }

    /// Launch a program directly (program + args, no shell). The command line
    /// is tokenized respecting double quotes, so there is no shell
    /// interpretation and no injection surface. The child is spawned detached;
    /// on Windows it gets `CREATE_NO_WINDOW | DETACHED_PROCESS` so no console
    /// flashes. A spawn failure is logged and swallowed — a bad launch must
    /// never break the dictation output pipeline.
    fn launch_app(command_line: &str) {
        use crate::postprocess::voice_commands::tokenize_command_line;

        let tokens = tokenize_command_line(command_line);
        let (program, args) = match tokens.split_first() {
            Some(parts) => parts,
            None => {
                crate::llm::diaglog::log("LaunchApp: empty command line, nothing to run");
                return;
            }
        };

        let mut cmd = std::process::Command::new(program);
        cmd.args(args);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW (0x0800_0000) | DETACHED_PROCESS (0x0000_0008)
            cmd.creation_flags(0x0800_0000 | 0x0000_0008);
        }

        match cmd.spawn() {
            Ok(_child) => {} // detached: drop the handle, do not wait
            Err(e) => {
                crate::llm::diaglog::log(&format!("LaunchApp: failed to spawn '{program}': {e}"));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputRouter;
    use crate::output::types::OutputConfig;
    use crate::postprocess::voice_commands::{ComboKey, KeyModifier, VoiceCommand};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;
    use std::time::Duration;

    /// Every focus-dependent inline primitive must be gated on the bound target
    /// (B2-12): the Ctrl-chord edits + bare Tab/Escape used to fire unverified.
    #[test]
    fn all_focus_dependent_primitives_require_foreground() {
        let gated = [
            VoiceCommand::NewLine,
            VoiceCommand::NewParagraph,
            VoiceCommand::DeleteLastWord,
            VoiceCommand::Send,
            VoiceCommand::SelectAll,
            VoiceCommand::Copy,
            VoiceCommand::Cut,
            VoiceCommand::Undo,
            VoiceCommand::Redo,
            VoiceCommand::PressTab,
            VoiceCommand::PressEscape,
            VoiceCommand::PressEnter,
            VoiceCommand::KeyCombo {
                modifiers: vec![KeyModifier::Ctrl],
                key: ComboKey::Char('k'),
            },
            VoiceCommand::MouseClick,
            VoiceCommand::MouseRightClick,
            VoiceCommand::MouseDoubleClick,
            VoiceCommand::ScrollUp,
            VoiceCommand::ScrollDown,
        ];
        for cmd in &gated {
            assert!(
                OutputRouter::command_needs_foreground(cmd),
                "{cmd:?} must be identity-verified before firing"
            );
        }
    }

    /// Process launches and list-marker placeholders do not send input to the
    /// currently focused control, so they are exempt from target verification.
    #[test]
    fn launch_and_list_marker_commands_are_exempt() {
        let exempt = [
            VoiceCommand::LaunchApp("notepad".into()),
            VoiceCommand::BulletItem,
            VoiceCommand::NumberedItem,
            VoiceCommand::EndList,
        ];
        for cmd in &exempt {
            assert!(
                !OutputRouter::command_needs_foreground(cmd),
                "{cmd:?} should not require foreground verification"
            );
        }
    }

    /// The gate refuses an absent target — a consequential inline command never
    /// fires blind (B2-3/B2-12).
    #[test]
    fn absent_target_is_refused() {
        assert!(!OutputRouter::foreground_is_target(None));
    }

    #[test]
    fn trailing_send_only_waits_for_unelapsed_paste_guard() {
        assert_eq!(
            OutputRouter::remaining_paste_guard(None),
            Duration::from_millis(250)
        );
        assert_eq!(
            OutputRouter::remaining_paste_guard(Some(Duration::from_millis(100))),
            Duration::from_millis(150)
        );
        assert_eq!(
            OutputRouter::remaining_paste_guard(Some(Duration::from_millis(250))),
            Duration::ZERO
        );
        assert_eq!(
            OutputRouter::remaining_paste_guard(Some(Duration::from_secs(1))),
            Duration::ZERO
        );
    }

    #[test]
    fn copy_and_cut_transfer_clipboard_ownership_to_the_target() {
        assert!(OutputRouter::command_transfers_clipboard(
            &VoiceCommand::Copy
        ));
        assert!(OutputRouter::command_transfers_clipboard(
            &VoiceCommand::Cut
        ));
        assert!(!OutputRouter::command_transfers_clipboard(
            &VoiceCommand::SelectAll
        ));
    }

    #[test]
    fn clipboard_ownership_matching_accepts_only_router_text() {
        assert!(OutputRouter::clipboard_text_matches(
            "router text\r\nnext",
            "router text\nnext"
        ));
        assert!(!OutputRouter::clipboard_text_matches(
            "new copy from the user",
            "router text"
        ));
    }

    #[test]
    fn output_transactions_are_process_wide_and_serialized() {
        let start = Arc::new(Barrier::new(3));
        let in_transaction = Arc::new(AtomicBool::new(false));
        let mut workers = Vec::new();

        for _ in 0..2 {
            let start = Arc::clone(&start);
            let in_transaction = Arc::clone(&in_transaction);
            workers.push(thread::spawn(move || {
                start.wait();
                OutputRouter::with_transaction(|| {
                    assert!(
                        !in_transaction.swap(true, Ordering::SeqCst),
                        "two output transactions overlapped"
                    );
                    thread::sleep(Duration::from_millis(20));
                    in_transaction.store(false, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
            }));
        }

        start.wait();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(!in_transaction.load(Ordering::SeqCst));
    }

    #[test]
    fn queued_type_text_cancellation_happens_before_clipboard_access() {
        let result = OutputRouter::new().paste_text_to_target_with_submit(
            "must not be written",
            true,
            crate::focus::WindowTarget { hwnd: 0, pid: None },
            true,
            || true,
        );
        assert!(matches!(
            result,
            Err(crate::error::AppError::Output(message)) if message == "stopped"
        ));
    }

    #[test]
    fn ship_mode_rejects_clipboard_only_output_before_os_access() {
        let result = OutputRouter::new().send_to_target_with_submit(
            "must not be written",
            &OutputConfig::default(),
            crate::focus::WindowTarget { hwnd: 0, pid: None },
            Duration::ZERO,
        );
        assert!(matches!(
            result,
            Err(crate::error::AppError::Output(message))
                if message == "Ship Mode requires Type Simulation or Both output mode"
        ));
    }
}
