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
    /// Left mouse click at the current cursor position.
    MouseClick,
    /// Right mouse click at the current cursor position.
    MouseRightClick,
    /// Double left click at the current cursor position.
    MouseDoubleClick,
    /// Scroll the mouse wheel up.
    ScrollUp,
    /// Scroll the mouse wheel down.
    ScrollDown,
    /// Launch a program by command line (program + args, run directly with no
    /// shell). The string is tokenized with [`tokenize_command_line`].
    LaunchApp(String),
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

/// Opt-in commands seeded **disabled** so a stray transcription can't move the
/// mouse or switch windows mid-dictation. These are deliberately NOT part of
/// [`default_command_table`], so they never fire via the parser fallback used
/// when the DB can't be read — they only become active once the user enables
/// them in the Voice Commands page. They default to `EndOfUtterance` scope as
/// an extra guard even while disabled.
///
/// Only `switch window` (Alt+Tab) is seeded for window management: the other
/// common shortcuts (minimize/maximize on Meta+Arrow, close on Alt+F4) need
/// arrow/function keys that [`ComboKey`] does not currently model.
pub fn default_disabled_command_table() -> Vec<CommandDef> {
    use TriggerScope::EndOfUtterance;
    use VoiceCommand::*;
    vec![
        CommandDef { phrase: "mouse click".into(), command: MouseClick, scope: EndOfUtterance },
        CommandDef { phrase: "right click".into(), command: MouseRightClick, scope: EndOfUtterance },
        CommandDef { phrase: "double click".into(), command: MouseDoubleClick, scope: EndOfUtterance },
        CommandDef { phrase: "scroll up".into(), command: ScrollUp, scope: EndOfUtterance },
        CommandDef { phrase: "scroll down".into(), command: ScrollDown, scope: EndOfUtterance },
        CommandDef {
            phrase: "switch window".into(),
            command: KeyCombo { modifiers: vec![KeyModifier::Alt], key: ComboKey::Tab },
            scope: EndOfUtterance,
        },
    ]
}

