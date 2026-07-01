//! Voice command detection and parsing.
//!
//! Scans post-processed transcription text for spoken commands like "new line",
//! "new paragraph", and "delete last word".  Splits the text into a sequence of
//! [`OutputSegment`]s that the output router can execute as mixed text + keystrokes.
//!
//! Runs after the processor chain and formatter so commands are detected in
//! clean, fully-formatted text.  Does not interfere with filler removal,
//! capitalization, or list formatting.

/// A modifier key for a user-defined [`VoiceCommand::KeyCombo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyModifier {
    Ctrl,
    Alt,
    Shift,
    Meta,
}

/// The main (non-modifier) key of a user-defined [`VoiceCommand::KeyCombo`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboKey {
    /// A single ASCII letter or digit.
    Char(char),
    Tab,
    Escape,
    Enter,
    Space,
    Backspace,
}

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
    /// Select all (Ctrl/Cmd+A).
    SelectAll,
    /// Copy (Ctrl/Cmd+C).
    Copy,
    /// Cut (Ctrl/Cmd+X).
    Cut,
    /// Undo (Ctrl/Cmd+Z).
    Undo,
    /// Redo (Ctrl/Cmd+Shift+Z).
    Redo,
    /// Press Tab.
    PressTab,
    /// Press Escape.
    PressEscape,
    /// Press Enter inline (fires wherever spoken, unlike "send").
    PressEnter,
    /// User-defined key combination (e.g. Ctrl+Shift+K): press modifiers,
    /// click the key, release modifiers.
    KeyCombo {
        modifiers: Vec<KeyModifier>,
        key: ComboKey,
    },
}

/// When a command phrase is allowed to fire within an utterance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerScope {
    /// Matches at any word boundary, anywhere in the text.
    Anywhere,
    /// Matches only as the trailing word(s) of the text (like "send").
    EndOfUtterance,
}

/// A single command definition: a spoken phrase, the command it triggers, and
/// the scope in which it is allowed to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandDef {
    pub phrase: String,
    pub command: VoiceCommand,
    pub scope: TriggerScope,
}

/// The built-in command table, seeded into the DB on first run and used as a
/// fallback when the DB can't be read.  Order also seeds `sort_order`.
pub fn default_command_table() -> Vec<CommandDef> {
    use TriggerScope::{Anywhere, EndOfUtterance};
    use VoiceCommand::*;
    vec![
        CommandDef { phrase: "new line".into(), command: NewLine, scope: Anywhere },
        CommandDef { phrase: "new paragraph".into(), command: NewParagraph, scope: Anywhere },
        CommandDef { phrase: "delete last word".into(), command: DeleteLastWord, scope: Anywhere },
        CommandDef { phrase: "select all".into(), command: SelectAll, scope: Anywhere },
        CommandDef { phrase: "copy that".into(), command: Copy, scope: Anywhere },
        CommandDef { phrase: "cut that".into(), command: Cut, scope: Anywhere },
        CommandDef { phrase: "undo that".into(), command: Undo, scope: Anywhere },
        CommandDef { phrase: "redo that".into(), command: Redo, scope: Anywhere },
        CommandDef { phrase: "press tab".into(), command: PressTab, scope: Anywhere },
        CommandDef { phrase: "press escape".into(), command: PressEscape, scope: Anywhere },
        CommandDef { phrase: "press enter".into(), command: PressEnter, scope: Anywhere },
        CommandDef { phrase: "send".into(), command: Send, scope: EndOfUtterance },
    ]
}

/// A segment of output: either literal text to type, or a command to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputSegment {
    Text(String),
    Command(VoiceCommand),
}

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
    parse_commands_with_table(text, &default_command_table())
}

/// Like [`parse_commands`] but allows the caller to disable "send" detection.
pub fn parse_commands_with_options(text: &str, detect_send: bool) -> Vec<OutputSegment> {
    let mut table = default_command_table();
    if !detect_send {
        table.retain(|d| d.command != VoiceCommand::Send);
    }
    parse_commands_with_table(text, &table)
}

