//! Voice "intent layer": map a natural-language command to a sequence of known
//! actions the app already knows how to execute.
//!
//! The local LLM (Qwen) is prompted to emit a small JSON object describing an
//! ordered plan of steps.  Each step is either literal text to type or one of
//! an enumerated set of *action strings* — the very same strings the
//! voice-command registry persists (see
//! [`crate::postprocess::voice_commands::action_to_command`]).  Reusing that
//! codec means the intent layer can only ever resolve to commands the output
//! router already runs; there is no second, drifting action vocabulary.
//!
//! This module is pure logic (schema, decode, destructive classification,
//! trigger detection, prompt construction) so it can be unit-tested without a
//! model or the Tauri runtime.

use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::postprocess::voice_commands::{
    action_to_command, command_to_action, ComboKey, KeyModifier, VoiceCommand,
};

/// Leading trigger word that arms the intent path for a single utterance.
/// Deliberately distinct from Structured Mode's trailing "Voxify" so the two
/// never collide, and placed at the *start* of the utterance ("computer, open
/// the terminal") the way a wake word naturally reads.
pub const INTENT_TRIGGER: &str = "computer";

/// The fixed action strings the model is allowed to choose from, listed
/// explicitly in the prompt.  This is the SINGLE source of truth shared with
/// the decoder: every entry here must decode via
/// [`action_to_command`], and a unit test enforces that so the prompt and the
/// decoder can never drift apart.  The two *parametric* action families
/// (`key:<combo>` and `launch:<command line>`) are documented in the prompt
/// text rather than enumerated here.
pub const ALLOWED_ACTIONS: &[&str] = &[
    "NewLine",
    "NewParagraph",
    "DeleteLastWord",
    "Send",
    "SelectAll",
    "Copy",
    "Cut",
    "Undo",
    "Redo",
    "PressTab",
    "PressEscape",
    "PressEnter",
    "mouse:click",
    "mouse:right_click",
    "mouse:double_click",
    "mouse:scroll_up",
    "mouse:scroll_down",
];

/// One raw step as emitted by the model, before the action string is resolved
/// to a concrete command.  Externally tagged so `{"type":"hello"}` and
/// `{"action":"Copy"}` map onto the two variants by key name.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IntentStep {
    /// Literal text to type verbatim.
    Type(String),
    /// An action-encoding string (see [`ALLOWED_ACTIONS`] and the `key:` /
    /// `launch:` families).
    Action(String),
}

/// The JSON object the model must return: an ordered list of steps.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct IntentPlan {
    #[serde(default)]
    pub steps: Vec<IntentStep>,
}

/// A decoded, ready-to-execute plan step: literal text, or a concrete
/// [`VoiceCommand`] the output router can run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanItem {
    Type(String),
    Action(VoiceCommand),
}

/// Decode the model's JSON into an executable plan.
///
/// - Malformed JSON (or wrong shape) returns `Err` so the caller can degrade.
/// - A `type` step becomes [`PlanItem::Type`] (empty strings are dropped —
///   typing nothing is a no-op).
/// - An `action` step is resolved through [`action_to_command`]; unknown /
///   undecodable action strings are dropped with a logged warning and never
///   panic, so a hallucinated action can't take down the run.
pub fn decode_intent_plan(json: &str) -> AppResult<Vec<PlanItem>> {
    let plan: IntentPlan = serde_json::from_str(json.trim())
        .map_err(|e| AppError::Llm(format!("intent plan JSON parse failed: {e}")))?;

    let mut items = Vec::with_capacity(plan.steps.len());
    for step in plan.steps {
        match step {
            IntentStep::Type(text) => {
                if !text.is_empty() {
                    items.push(PlanItem::Type(text));
                }
            }
            IntentStep::Action(action) => match action_to_command(&action) {
                Some(cmd) => items.push(PlanItem::Action(cmd)),
                None => crate::llm::diaglog::log(&format!(
                    "intent: dropping unknown action string '{action}'"
                )),
            },
        }
    }
    Ok(items)
}

