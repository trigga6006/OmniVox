use std::thread;
use std::time::{Duration, Instant};

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

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
use crate::postprocess::voice_commands::{segments_to_string, OutputSegment, VoiceCommand};

/// Routes transcribed text to the user's focused application.
///
/// Supports three output modes:
/// - **Clipboard**: Copies text to the system clipboard. User pastes manually.
/// - **TypeSimulation**: Pastes text via clipboard + Ctrl+V and leaves the
///   transcription on the clipboard so deferred paste handlers cannot read
///   stale user clipboard contents.
/// - **Both**: Sets clipboard permanently and also pastes into the focused app.
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
            OutputMode::TypeSimulation | OutputMode::Both => {
                self.paste_text(text)?;
            }
        }

        Ok(())
    }

    /// Send a sequence of text segments and voice commands to the focused app.
    ///
    /// In **TypeSimulation** mode, text segments are pasted and commands execute
    /// keystrokes. In **Clipboard** mode, segments are collapsed to a string.
    /// In **Both** mode, clipboard gets the string and keystrokes execute.
    pub fn send_segments(&self, segments: &[OutputSegment], config: &OutputConfig) -> AppResult<()> {
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
            OutputMode::TypeSimulation | OutputMode::Both => {
                self.execute_segments(segments)?;
            }
        }

        Ok(())
    }

    /// Execute a sequence of text + command segments via paste + keystrokes.
    fn execute_segments(&self, segments: &[OutputSegment]) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

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
                OutputSegment::Command(VoiceCommand::NewLine) => {
                    Self::shift_enter(&mut enigo)?;
                }
                OutputSegment::Command(VoiceCommand::NewParagraph) => {
                    Self::shift_enter(&mut enigo)?;
                    Self::shift_enter(&mut enigo)?;
                }
                OutputSegment::Command(VoiceCommand::DeleteLastWord) => {
                    enigo
                        .key(DELETE_WORD_MODIFIER, Direction::Press)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))?;
                    enigo
                        .key(Key::Backspace, Direction::Click)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))?;
                    enigo
                        .key(DELETE_WORD_MODIFIER, Direction::Release)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))?;
                }
                OutputSegment::Command(VoiceCommand::Send) => {
                    thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));
                    enigo
                        .key(Key::Return, Direction::Click)
                        .map_err(|e| AppError::Output(format!("Send (Enter) failed: {e}")))?;
                }
            }
        }

        // Leave the complete paste-ready transcription on the clipboard. Some
        // target apps read clipboard contents after the Ctrl+V key event
        // returns, so restoring the user's previous clipboard can leak stale
        // text into the selected input.
        let final_text = segments_to_string(segments);
        if !final_text.is_empty() {
            Self::set_clipboard_verified(&mut clipboard, &final_text)?;
        }

        Ok(())
    }

    /// Paste text into the focused app and leave that text on the clipboard.
    fn paste_text(&self, text: &str) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

        // Refuse to press Ctrl+V unless the clipboard readback matches this
        // dictation. Otherwise a transient clipboard race can paste the user's
        // previous clipboard into the target app.
        Self::set_clipboard_verified(&mut clipboard, text)?;

        self.send_paste_keystroke()?;

        // Keep the clipboard stable long enough for target apps that read it
        // on a deferred tick after Ctrl+V. The text remains afterward by design.
        thread::sleep(Duration::from_millis(POST_PASTE_GUARD_MS));

        Ok(())
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
        enigo
            .key(PASTE_MODIFIER, Direction::Press)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;
        enigo
            .key(PASTE_MODIFIER, Direction::Release)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;
        Ok(())
    }

    /// Send Shift+Enter (line break that works in chat apps too).
    fn shift_enter(enigo: &mut Enigo) -> AppResult<()> {
        enigo
            .key(Key::Shift, Direction::Press)
            .map_err(|e| AppError::Output(format!("Newline failed: {e}")))?;
        enigo
            .key(Key::Return, Direction::Click)
            .map_err(|e| AppError::Output(format!("Newline failed: {e}")))?;
        enigo
            .key(Key::Shift, Direction::Release)
            .map_err(|e| AppError::Output(format!("Newline failed: {e}")))?;
        Ok(())
    }
}