/// Split a command line into program + arguments using whitespace splitting
/// that respects double-quoted spans. There is **no** shell interpretation —
/// no variable expansion, globbing, or metacharacters — so the tokens can be
/// passed straight to `std::process::Command` with no injection surface.
///
/// Double quotes group whitespace into one token and are removed from the
/// output (`say "hello world"` → `["say", "hello world"]`). An empty `""` is
/// preserved as an empty argument. An unbalanced quote runs to end of string.
pub fn tokenize_command_line(line: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut has_token = false;
    for c in line.chars() {
        match c {
            '"' => {
                in_quotes = !in_quotes;
                has_token = true; // a bare "" is still a (empty) token
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_token {
                    tokens.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                cur.push(c);
                has_token = true;
            }
        }
    }
    if has_token {
        tokens.push(cur);
    }
    tokens
}

// ── action <-> VoiceCommand encoding ─────────────────────────────
//
// The canonical string codec for a [`VoiceCommand`].  Built-ins encode as
// their variant name; custom key combos as `key:ctrl+shift+k`; mouse actions
// as `mouse:click` etc.; app launches as `launch:<command line>`.  These are
// the exact strings persisted in the voice-command registry AND the strings
// the LLM intent layer emits — decoding lives here, next to the enum, so both
// consumers share one source of truth.

/// Decode a stored `action` string into a [`VoiceCommand`].
///
/// Returns `None` for anything unrecognized (caller skips + logs).
pub fn action_to_command(action: &str) -> Option<VoiceCommand> {
    match action {
        "NewLine" => Some(VoiceCommand::NewLine),
        "NewParagraph" => Some(VoiceCommand::NewParagraph),
        "DeleteLastWord" => Some(VoiceCommand::DeleteLastWord),
        "Send" => Some(VoiceCommand::Send),
        "SelectAll" => Some(VoiceCommand::SelectAll),
        "Copy" => Some(VoiceCommand::Copy),
        "Cut" => Some(VoiceCommand::Cut),
        "Undo" => Some(VoiceCommand::Undo),
        "Redo" => Some(VoiceCommand::Redo),
        "PressTab" => Some(VoiceCommand::PressTab),
        "PressEscape" => Some(VoiceCommand::PressEscape),
        "PressEnter" => Some(VoiceCommand::PressEnter),
        "mouse:click" => Some(VoiceCommand::MouseClick),
        "mouse:right_click" => Some(VoiceCommand::MouseRightClick),
        "mouse:double_click" => Some(VoiceCommand::MouseDoubleClick),
        "mouse:scroll_up" => Some(VoiceCommand::ScrollUp),
        "mouse:scroll_down" => Some(VoiceCommand::ScrollDown),
        other if other.starts_with("key:") => parse_key_combo(other),
        // Everything after "launch:" is the raw command line (no shell).
        other if other.starts_with("launch:") => {
            Some(VoiceCommand::LaunchApp(other["launch:".len()..].to_string()))
        }
        _ => None,
    }
}

/// Encode a [`VoiceCommand`] into its canonical `action` string.
pub fn command_to_action(cmd: &VoiceCommand) -> String {
    match cmd {
        VoiceCommand::NewLine => "NewLine".into(),
        VoiceCommand::NewParagraph => "NewParagraph".into(),
        VoiceCommand::DeleteLastWord => "DeleteLastWord".into(),
        VoiceCommand::Send => "Send".into(),
        VoiceCommand::SelectAll => "SelectAll".into(),
        VoiceCommand::Copy => "Copy".into(),
        VoiceCommand::Cut => "Cut".into(),
        VoiceCommand::Undo => "Undo".into(),
        VoiceCommand::Redo => "Redo".into(),
        VoiceCommand::PressTab => "PressTab".into(),
        VoiceCommand::PressEscape => "PressEscape".into(),
        VoiceCommand::PressEnter => "PressEnter".into(),
        VoiceCommand::KeyCombo { modifiers, key } => encode_key_combo(modifiers, key),
        VoiceCommand::MouseClick => "mouse:click".into(),
        VoiceCommand::MouseRightClick => "mouse:right_click".into(),
        VoiceCommand::MouseDoubleClick => "mouse:double_click".into(),
        VoiceCommand::ScrollUp => "mouse:scroll_up".into(),
        VoiceCommand::ScrollDown => "mouse:scroll_down".into(),
        VoiceCommand::LaunchApp(cmd_line) => format!("launch:{cmd_line}"),
    }
}

/// Encode a key combo as `key:ctrl+shift+k`.
fn encode_key_combo(modifiers: &[KeyModifier], key: &ComboKey) -> String {
    let mut parts: Vec<String> = modifiers
        .iter()
        .map(|m| match m {
            KeyModifier::Ctrl => "ctrl",
            KeyModifier::Alt => "alt",
            KeyModifier::Shift => "shift",
            KeyModifier::Meta => "meta",
        }
        .to_string())
        .collect();
    parts.push(match key {
        ComboKey::Char(c) => c.to_string(),
        ComboKey::Tab => "tab".into(),
        ComboKey::Escape => "escape".into(),
        ComboKey::Enter => "enter".into(),
        ComboKey::Space => "space".into(),
        ComboKey::Backspace => "backspace".into(),
    });
    format!("key:{}", parts.join("+"))
}

/// Parse a `key:ctrl+shift+k` spec. Last token is the key; the rest modifiers.
fn parse_key_combo(spec: &str) -> Option<VoiceCommand> {
    let body = spec.strip_prefix("key:")?;
    let tokens: Vec<&str> = body
        .split('+')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .collect();
    let (key_tok, mod_toks) = tokens.split_last()?;

    let mut modifiers = Vec::new();
    for t in mod_toks {
        let m = match t.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => KeyModifier::Ctrl,
            "alt" | "option" => KeyModifier::Alt,
            "shift" => KeyModifier::Shift,
            "meta" | "cmd" | "command" | "win" | "super" => KeyModifier::Meta,
            _ => return None,
        };
        modifiers.push(m);
    }
    let key = parse_combo_key(key_tok)?;
    Some(VoiceCommand::KeyCombo { modifiers, key })
}

