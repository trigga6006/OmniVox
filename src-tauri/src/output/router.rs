use std::thread;
use std::time::Duration;

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

use crate::error::{AppError, AppResult};
use crate::output::types::{OutputConfig, OutputMode};
use crate::postprocess::voice_commands::{OutputSegment, VoiceCommand, segments_to_string};

/// Routes transcribed text to the user's focused application.
///
/// Supports three output modes:
/// - **Clipboard**: Copies text to the system clipboard. User pastes manually.
/// - **TypeSimulation**: Pastes text via a temporary clipboard write + Ctrl+V,
///   then restores the previous clipboard contents. Reliable for any text length.
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
            OutputMode::TypeSimulation => {
                self.paste_with_restore(text)?;
            }
            OutputMode::Both => {
                // Clipboard gets our text permanently; also paste into focused app.
                // No restore needed since the user wants the text on clipboard.
                self.set_clipboard(text)?;
                thread::sleep(Duration::from_millis(30));
                self.send_paste_keystroke()?;
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
            OutputMode::TypeSimulation => {
                self.execute_segments(segments)?;
            }
            OutputMode::Both => {
                let text = segments_to_string(segments);
                if !text.is_empty() {
                    self.set_clipboard(&text)?;
                }
                thread::sleep(Duration::from_millis(30));
                self.execute_segments(segments)?;
            }
        }

        Ok(())
    }

    /// Execute a sequence of text + command segments via paste + keystrokes.
    fn execute_segments(&self, segments: &[OutputSegment]) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        // Snapshot clipboard for restoration after we're done
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;
        let previous = clipboard.get_text().ok();

        for seg in segments {
            match seg {
                OutputSegment::Text(s) => {
                    if !s.is_empty() {
                        // Handle newlines: split into lines, paste each, Shift+Enter between
                        let lines: Vec<&str> = s.split('\n').collect();
                        for (i, line) in lines.iter().enumerate() {
                            if !line.is_empty() {
                                clipboard
                                    .set_text(*line)
                                    .map_err(|e| AppError::Output(format!("Clipboard failed: {e}")))?;
                                thread::sleep(Duration::from_millis(20));
                                Self::paste_keystroke(&mut enigo)?;
                                thread::sleep(Duration::from_millis(30));
                            }
                            if i < lines.len() - 1 {
                                Self::shift_enter(&mut enigo)?;
                            }
                        }
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
            }
        }

        // Restore previous clipboard contents
        if let Some(prev) = previous {
            let _ = clipboard.set_text(&prev);
        }

        Ok(())
    }

    /// Paste text into the focused app, then restore the user's clipboard.
    fn paste_with_restore(&self, text: &str) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;

        // Snapshot — clipboard may contain non-text (images, etc.), so failing is OK
        let previous = clipboard.get_text().ok();

        clipboard
            .set_text(text)
            .map_err(|e| AppError::Output(format!("Failed to set clipboard: {e}")))?;

        // Allow the clipboard to settle before sending the keystroke
        thread::sleep(Duration::from_millis(30));

        self.send_paste_keystroke()?;

        // Give the target application time to process the paste event
        thread::sleep(Duration::from_millis(50));

        // Restore original clipboard contents
        if let Some(prev) = previous {
            let _ = clipboard.set_text(&prev);
        }

        Ok(())
    }

    fn set_clipboard(&self, text: &str) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;
        clipboard
            .set_text(text)
            .map_err(|e| AppError::Output(format!("Failed to set clipboard: {e}")))?;
        Ok(())
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
