//! Voice command detection and parsing.
//!
//! Scans post-processed transcription text for spoken commands like "new line",
//! "new paragraph", and "delete last word".  Splits the text into a sequence of
//! [`OutputSegment`]s that the output router can execute as mixed text + keystrokes.
//!
//! Runs after the processor chain and formatter so commands are detected in
//! clean, fully-formatted text.  Does not interfere with filler removal,
//! capitalization, or list formatting.

/// A voice command that maps to OS-level keystrokes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VoiceCommand {
    /// Single line break (Shift+Enter).
    NewLine,
    /// Double line break / paragraph break (Shift+Enter × 2).
    NewParagraph,
    /// Delete the previous word (Ctrl+Backspace).
    DeleteLastWord,
    /// Send the message (Enter). Only triggers when "send" is the last word.
    Send,
}

/// A segment of output: either literal text to type, or a command to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSegment {
    Text(String),
    Command(VoiceCommand),
}

/// Command definitions: (phrase, command).
/// Sorted longest-first so "new paragraph" matches before "new line".
const COMMANDS: &[(&str, VoiceCommand)] = &[
    ("delete last word", VoiceCommand::DeleteLastWord),
    ("new paragraph", VoiceCommand::NewParagraph),
    ("new line", VoiceCommand::NewLine),
];

/// True if a byte is part of a "word" for command boundary matching.
/// Mirrors the logic in processor.rs.
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'\'' || b == b'-'
}

/// Parse voice commands from transcribed text.
///
/// Returns a sequence of [`OutputSegment`]s: literal text interleaved with
/// commands.  Commands are matched case-insensitively at word boundaries.
///
/// When `detect_send` is `false`, the trailing "send" → Enter detection is
/// skipped entirely.  All other voice commands still fire normally.
///
/// **"delete last word" optimization**: when this command follows a text
/// segment, the parser removes the last word from the preceding text instead
/// of emitting a `DeleteLastWord` command.  This avoids typing text and then
/// immediately sending Ctrl+Backspace (race condition).  A `DeleteLastWord`
/// command is only emitted when there is no preceding text to trim.
pub fn parse_commands(text: &str) -> Vec<OutputSegment> {
    parse_commands_inner(text, true)
}

/// Like [`parse_commands`] but allows the caller to disable "send" detection.
pub fn parse_commands_with_options(text: &str, detect_send: bool) -> Vec<OutputSegment> {
    parse_commands_inner(text, detect_send)
}

fn parse_commands_inner(text: &str, detect_send: bool) -> Vec<OutputSegment> {
    if text.is_empty() {
        return Vec::new();
    }

    let lower = text.to_lowercase();
    let bytes = text.as_bytes();
    let mut segments: Vec<OutputSegment> = Vec::new();
    let mut text_start: usize = 0;
    let mut i: usize = 0;

    while i < bytes.len() {
        let mut matched = false;

        for &(phrase, ref cmd) in COMMANDS {
            let phrase_len = phrase.len();
            if i + phrase_len > lower.len() {
                continue;
            }

            // Case-insensitive match.
            if &lower[i..i + phrase_len] != phrase {
                continue;
            }

            // Word boundary checks.
            let at_word_start = i == 0 || !is_word_char(bytes[i - 1]);
            let end_pos = i + phrase_len;
            let at_word_end = end_pos >= bytes.len() || !is_word_char(bytes[end_pos]);

            if !at_word_start || !at_word_end {
                continue;
            }

            // Flush accumulated text before this command.
            if text_start < i {
                let segment_text = text[text_start..i].trim_end().to_string();
                if !segment_text.is_empty() {
                    segments.push(OutputSegment::Text(segment_text));
                }
            }

            // Handle "delete last word" optimization: remove last word from
            // preceding text segment instead of emitting a command.
            if *cmd == VoiceCommand::DeleteLastWord {
                if let Some(OutputSegment::Text(ref mut prev)) = segments.last_mut() {
                    // Trim trailing whitespace, then remove the last word.
                    let trimmed = prev.trim_end();
                    if let Some(space_pos) = trimmed.rfind(|c: char| c.is_whitespace()) {
                        *prev = trimmed[..space_pos].trim_end().to_string();
                    } else {
                        // Only one word — remove the entire text segment.
                        *prev = String::new();
                    }
                    // Remove the segment entirely if it's now empty.
                    if prev.is_empty() {
                        segments.pop();
                    }
                } else {
                    // No preceding text — emit command so OutputRouter sends
                    // Ctrl+Backspace to delete from previously typed content.
                    segments.push(OutputSegment::Command(cmd.clone()));
                }
            } else {
                segments.push(OutputSegment::Command(cmd.clone()));
            }

            // Advance past the command phrase and any leading whitespace after it.
            i = end_pos;
            while i < bytes.len() && bytes[i] == b' ' {
                i += 1;
            }
            text_start = i;
            matched = true;
            break;
        }

        if !matched {
            i += 1;
        }
    }

    // Flush remaining text.
    if text_start < text.len() {
        let remaining = text[text_start..].trim().to_string();
        if !remaining.is_empty() {
            segments.push(OutputSegment::Text(remaining));
        }
    }

    // "send" command — only matches as the very last word to avoid
    // false positives (since "send" is a common English word).
    // Skipped entirely when `detect_send` is false.
    // Check + strip in two phases to satisfy the borrow checker.
    let is_send_at_end = detect_send && matches!(segments.last(), Some(OutputSegment::Text(t)) if {
        let last_word = t.trim_end()
            .rsplit_once(|c: char| c.is_whitespace())
            .map_or(t.trim_end(), |(_, w)| w);
        // Strip trailing punctuation Whisper may add ("send." → "send")
        let core = last_word.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
        core.eq_ignore_ascii_case("send")
    });

    if is_send_at_end {
        if let Some(OutputSegment::Text(t)) = segments.last_mut() {
            let trimmed = t.trim_end();
            if let Some((prefix, _)) = trimmed.rsplit_once(|c: char| c.is_whitespace()) {
                *t = prefix.trim_end().to_string();
            } else {
                *t = String::new();
            }
            if t.is_empty() {
                segments.pop();
            }
        }
        segments.push(OutputSegment::Command(VoiceCommand::Send));
    }

    segments
}

