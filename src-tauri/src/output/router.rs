use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::error::{AppError, AppResult};
use crate::output::types::{OutputConfig, OutputMode};
use crate::postprocess::voice_commands::{OutputSegment, VoiceCommand, segments_to_string};

/// Routes transcribed text to the user's focused application.
///
/// Supports three output modes:
/// - **Clipboard**: Copies text to the system clipboard. User pastes manually.
/// - **TypeSimulation**: Types text directly into the focused app via simulated
///   Unicode keystrokes.  Inserts at the cursor without touching the clipboard,
///   so existing text in the input field is never erased.
/// - **Both**: Sets clipboard (user keeps it) and types into the focused app.
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
                // Type directly via Unicode keystrokes — inserts at cursor
                // without triggering app-specific paste handlers that can
                // replace entire input content.  Leaves clipboard untouched.
                self.type_text(text)?;
            }
            OutputMode::Both => {
                // Clipboard gets our text permanently; also type into focused app
                self.set_clipboard(text)?;
                thread::sleep(Duration::from_millis(30));
                self.type_text(text)?;
            }
        }

        Ok(())
    }

    /// Send a sequence of text segments and voice commands to the focused app.
    ///
    /// In **TypeSimulation** mode, text is typed and commands execute keystrokes.
    /// In **Clipboard** mode, segments are collapsed to a string (commands become
    /// `\n` characters; `DeleteLastWord` is dropped).
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
                self.type_segments(segments)?;
            }
            OutputMode::Both => {
                let text = segments_to_string(segments);
                if !text.is_empty() {
                    self.set_clipboard(&text)?;
                }
                thread::sleep(Duration::from_millis(30));
                self.type_segments(segments)?;
            }
        }

        Ok(())
    }

    /// Execute a sequence of text + command segments via keystroke simulation.
    fn type_segments(&self, segments: &[OutputSegment]) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        for seg in segments {
            match seg {
                OutputSegment::Text(s) => {
                    if !s.is_empty() {
                        // Handle embedded newlines the same way type_text does.
                        let lines: Vec<&str> = s.split('\n').collect();
                        for (i, line) in lines.iter().enumerate() {
                            if !line.is_empty() {
                                Self::type_chunked(&mut enigo, line)?;
                            }
                            if i < lines.len() - 1 {
                                self.send_shift_enter(&mut enigo)?;
                            }
                        }
                    }
                }
                OutputSegment::Command(VoiceCommand::NewLine) => {
                    self.send_shift_enter(&mut enigo)?;
                }
                OutputSegment::Command(VoiceCommand::NewParagraph) => {
                    self.send_shift_enter(&mut enigo)?;
                    self.send_shift_enter(&mut enigo)?;
                }
                OutputSegment::Command(VoiceCommand::DeleteLastWord) => {
                    // Ctrl+Backspace deletes the previous word.
                    enigo
                        .key(Key::Control, Direction::Press)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))?;
                    enigo
                        .key(Key::Backspace, Direction::Click)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))?;
                    enigo
                        .key(Key::Control, Direction::Release)
                        .map_err(|e| AppError::Output(format!("Delete word failed: {e}")))?;
                }
            }
        }

        Ok(())
    }

    /// Send Shift+Enter (line break that works in chat apps too).
    fn send_shift_enter(&self, enigo: &mut Enigo) -> AppResult<()> {
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

    fn set_clipboard(&self, text: &str) -> AppResult<()> {
        let mut clipboard = Clipboard::new()
            .map_err(|e| AppError::Output(format!("Failed to access clipboard: {e}")))?;
        clipboard
            .set_text(text)
            .map_err(|e| AppError::Output(format!("Failed to set clipboard: {e}")))?;
        Ok(())
    }

    /// Type text into the focused application using simulated Unicode keystrokes.
    ///
    /// Unlike clipboard + Ctrl+V, this inserts at the cursor position and never
    /// triggers app-specific paste handlers that might replace existing content.
    ///
    /// Newlines are sent as `Shift+Enter` keypresses (rather than raw `\n`
    /// codepoints which Windows apps ignore).  `Shift+Enter` inserts a line
    /// break in virtually all apps — including chat inputs that treat bare
    /// `Enter` as "send message".
    ///
    /// Text is sent in word-sized chunks with small inter-chunk delays to prevent
    /// the target app's message queue from backing up and triggering Windows
    /// keyboard auto-repeat on the last character.
    fn type_text(&self, text: &str) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        let lines: Vec<&str> = text.split('\n').collect();
        for (i, line) in lines.iter().enumerate() {
            if !line.is_empty() {
                Self::type_chunked(&mut enigo, line)?;
            }
            if i < lines.len() - 1 {
                self.send_shift_enter(&mut enigo)?;
            }
        }
        Ok(())
    }

    /// Send text in small chunks with brief pauses so the target app's message
    /// queue can drain between bursts.  Prevents `SendInput` from flooding the
    /// queue and triggering keyboard auto-repeat on the last character.
    ///
    /// Chunks on word boundaries (space characters) so we never split a
    /// multi-byte UTF-8 character.  A 5 ms pause between words is imperceptible
    /// to the user but gives the target app enough time to process each batch.
    fn type_chunked(enigo: &mut Enigo, text: &str) -> AppResult<()> {
        const CHUNK_DELAY: Duration = Duration::from_millis(5);

        let mut remaining = text;
        while !remaining.is_empty() {
            // Find the next space after at least one character.
            let chunk_end = remaining[1..]
                .find(' ')
                .map(|pos| pos + 2) // +1 for the offset, +1 to include the space
                .unwrap_or(remaining.len());

            let chunk = &remaining[..chunk_end];
            enigo
                .text(chunk)
                .map_err(|e| AppError::Output(format!("Text input failed: {e}")))?;

            remaining = &remaining[chunk_end..];

            if !remaining.is_empty() {
                thread::sleep(CHUNK_DELAY);
            }
        }

        Ok(())
    }

}
