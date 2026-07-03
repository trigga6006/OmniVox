use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
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
const POST_PASTE_GUARD_MS: u64 = 250;

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

impl OutputRouter {
    pub fn new() -> Self {
        Self
    }

    pub fn send(&self, text: &str, config: &OutputConfig) -> AppResult<()> {
        if text.is_empty() {
            return Ok(());
        }

        match config.mode {
            OutputMode::Clipboard => {
                self.set_clipboard(text)?;
            }
            OutputMode::TypeSimulation => {
                self.paste_text(text, true)?;
            }
            OutputMode::Both => {
                self.paste_text(text, false)?;
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
    pub fn send_segments(
        &self,
        segments: &[OutputSegment],
        config: &OutputConfig,
    ) -> AppResult<()> {
        if segments.is_empty() {
            return Ok(());
        }

        match config.mode {
            OutputMode::Clipboard => {
                let text = segments_to_string(segments);
                if !text.is_empty() {
                    self.set_clipboard(&text)?;
                }
            }
            OutputMode::TypeSimulation => {
                self.execute_segments(segments, true)?;
            }
            OutputMode::Both => {
                self.execute_segments(segments, false)?;
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
    ) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

        // Snapshot the user's clipboard BEFORE we overwrite it. Only used in
        // TypeSimulation mode; captured eagerly so it's still the *prior*
        // contents (not a half-written dictation) by the time we restore.
        let saved_clipboard = if restore_prior_clipboard {
            Self::capture_clipboard_text(&mut clipboard)
        } else {
            None
        };

        for seg in segments {
            match seg {
                OutputSegment::Text(s) => {
                    if !s.is_empty() {
                        // Paste the entire text block at once, including
                        // newlines. This keeps terminal/editor paste handling
                        // atomic and avoids per-line command execution.
                        Self::set_clipboard_verified(&mut clipboard, s)?;
                        Self::paste_keystroke(&mut enigo)?;
                        thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));
                    }
                }
                OutputSegment::Command(cmd) => {
                    Self::run_command(&mut enigo, cmd)?;
                }
            }
        }

        if restore_prior_clipboard {
            // TypeSimulation: return the clipboard to whatever the user had
            // before dictating. If we couldn't read text (image, empty, error)
            // we leave the last-pasted segment in place — safer than clearing
            // their (possibly non-text) clipboard.
            if let Some(prior) = saved_clipboard {
                let _ = Self::set_clipboard_verified(&mut clipboard, &prior);
            }
        } else {
            // Both: write the full concatenated dictation so the user can
            // re-paste it. Also defends deferred-read apps from seeing only
            // the trailing segment.
            let final_text = segments_to_string(segments);
            if !final_text.is_empty() {
                Self::set_clipboard_verified(&mut clipboard, &final_text)?;
            }
        }

        Ok(())
    }

    /// Paste text into the focused app via Ctrl+V.
    ///
    /// When `restore_prior_clipboard` is true (TypeSimulation), the user's
    /// prior clipboard text is captured first and restored after the
    /// deferred-read guard so pre-copied snippets survive dictation.
    /// `pub(crate)` so Command Mode's `type_text` reuses the exact same
    /// clipboard-verified paste instead of growing a second injection path.
    pub(crate) fn paste_text(&self, text: &str, restore_prior_clipboard: bool) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

        let saved_clipboard = if restore_prior_clipboard {
            Self::capture_clipboard_text(&mut clipboard)
        } else {
            None
        };

        // Refuse to press Ctrl+V unless the clipboard readback matches this
        // dictation. Otherwise a transient clipboard race can paste the user's
        // previous clipboard into the target app.
        Self::set_clipboard_verified(&mut clipboard, text)?;

        self.send_paste_keystroke()?;

        // Keep the clipboard stable long enough for target apps that read it
        // on a deferred tick after Ctrl+V.
        thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));

        if restore_prior_clipboard {
            if let Some(prior) = saved_clipboard {
                let _ = Self::set_clipboard_verified(&mut clipboard, &prior);
            }
        }

        Ok(())
    }

    /// Best-effort snapshot of the clipboard's text contents prior to a paste.
    /// Returns None for non-text clipboards (images, files, errors) — in that
    /// case the caller should leave the post-paste clipboard alone rather than
    /// clearing whatever non-text content was there.
    fn capture_clipboard_text(clipboard: &mut Clipboard) -> Option<String> {
        clipboard.get_text().ok()
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

    /// Simulates Ctrl+V.
    fn send_paste_keystroke(&self) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;
        Self::paste_keystroke(&mut enigo)
    }

    fn paste_keystroke(enigo: &mut Enigo) -> AppResult<()> {
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

    /// Execute a single voice command as OS-level input. Every modifier-bearing
    /// action routes through [`with_modifier`]/[`with_modifiers`] so a failure
    /// mid-chord can never leak a stuck Ctrl/Shift/Alt into the OS.
    fn run_command(enigo: &mut Enigo, cmd: &VoiceCommand) -> AppResult<()> {
        match cmd {
            VoiceCommand::NewLine => {
                Self::shift_enter(enigo)?;
            }
            VoiceCommand::NewParagraph => {
                Self::shift_enter(enigo)?;
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
                // Keep the paste-guard so trailing "send" doesn't fire before the
                // dictation text has landed in the target app.
                thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));
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
                Self::launch_app(command_line);
            }
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
