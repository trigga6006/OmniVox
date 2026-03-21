use std::thread;
use std::time::Duration;

use arboard::Clipboard;
use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::error::{AppError, AppResult};
use crate::output::types::{OutputConfig, OutputMode};

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
    /// All characters are batched into a single `SendInput` call for speed.
    fn type_text(&self, text: &str) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;
        enigo
            .text(text)
            .map_err(|e| AppError::Output(format!("Text input failed: {e}")))?;
        Ok(())
    }

    /// Paste into the focused app while preserving the user's clipboard.
    /// (Legacy — kept as fallback; prefer `type_text` for insertion.)
    #[allow(dead_code)]
    fn paste_with_restore(&self, text: &str, config: &OutputConfig) -> AppResult<()> {
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
        let settle = (config.typing_delay_ms as u64).max(50);
        thread::sleep(Duration::from_millis(settle));

        // Restore original clipboard contents
        if let Some(prev) = previous {
            let _ = clipboard.set_text(&prev);
        }

        Ok(())
    }

    /// Simulates Ctrl+V on Windows.
    /// (Legacy — kept as fallback; prefer `type_text` for insertion.)
    #[allow(dead_code)]
    fn send_paste_keystroke(&self) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        enigo
            .key(Key::Control, Direction::Press)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;
        enigo
            .key(Key::Unicode('v'), Direction::Click)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;
        enigo
            .key(Key::Control, Direction::Release)
            .map_err(|e| AppError::Output(format!("Keystroke failed: {e}")))?;

        Ok(())
    }

    /// Character-by-character typing fallback.
    /// Use when the target app intercepts Ctrl+V (rare, but possible).
    #[allow(dead_code)]
    fn type_characters(&self, text: &str, char_delay_ms: u32) -> AppResult<()> {
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| AppError::Output(format!("Failed to init keystroke engine: {e}")))?;

        for ch in text.chars() {
            enigo
                .text(&ch.to_string())
                .map_err(|e| AppError::Output(format!("Typing failed: {e}")))?;

            if char_delay_ms > 0 {
                thread::sleep(Duration::from_millis(char_delay_ms as u64));
            }
        }

        Ok(())
    }
}