fn parse_combo_key(tok: &str) -> Option<ComboKey> {
    match tok.to_ascii_lowercase().as_str() {
        "tab" => Some(ComboKey::Tab),
        "escape" | "esc" => Some(ComboKey::Escape),
        "enter" | "return" => Some(ComboKey::Enter),
        "space" => Some(ComboKey::Space),
        "backspace" => Some(ComboKey::Backspace),
        lower => {
            let mut chars = lower.chars();
            let c = chars.next()?;
            if chars.next().is_some() {
                return None; // more than one char, not a named key
            }
            if c.is_ascii_alphanumeric() {
                Some(ComboKey::Char(c))
            } else {
                None
            }
        }
    }
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
                | VoiceCommand::KeyCombo { .. }
                | VoiceCommand::MouseClick
                | VoiceCommand::MouseRightClick
                | VoiceCommand::MouseDoubleClick
                | VoiceCommand::ScrollUp
                | VoiceCommand::ScrollDown
                | VoiceCommand::LaunchApp(_),
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

    // ── Mouse commands (opt-in disabled table) ────────────────────

    #[test]
    fn disabled_table_mouse_phrases_are_excluded_from_defaults() {
        // Mouse phrases must NOT be in the default (fallback) table, so they
        // never fire when the DB can't be read.
        for def in default_command_table() {
            assert!(!matches!(
                def.command,
                VoiceCommand::MouseClick
                    | VoiceCommand::MouseRightClick
                    | VoiceCommand::MouseDoubleClick
                    | VoiceCommand::ScrollUp
                    | VoiceCommand::ScrollDown
            ));
        }
    }

    #[test]
    fn disabled_table_all_end_of_utterance() {
        for def in default_disabled_command_table() {
            assert_eq!(def.scope, TriggerScope::EndOfUtterance);
        }
    }

    #[test]
    fn mouse_click_parses_when_enabled_via_table() {
        let table = default_disabled_command_table();
        assert_eq!(
            parse_commands_with_table("mouse click", &table),
            vec![OutputSegment::Command(VoiceCommand::MouseClick)]
        );
        assert_eq!(
            parse_commands_with_table("scroll down", &table),
            vec![OutputSegment::Command(VoiceCommand::ScrollDown)]
        );
        // EndOfUtterance guard: mid-sentence must not trigger.
        assert_eq!(
            parse_commands_with_table("please scroll down the page", &table),
            vec![OutputSegment::Text(
                "please scroll down the page".to_string()
            )]
        );
    }

    // ── Command-line tokenizer ────────────────────────────────────

    #[test]
    fn tokenize_plain_whitespace() {
        assert_eq!(
            tokenize_command_line("notepad foo.txt"),
            vec!["notepad".to_string(), "foo.txt".to_string()]
        );
    }

    #[test]
    fn tokenize_collapses_runs_of_whitespace() {
        assert_eq!(
            tokenize_command_line("  a   b\tc  "),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn tokenize_respects_double_quotes() {
        assert_eq!(
            tokenize_command_line(r#"code "C:\My Docs\file.txt""#),
            vec!["code".to_string(), r"C:\My Docs\file.txt".to_string()]
        );
    }

    #[test]
    fn tokenize_empty_quotes_is_empty_arg() {
        assert_eq!(
            tokenize_command_line(r#"prog "" x"#),
            vec!["prog".to_string(), "".to_string(), "x".to_string()]
        );
    }

    #[test]
    fn tokenize_unbalanced_quote_runs_to_end() {
        assert_eq!(
            tokenize_command_line(r#"prog "unterminated arg"#),
            vec!["prog".to_string(), "unterminated arg".to_string()]
        );
    }

    #[test]
    fn tokenize_empty_line_is_empty() {
        assert!(tokenize_command_line("   ").is_empty());
        assert!(tokenize_command_line("").is_empty());
    }

    #[test]
    fn tokenize_no_shell_metacharacters_are_split() {
        // Metacharacters are literal — never interpreted. `;` and `&&` stay
        // inside their tokens; nothing is executed as a separate command.
        assert_eq!(
            tokenize_command_line("echo hi; rm -rf /"),
            vec![
                "echo".to_string(),
                "hi;".to_string(),
                "rm".to_string(),
                "-rf".to_string(),
                "/".to_string(),
            ]
        );
    }
}