/// Parse voice commands from `text` using a user-supplied command `table`.
///
/// `Anywhere`-scope phrases match inline at word boundaries (longest phrase
/// first).  `EndOfUtterance`-scope phrases match only as the trailing word(s),
/// generalizing the old "send" special case (trailing punctuation is stripped).
pub fn parse_commands_with_table(text: &str, table: &[CommandDef]) -> Vec<OutputSegment> {
    if text.is_empty() {
        return Vec::new();
    }

    // Split by scope; sort each longest-phrase-first so "new paragraph" wins
    // over "new line" and multi-word end phrases win over single words.
    let mut anywhere: Vec<&CommandDef> = table
        .iter()
        .filter(|d| d.scope == TriggerScope::Anywhere)
        .collect();
    anywhere.sort_by(|a, b| b.phrase.len().cmp(&a.phrase.len()));
    let mut end_of_utterance: Vec<&CommandDef> = table
        .iter()
        .filter(|d| d.scope == TriggerScope::EndOfUtterance)
        .collect();
    end_of_utterance.sort_by(|a, b| b.phrase.len().cmp(&a.phrase.len()));

    let bytes = text.as_bytes();
    let mut segments: Vec<OutputSegment> = Vec::new();
    let mut text_start: usize = 0;
    let mut i: usize = 0;

    while i < bytes.len() {
        let mut matched = false;

        for def in &anywhere {
            let phrase = def.phrase.as_bytes();
            let phrase_len = phrase.len();
            if phrase_len == 0 || i + phrase_len > bytes.len() {
                continue;
            }

            // Case-insensitive match against the original bytes. Phrases are
            // ASCII, so we compare raw bytes with `eq_ignore_ascii_case`
            // instead of slicing `text.to_lowercase()`: some non-ASCII
            // uppercase chars (e.g. Turkish 'İ') change byte length when
            // lowercased, which would desync the lowercase string from
            // `text`'s byte indices and could panic on a non-boundary slice.
            if !bytes[i..i + phrase_len].eq_ignore_ascii_case(phrase) {
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
            if def.command == VoiceCommand::DeleteLastWord {
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
                    segments.push(OutputSegment::Command(def.command.clone()));
                }
            } else {
                segments.push(OutputSegment::Command(def.command.clone()));
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

    // End-of-utterance pass: match a trailing command phrase (e.g. "send").
    apply_end_of_utterance(&mut segments, &end_of_utterance);

    segments
}

/// Match at most one `EndOfUtterance`-scope phrase against the trailing text
/// segment and, on a hit, strip the phrase and append its command.
///
/// Generalizes the old "send" special case: the trailing word(s) must equal a
/// phrase at a whitespace boundary, with any trailing punctuation Whisper adds
/// ("send." → "send") ignored.
fn apply_end_of_utterance(segments: &mut Vec<OutputSegment>, eou: &[&CommandDef]) {
    if eou.is_empty() {
        return;
    }

    // Phase 1: inspect the trailing text immutably and decide the outcome.
    // `None` → no match. `Some((new_text, cmd))` → replace the trailing text
    // with `new_text` (or drop it when `None`) and append `cmd`.
    let outcome: Option<(Option<String>, VoiceCommand)> =
        if let Some(OutputSegment::Text(t)) = segments.last() {
            let trimmed = t.trim_end();
            // Strip trailing punctuation Whisper may add ("send." → "send").
            let core = trimmed.trim_end_matches(|c: char| !c.is_ascii_alphanumeric());
            let mut found = None;
            for def in eou {
                let plen = def.phrase.len();
                if plen == 0 || core.len() < plen {
                    continue;
                }
                let split = core.len() - plen;
                if !core.is_char_boundary(split) {
                    continue;
                }
                let (prefix, tail) = core.split_at(split);
                if !tail.eq_ignore_ascii_case(&def.phrase) {
                    continue;
                }
                // Require a whitespace boundary before the phrase so "sending"
                // or "hello-send" don't trigger.
                if !prefix.is_empty() && !prefix.ends_with(|c: char| c.is_whitespace()) {
                    continue;
                }
                let np = prefix.trim_end();
                let new_text = if np.is_empty() {
                    None
                } else {
                    Some(np.to_string())
                };
                found = Some((new_text, def.command.clone()));
                break;
            }
            found
        } else {
            None
        };

    // Phase 2: apply the outcome.
    if let Some((new_text, command)) = outcome {
        match new_text {
            Some(txt) => {
                if let Some(OutputSegment::Text(t)) = segments.last_mut() {
                    *t = txt;
                }
            }
            None => {
                segments.pop();
            }
        }
        segments.push(OutputSegment::Command(command));
    }
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
            // Keystroke-only commands: no textual representation in clipboard mode.
            OutputSegment::Command(
                VoiceCommand::SelectAll
                | VoiceCommand::Copy
                | VoiceCommand::Cut
                | VoiceCommand::Undo
                | VoiceCommand::Redo
                | VoiceCommand::PressTab
                | VoiceCommand::PressEscape
                | VoiceCommand::PressEnter
                | VoiceCommand::KeyCombo { .. },
            ) => {}
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
        assert_eq!(
            result,
            vec![OutputSegment::Command(VoiceCommand::NewParagraph)]
        );
    }

    #[test]
    fn delete_last_word_alone() {
        // No preceding text → emits command.
        let result = parse_commands("delete last word");
        assert_eq!(
            result,
            vec![OutputSegment::Command(VoiceCommand::DeleteLastWord)]
        );
    }

    // ── Mid-text commands ─────────────────────────────────────────

    #[test]
    fn new_line_mid_text() {
        let result = parse_commands("hello new line world");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text("world".to_string()),
            ]
        );
    }

    #[test]
    fn new_paragraph_mid_text() {
        let result = parse_commands("hello new paragraph world");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::NewParagraph),
                OutputSegment::Text("world".to_string()),
            ]
        );
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
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Text("more text".to_string()),
            ]
        );
    }

    // ── Case insensitivity ────────────────────────────────────────

    #[test]
    fn case_insensitive_new_line() {
        let result = parse_commands("hello New Line world");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text("world".to_string()),
            ]
        );
    }

    #[test]
    fn case_insensitive_new_paragraph() {
        let result = parse_commands("NEW PARAGRAPH");
        assert_eq!(
            result,
            vec![OutputSegment::Command(VoiceCommand::NewParagraph)]
        );
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
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text("world".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text("goodbye".to_string()),
            ]
        );
    }

    #[test]
    fn consecutive_commands() {
        let result = parse_commands("new line new paragraph");
        assert_eq!(
            result,
            vec![
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Command(VoiceCommand::NewParagraph),
            ]
        );
    }

    // ── Edge cases ────────────────────────────────────────────────

    #[test]
    fn empty_input() {
        assert_eq!(parse_commands(""), Vec::<OutputSegment>::new());
    }

    #[test]
    fn no_commands() {
        let result = parse_commands("hello world this is a test");
        assert_eq!(
            result,
            vec![OutputSegment::Text(
                "hello world this is a test".to_string()
            )]
        );
    }

    #[test]
    fn command_at_end() {
        let result = parse_commands("hello world new line");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello world".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
            ]
        );
    }

    #[test]
    fn command_with_trailing_punctuation() {
        // "hello new line." — period after command phrase.
        // The period is not a word char, so "new line" still matches at boundary.
        // The period becomes trailing text.
        let result = parse_commands("hello new line.");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text(".".to_string()),
            ]
        );
    }

    // ── segments_to_string ────────────────────────────────────────

    // ── Send command (end-of-text only) ─────────────────────────

    #[test]
    fn send_at_end() {
        let result = parse_commands("hello world send");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello world".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
    }

    #[test]
    fn send_alone() {
        let result = parse_commands("send");
        assert_eq!(result, vec![OutputSegment::Command(VoiceCommand::Send)]);
    }

    #[test]
    fn send_case_insensitive() {
        let result = parse_commands("hello Send");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
    }

    #[test]
    fn send_with_trailing_period() {
        // Whisper may add punctuation — "send." should still trigger
        let result = parse_commands("hello send.");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
    }

    #[test]
    fn send_mid_text_does_not_trigger() {
        // "send" in the middle of a sentence should NOT trigger
        let result = parse_commands("please send the email");
        assert_eq!(
            result,
            vec![OutputSegment::Text("please send the email".to_string())]
        );
    }

    #[test]
    fn send_as_part_of_word_does_not_trigger() {
        // "sending" should NOT trigger
        let result = parse_commands("I am sending");
        assert_eq!(
            result,
            vec![OutputSegment::Text("I am sending".to_string())]
        );
    }

    #[test]
    fn send_after_command() {
        let result = parse_commands("hello new line world send");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text("world".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
    }

    // ── Non-ASCII robustness (no panic) ───────────────────────────

    #[test]
    fn multibyte_uppercase_does_not_panic() {
        // Turkish dotted capital 'İ' (U+0130) grows when lowercased, which
        // used to desync byte indices between `text` and its lowercase form
        // and could panic on a non-boundary slice.
        let result = parse_commands("İ new line");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("İ".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
            ]
        );
    }

    #[test]
    fn accented_text_before_command() {
        let result = parse_commands("café new line");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("café".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
            ]
        );
    }

    // ── New keystroke commands ────────────────────────────────────

    #[test]
    fn select_all_alone() {
        assert_eq!(
            parse_commands("select all"),
            vec![OutputSegment::Command(VoiceCommand::SelectAll)]
        );
    }

    #[test]
    fn copy_that_alone() {
        assert_eq!(
            parse_commands("copy that"),
            vec![OutputSegment::Command(VoiceCommand::Copy)]
        );
    }

    #[test]
    fn cut_that_alone() {
        assert_eq!(
            parse_commands("cut that"),
            vec![OutputSegment::Command(VoiceCommand::Cut)]
        );
    }

    #[test]
    fn undo_that_alone() {
        assert_eq!(
            parse_commands("undo that"),
            vec![OutputSegment::Command(VoiceCommand::Undo)]
        );
    }

    #[test]
    fn redo_that_alone() {
        assert_eq!(
            parse_commands("redo that"),
            vec![OutputSegment::Command(VoiceCommand::Redo)]
        );
    }

    #[test]
    fn press_tab_alone() {
        assert_eq!(
            parse_commands("press tab"),
            vec![OutputSegment::Command(VoiceCommand::PressTab)]
        );
    }

    #[test]
    fn press_escape_alone() {
        assert_eq!(
            parse_commands("press escape"),
            vec![OutputSegment::Command(VoiceCommand::PressEscape)]
        );
    }

    #[test]
    fn press_enter_alone() {
        assert_eq!(
            parse_commands("press enter"),
            vec![OutputSegment::Command(VoiceCommand::PressEnter)]
        );
    }

    #[test]
    fn select_all_mid_text() {
        let result = parse_commands("hello select all world");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::SelectAll),
                OutputSegment::Text("world".to_string()),
            ]
        );
    }

    #[test]
    fn press_enter_mid_text_fires_inline() {
        // Unlike "send", "press enter" fires wherever spoken, not just at end.
        let result = parse_commands("line one press enter line two");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("line one".to_string()),
                OutputSegment::Command(VoiceCommand::PressEnter),
                OutputSegment::Text("line two".to_string()),
            ]
        );
    }

    #[test]
    fn copy_that_case_insensitive() {
        let result = parse_commands("hello Copy That world");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::Copy),
                OutputSegment::Text("world".to_string()),
            ]
        );
    }

    #[test]
    fn press_escape_case_insensitive() {
        assert_eq!(
            parse_commands("PRESS ESCAPE"),
            vec![OutputSegment::Command(VoiceCommand::PressEscape)]
        );
    }

    #[test]
    fn redo_takes_priority_over_no_such_shorter_phrase() {
        // "redo that" must not be mistaken for anything shorter; verifies the
        // longest-first table still segments cleanly around it.
        let result = parse_commands("do it redo that now");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("do it".to_string()),
                OutputSegment::Command(VoiceCommand::Redo),
                OutputSegment::Text("now".to_string()),
            ]
        );
    }

    // ── False-trigger behavior (documents the boundary matcher) ───

    #[test]
    fn select_all_triggers_mid_sentence() {
        // NOTE: the matcher fires at any word boundary, so "select all" DOES
        // trigger mid-sentence. This is the real, intended behavior — we do
        // not special-case it. See report for the residual risk.
        let result = parse_commands("please select all the files");
        assert_eq!(
            result,
            vec![
                OutputSegment::Text("please".to_string()),
                OutputSegment::Command(VoiceCommand::SelectAll),
                OutputSegment::Text("the files".to_string()),
            ]
        );
    }

    #[test]
    fn press_tab_not_matched_inside_word() {
        // "press table" should NOT match "press tab" (boundary enforced).
        let result = parse_commands("press table");
        assert_eq!(
            result,
            vec![OutputSegment::Text("press table".to_string())]
        );
    }

    #[test]
    fn segments_to_string_omits_keystroke_commands() {
        let segments = vec![
            OutputSegment::Text("a".to_string()),
            OutputSegment::Command(VoiceCommand::SelectAll),
            OutputSegment::Command(VoiceCommand::Copy),
            OutputSegment::Command(VoiceCommand::Cut),
            OutputSegment::Command(VoiceCommand::Undo),
            OutputSegment::Command(VoiceCommand::Redo),
            OutputSegment::Command(VoiceCommand::PressTab),
            OutputSegment::Command(VoiceCommand::PressEscape),
            OutputSegment::Command(VoiceCommand::PressEnter),
            OutputSegment::Text("b".to_string()),
        ];
        assert_eq!(segments_to_string(&segments), "ab");
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

    // ── Table-driven scopes (parse_commands_with_table) ───────────

    /// Build a KeyCombo command for tests.
    fn key_combo(mods: &[KeyModifier], key: ComboKey) -> VoiceCommand {
        VoiceCommand::KeyCombo {
            modifiers: mods.to_vec(),
            key,
        }
    }

    #[test]
    fn end_of_utterance_scope_only_matches_at_end() {
        let table = vec![CommandDef {
            phrase: "go".to_string(),
            command: VoiceCommand::Send,
            scope: TriggerScope::EndOfUtterance,
        }];
        // Trailing → matches.
        assert_eq!(
            parse_commands_with_table("hello go", &table),
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
        // Mid-sentence → does NOT match (this is the false-trigger fix).
        assert_eq!(
            parse_commands_with_table("go home now", &table),
            vec![OutputSegment::Text("go home now".to_string())]
        );
    }

    #[test]
    fn anywhere_scope_matches_inline() {
        let table = vec![CommandDef {
            phrase: "boom".to_string(),
            command: VoiceCommand::NewLine,
            scope: TriggerScope::Anywhere,
        }];
        assert_eq!(
            parse_commands_with_table("a boom b", &table),
            vec![
                OutputSegment::Text("a".to_string()),
                OutputSegment::Command(VoiceCommand::NewLine),
                OutputSegment::Text("b".to_string()),
            ]
        );
    }

    #[test]
    fn same_phrase_end_scope_does_not_fire_inline() {
        // "select all" as End-of-utterance must not trigger mid-sentence,
        // unlike the default Anywhere behavior — this is exactly the knob the
        // UI exposes to fix false triggers.
        let table = vec![CommandDef {
            phrase: "select all".to_string(),
            command: VoiceCommand::SelectAll,
            scope: TriggerScope::EndOfUtterance,
        }];
        assert_eq!(
            parse_commands_with_table("please select all the files", &table),
            vec![OutputSegment::Text(
                "please select all the files".to_string()
            )]
        );
        assert_eq!(
            parse_commands_with_table("now select all", &table),
            vec![
                OutputSegment::Text("now".to_string()),
                OutputSegment::Command(VoiceCommand::SelectAll),
            ]
        );
    }

    #[test]
    fn custom_key_combo_round_trips_through_parser() {
        let cmd = key_combo(&[KeyModifier::Ctrl, KeyModifier::Shift], ComboKey::Char('k'));
        let table = vec![CommandDef {
            phrase: "command palette".to_string(),
            command: cmd.clone(),
            scope: TriggerScope::Anywhere,
        }];
        assert_eq!(
            parse_commands_with_table("open command palette please", &table),
            vec![
                OutputSegment::Text("open".to_string()),
                OutputSegment::Command(cmd),
                OutputSegment::Text("please".to_string()),
            ]
        );
    }

    #[test]
    fn longest_first_with_mixed_scopes() {
        // "new" (Anywhere) vs "new paragraph" (Anywhere): the longer wins.
        // "up" is EndOfUtterance and only fires at the very end.
        let table = vec![
            CommandDef {
                phrase: "new".to_string(),
                command: VoiceCommand::NewLine,
                scope: TriggerScope::Anywhere,
            },
            CommandDef {
                phrase: "new paragraph".to_string(),
                command: VoiceCommand::NewParagraph,
                scope: TriggerScope::Anywhere,
            },
            CommandDef {
                phrase: "up".to_string(),
                command: VoiceCommand::Send,
                scope: TriggerScope::EndOfUtterance,
            },
        ];
        assert_eq!(
            parse_commands_with_table("a new paragraph b up", &table),
            vec![
                OutputSegment::Text("a".to_string()),
                OutputSegment::Command(VoiceCommand::NewParagraph),
                OutputSegment::Text("b".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
    }

    #[test]
    fn command_send_off_excludes_send() {
        // parse_commands_with_options(false) must drop the trailing "send".
        assert_eq!(
            parse_commands_with_options("hello send", false),
            vec![OutputSegment::Text("hello send".to_string())]
        );
        assert_eq!(
            parse_commands_with_options("hello send", true),
            vec![
                OutputSegment::Text("hello".to_string()),
                OutputSegment::Command(VoiceCommand::Send),
            ]
        );
    }
}