/// Classify a resolved command as *destructive* — one that could discard work,
/// close a window, or spawn a process, and therefore warrants a confirmation
/// gate before the intent layer runs it.
///
/// Rule (small + documented):
///   - `LaunchApp` — spawns an arbitrary process.
///   - `Cut` — removes the current selection.
///   - a `KeyCombo` whose key is `w` with a Ctrl or Meta modifier — the near-
///     universal "close tab / close window" shortcut.
///
/// `ComboKey` cannot express function keys, so Alt+F4 is unreachable today and
/// is intentionally omitted rather than guessed at.  Everything else (typing,
/// navigation, copy, undo, scroll) is treated as non-destructive.
pub fn is_destructive(cmd: &VoiceCommand) -> bool {
    match cmd {
        VoiceCommand::LaunchApp(_) | VoiceCommand::Cut => true,
        VoiceCommand::KeyCombo { modifiers, key } => {
            matches!(key, ComboKey::Char('w') | ComboKey::Char('W'))
                && modifiers
                    .iter()
                    .any(|m| matches!(m, KeyModifier::Ctrl | KeyModifier::Meta))
        }
        _ => false,
    }
}

/// True if any step in the plan is destructive.
pub fn plan_has_destructive(items: &[PlanItem]) -> bool {
    items
        .iter()
        .any(|it| matches!(it, PlanItem::Action(cmd) if is_destructive(cmd)))
}

/// Detect a leading [`INTENT_TRIGGER`] word and return the command text with it
/// stripped.  Returns `None` when the trigger is absent, when it's only part of
/// a longer word ("computerize"), or when nothing follows it (just "computer"),
/// mirroring how `voxify::detect_and_strip_trigger` refuses a bare trigger.
pub fn detect_and_strip_intent_trigger(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if trimmed.len() < INTENT_TRIGGER.len() {
        return None;
    }
    // Guard the byte-index split: a leading multibyte char can't match the
    // ASCII trigger anyway, but `split_at` on a non-boundary would panic.
    if !trimmed.is_char_boundary(INTENT_TRIGGER.len()) {
        return None;
    }
    let (head, tail) = trimmed.split_at(INTENT_TRIGGER.len());
    if !head.eq_ignore_ascii_case(INTENT_TRIGGER) {
        return None;
    }
    // Require a word boundary right after the trigger so "computerize" and
    // "computers" don't arm the path.
    if tail.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    // Drop the separator between the trigger and the command
    // ("computer, open the terminal" -> "open the terminal").
    let rest = tail
        .trim_start_matches(|c: char| {
            c.is_whitespace() || matches!(c, ',' | '.' | ':' | ';' | '!' | '?')
        })
        .to_string();
    if rest.is_empty() {
        return None;
    }
    Some(rest)
}

/// A short, human-readable label for a plan step, used in the confirmation
/// prompt shown to the user.
pub fn describe_plan_item(item: &PlanItem) -> String {
    match item {
        PlanItem::Type(text) => format!("Type \u{201c}{text}\u{201d}"),
        PlanItem::Action(cmd) => describe_command(cmd),
    }
}

fn describe_command(cmd: &VoiceCommand) -> String {
    match cmd {
        VoiceCommand::NewLine => "New line".into(),
        VoiceCommand::NewParagraph => "New paragraph".into(),
        VoiceCommand::DeleteLastWord => "Delete last word".into(),
        VoiceCommand::Send => "Press Enter (send)".into(),
        VoiceCommand::SelectAll => "Select all".into(),
        VoiceCommand::Copy => "Copy".into(),
        VoiceCommand::Cut => "Cut".into(),
        VoiceCommand::Undo => "Undo".into(),
        VoiceCommand::Redo => "Redo".into(),
        VoiceCommand::PressTab => "Press Tab".into(),
        VoiceCommand::PressEscape => "Press Escape".into(),
        VoiceCommand::PressEnter => "Press Enter".into(),
        VoiceCommand::MouseClick => "Mouse click".into(),
        VoiceCommand::MouseRightClick => "Right click".into(),
        VoiceCommand::MouseDoubleClick => "Double click".into(),
        VoiceCommand::ScrollUp => "Scroll up".into(),
        VoiceCommand::ScrollDown => "Scroll down".into(),
        VoiceCommand::KeyCombo { .. } => format!("Key combo ({})", command_to_action(cmd)),
        VoiceCommand::LaunchApp(cmd_line) => format!("Launch: {cmd_line}"),
    }
}

/// Best-effort extraction of the first complete JSON object from a model
/// response that may be wrapped in stray prose or code fences.  Returns the
/// slice from the first `{` to the last `}`.
pub fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end > start {
        Some(&s[start..=end])
    } else {
        None
    }
}