/// Collapse segments back into a plain string for clipboard mode.
///
/// - `NewLine` → `\n`
/// - `NewParagraph` → `\n\n`
/// - `DeleteLastWord` → omitted (can't execute via clipboard)
pub fn segments_to_string(segments: &[OutputSegment]) -> String {
    let mut out = String::new();
    for seg in segments {
        match seg {
            OutputSegment::Text(s) => out.push_str(s),
            OutputSegment::Command(VoiceCommand::NewLine) => out.push('\n'),
            OutputSegment::Command(VoiceCommand::NewParagraph) => out.push_str("\n\n"),
            OutputSegment::Command(VoiceCommand::DeleteLastWord) => {}
            OutputSegment::Command(VoiceCommand::Send) => {} // keystroke-only, omitted in clipboard
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Basic command detection ────────────────────────────────────

    #[test]
    fn new_line_alone() {
        let result = parse_commands("new line");
        assert_eq!(result, vec![OutputSegment::Command(VoiceCommand::NewLine)]);
    }

    #[test]
    fn new_paragraph_alone() {
        let result = parse_commands("new paragraph");
        assert_eq!(result, vec![OutputSegment::Command(VoiceCommand::NewParagraph)]);
    }

    #[test]
    fn delete_last_word_alone() {
        // No preceding text → emits command.
        let result = parse_commands("delete last word");
        assert_eq!(result, vec![OutputSegment::Command(VoiceCommand::DeleteLastWord)]);
    }

    // ── Mid-text commands ─────────────────────────────────────────

    #[test]
    fn new_line_mid_text() {
        let result = parse_commands("hello new line world");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text("world".to_string()),
        ]);
    }

    #[test]
    fn new_paragraph_mid_text() {
        let result = parse_commands("hello new paragraph world");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewParagraph),
            OutputSegment::Text("world".to_string()),
        ]);
    }

    // ── Delete last word optimization ─────────────────────────────

    #[test]
    fn delete_last_word_removes_preceding_word() {
        // "hello world delete last word" → "hello" (removes "world" in parser)
        let result = parse_commands("hello world delete last word");
        assert_eq!(result, vec![OutputSegment::Text("hello".to_string())]);
    }

    #[test]
    fn delete_last_word_removes_only_word() {
        // "hello delete last word" → empty (removes "hello", segment dropped)
        let result = parse_commands("hello delete last word");
        assert_eq!(result, vec![]);
    }

    #[test]
    fn delete_last_word_with_trailing_text() {
        // "hello world delete last word more text"
        // → "hello" then "more text"
        let result = parse_commands("hello world delete last word more text");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Text("more text".to_string()),
        ]);
    }

    // ── Case insensitivity ────────────────────────────────────────

    #[test]
    fn case_insensitive_new_line() {
        let result = parse_commands("hello New Line world");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text("world".to_string()),
        ]);
    }

    #[test]
    fn case_insensitive_new_paragraph() {
        let result = parse_commands("NEW PARAGRAPH");
        assert_eq!(result, vec![OutputSegment::Command(VoiceCommand::NewParagraph)]);
    }

    // ── Word boundary enforcement ─────────────────────────────────

    #[test]
    fn no_match_inside_word() {
        // "new lineup" should NOT match "new line"
        let input = "new lineup";
        let result = parse_commands(input);
        assert_eq!(result, vec![OutputSegment::Text("new lineup".to_string())]);
    }

    #[test]
    fn no_match_partial_start() {
        // "renew line" should NOT match "new line"
        let input = "renew line";
        let result = parse_commands(input);
        assert_eq!(result, vec![OutputSegment::Text("renew line".to_string())]);
    }

    // ── Multiple commands ─────────────────────────────────────────

    #[test]
    fn multiple_commands() {
        let result = parse_commands("hello new line world new line goodbye");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text("world".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text("goodbye".to_string()),
        ]);
    }

    #[test]
    fn consecutive_commands() {
        let result = parse_commands("new line new paragraph");
        assert_eq!(result, vec![
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Command(VoiceCommand::NewParagraph),
        ]);
    }

    // ── Edge cases ────────────────────────────────────────────────

    #[test]
    fn empty_input() {
        assert_eq!(parse_commands(""), Vec::<OutputSegment>::new());
    }

    #[test]
    fn no_commands() {
        let result = parse_commands("hello world this is a test");
        assert_eq!(result, vec![OutputSegment::Text("hello world this is a test".to_string())]);
    }

    #[test]
    fn command_at_end() {
        let result = parse_commands("hello world new line");
        assert_eq!(result, vec![
            OutputSegment::Text("hello world".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
        ]);
    }

    #[test]
    fn command_with_trailing_punctuation() {
        // "hello new line." — period after command phrase.
        // The period is not a word char, so "new line" still matches at boundary.
        // The period becomes trailing text.
        let result = parse_commands("hello new line.");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text(".".to_string()),
        ]);
    }

    // ── segments_to_string ────────────────────────────────────────

    // ── Send command (end-of-text only) ─────────────────────────

    #[test]
    fn send_at_end() {
        let result = parse_commands("hello world send");
        assert_eq!(result, vec![
            OutputSegment::Text("hello world".to_string()),
            OutputSegment::Command(VoiceCommand::Send),
        ]);
    }

    #[test]
    fn send_alone() {
        let result = parse_commands("send");
        assert_eq!(result, vec![OutputSegment::Command(VoiceCommand::Send)]);
    }

    #[test]
    fn send_case_insensitive() {
        let result = parse_commands("hello Send");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::Send),
        ]);
    }

    #[test]
    fn send_with_trailing_period() {
        // Whisper may add punctuation — "send." should still trigger
        let result = parse_commands("hello send.");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::Send),
        ]);
    }

    #[test]
    fn send_mid_text_does_not_trigger() {
        // "send" in the middle of a sentence should NOT trigger
        let result = parse_commands("please send the email");
        assert_eq!(result, vec![OutputSegment::Text("please send the email".to_string())]);
    }

    #[test]
    fn send_as_part_of_word_does_not_trigger() {
        // "sending" should NOT trigger
        let result = parse_commands("I am sending");
        assert_eq!(result, vec![OutputSegment::Text("I am sending".to_string())]);
    }

    #[test]
    fn send_after_command() {
        let result = parse_commands("hello new line world send");
        assert_eq!(result, vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text("world".to_string()),
            OutputSegment::Command(VoiceCommand::Send),
        ]);
    }

    // ── segments_to_string ────────────────────────────────────────

    #[test]
    fn segments_to_string_basic() {
        let segments = vec![
            OutputSegment::Text("hello".to_string()),
            OutputSegment::Command(VoiceCommand::NewLine),
            OutputSegment::Text("world".to_string()),
        ];
        assert_eq!(segments_to_string(&segments), "hello\nworld");
    }

    #[test]
    fn segments_to_string_paragraph() {
        let segments = vec![
            OutputSegment::Text("first".to_string()),
            OutputSegment::Command(VoiceCommand::NewParagraph),
            OutputSegment::Text("second".to_string()),
        ];
        assert_eq!(segments_to_string(&segments), "first\n\nsecond");
    }

    #[test]
    fn segments_to_string_delete_omitted() {
        let segments = vec![
            OutputSegment::Command(VoiceCommand::DeleteLastWord),
            OutputSegment::Text("hello".to_string()),
        ];
        assert_eq!(segments_to_string(&segments), "hello");
    }
}