/// Build the Qwen system prompt for intent planning, embedding the allowed
/// action list from [`ALLOWED_ACTIONS`] so the prompt and decoder share one
/// source.  Mirrors the ChatML style of [`crate::llm::prompt`].
pub fn intent_system_prompt() -> String {
    let actions = ALLOWED_ACTIONS.join("\n- ");
    format!(
        "You translate a spoken computer command into a short JSON plan of actions the app will execute.

OUTPUT RULES
- Output exactly one minified JSON object on a single line and nothing else.
- No prose, no markdown, no code fences, no <think> blocks.
- The object has one key: \"steps\", an array of step objects in execution order.
- Each step is EITHER {{\"type\":\"literal text to type\"}} OR {{\"action\":\"ACTION\"}}.
- Use a \"type\" step for words the user wants typed verbatim.
- Use an \"action\" step for a known command. Choose ACTION from this list ONLY:
- {actions}
- For a keyboard shortcut, use {{\"action\":\"key:MOD+MOD+KEY\"}} where MOD is one of ctrl, alt, shift, meta and KEY is a single letter/digit or one of tab, escape, enter, space, backspace. Example: {{\"action\":\"key:ctrl+shift+k\"}}.
- To launch a program, use {{\"action\":\"launch:PROGRAM ARGS\"}} with a plain command line (no shell). Example: {{\"action\":\"launch:notepad\"}}.
- Never invent an action outside these forms. If part of the request has no matching action, drop that part rather than guessing.
- If the request maps to no known action at all, return {{\"steps\":[]}}.

EXAMPLES
Command: \"select all and copy it\"
{{\"steps\":[{{\"action\":\"SelectAll\"}},{{\"action\":\"Copy\"}}]}}
Command: \"type hello world then press enter\"
{{\"steps\":[{{\"type\":\"hello world\"}},{{\"action\":\"PressEnter\"}}]}}
Command: \"open the command palette\"
{{\"steps\":[{{\"action\":\"key:ctrl+shift+p\"}}]}}
Command: \"launch notepad\"
{{\"steps\":[{{\"action\":\"launch:notepad\"}}]}}"
    )
}

/// Wrap the user's command in Qwen's ChatML prompt format for intent planning.
/// `/no_think` disables Qwen3 reasoning traces so they can't leak into the JSON.
pub fn format_intent_prompt(user_text: &str) -> String {
    format!(
        "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\nCommand: {input}\n\nReturn only the JSON plan described above. /no_think<|im_end|>\n<|im_start|>assistant\n",
        system = intent_system_prompt(),
        input = user_text,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── decode_intent_plan ────────────────────────────────────────

    #[test]
    fn decodes_mixed_type_and_actions() {
        let json = r#"{"steps":[{"type":"hello world"},{"action":"SelectAll"},{"action":"Copy"}]}"#;
        let items = decode_intent_plan(json).unwrap();
        assert_eq!(
            items,
            vec![
                PlanItem::Type("hello world".to_string()),
                PlanItem::Action(VoiceCommand::SelectAll),
                PlanItem::Action(VoiceCommand::Copy),
            ]
        );
    }

    #[test]
    fn decodes_key_combo_and_launch_actions() {
        let json = r#"{"steps":[{"action":"key:ctrl+shift+k"},{"action":"launch:notepad foo.txt"}]}"#;
        let items = decode_intent_plan(json).unwrap();
        assert_eq!(
            items,
            vec![
                PlanItem::Action(VoiceCommand::KeyCombo {
                    modifiers: vec![KeyModifier::Ctrl, KeyModifier::Shift],
                    key: ComboKey::Char('k'),
                }),
                PlanItem::Action(VoiceCommand::LaunchApp("notepad foo.txt".to_string())),
            ]
        );
    }

    #[test]
    fn unknown_action_is_skipped_not_fatal() {
        let json = r#"{"steps":[{"action":"Teleport"},{"action":"Copy"},{"action":"key:bogus+"}]}"#;
        let items = decode_intent_plan(json).unwrap();
        // Only the valid Copy survives; the two undecodable actions are dropped.
        assert_eq!(items, vec![PlanItem::Action(VoiceCommand::Copy)]);
    }

    #[test]
    fn empty_type_step_is_dropped() {
        let json = r#"{"steps":[{"type":""},{"action":"Undo"}]}"#;
        let items = decode_intent_plan(json).unwrap();
        assert_eq!(items, vec![PlanItem::Action(VoiceCommand::Undo)]);
    }

    #[test]
    fn empty_plan_decodes_to_empty_vec() {
        assert_eq!(decode_intent_plan(r#"{"steps":[]}"#).unwrap(), vec![]);
        // Missing "steps" key defaults to empty as well.
        assert_eq!(decode_intent_plan(r#"{}"#).unwrap(), vec![]);
    }

    #[test]
    fn malformed_json_is_error() {
        assert!(decode_intent_plan("not json at all").is_err());
        assert!(decode_intent_plan(r#"{"steps": [ {"type": "#).is_err());
    }

    // ── is_destructive ────────────────────────────────────────────

    #[test]
    fn launch_and_cut_are_destructive() {
        assert!(is_destructive(&VoiceCommand::LaunchApp("rm".to_string())));
        assert!(is_destructive(&VoiceCommand::Cut));
    }

    #[test]
    fn ctrl_w_and_meta_w_are_destructive() {
        assert!(is_destructive(&VoiceCommand::KeyCombo {
            modifiers: vec![KeyModifier::Ctrl],
            key: ComboKey::Char('w'),
        }));
        assert!(is_destructive(&VoiceCommand::KeyCombo {
            modifiers: vec![KeyModifier::Meta],
            key: ComboKey::Char('W'),
        }));
    }

    #[test]
    fn benign_commands_and_combos_are_not_destructive() {
        assert!(!is_destructive(&VoiceCommand::Copy));
        assert!(!is_destructive(&VoiceCommand::SelectAll));
        assert!(!is_destructive(&VoiceCommand::NewLine));
        // w without a Ctrl/Meta modifier is just typing a shortcut, not close.
        assert!(!is_destructive(&VoiceCommand::KeyCombo {
            modifiers: vec![KeyModifier::Shift],
            key: ComboKey::Char('w'),
        }));
        // Ctrl+S (save) is not in our destructive set.
        assert!(!is_destructive(&VoiceCommand::KeyCombo {
            modifiers: vec![KeyModifier::Ctrl],
            key: ComboKey::Char('s'),
        }));
    }

    #[test]
    fn plan_has_destructive_flags_any_step() {
        let safe = vec![
            PlanItem::Type("hi".to_string()),
            PlanItem::Action(VoiceCommand::Copy),
        ];
        assert!(!plan_has_destructive(&safe));
        let dangerous = vec![
            PlanItem::Type("hi".to_string()),
            PlanItem::Action(VoiceCommand::Cut),
        ];
        assert!(plan_has_destructive(&dangerous));
    }

    // ── detect_and_strip_intent_trigger ───────────────────────────

    #[test]
    fn strips_leading_trigger() {
        assert_eq!(
            detect_and_strip_intent_trigger("Computer, open the terminal").as_deref(),
            Some("open the terminal")
        );
        assert_eq!(
            detect_and_strip_intent_trigger("computer select all").as_deref(),
            Some("select all")
        );
    }

    #[test]
    fn trigger_absent_returns_none() {
        assert_eq!(detect_and_strip_intent_trigger("open the terminal"), None);
    }

    #[test]
    fn bare_trigger_is_not_a_command() {
        assert_eq!(detect_and_strip_intent_trigger("computer"), None);
        assert_eq!(detect_and_strip_intent_trigger("Computer."), None);
        assert_eq!(detect_and_strip_intent_trigger("  computer  "), None);
    }

    #[test]
    fn trigger_inside_word_does_not_match() {
        assert_eq!(detect_and_strip_intent_trigger("computerize the report"), None);
        assert_eq!(detect_and_strip_intent_trigger("computers are great"), None);
    }

    #[test]
    fn multibyte_leading_char_does_not_panic() {
        // A leading accented char is shorter/longer in bytes than the ASCII
        // trigger; must return None without panicking on a byte split.
        assert_eq!(detect_and_strip_intent_trigger("café open"), None);
    }

    // ── shared list / prompt integrity ────────────────────────────

    #[test]
    fn every_allowed_action_decodes() {
        for action in ALLOWED_ACTIONS {
            assert!(
                action_to_command(action).is_some(),
                "allowed action '{action}' must decode via action_to_command"
            );
        }
    }

    #[test]
    fn prompt_lists_the_allowed_actions() {
        let prompt = intent_system_prompt();
        for action in ALLOWED_ACTIONS {
            assert!(prompt.contains(action), "prompt missing action '{action}'");
        }
    }

    // ── extract_json_object ───────────────────────────────────────

    #[test]
    fn extracts_object_from_wrapped_text() {
        let raw = "Sure! ```json\n{\"steps\":[]}\n``` done";
        assert_eq!(extract_json_object(raw), Some("{\"steps\":[]}"));
    }

    #[test]
    fn extract_json_object_none_when_absent() {
        assert_eq!(extract_json_object("no braces here"), None);
    }
}
