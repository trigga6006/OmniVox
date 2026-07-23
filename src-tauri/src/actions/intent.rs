//! The closed action vocabulary for Command Mode.
//!
//! Every command the app can perform is one of these variants.  Keeping the set
//! closed (and code-defined) is deliberate: it is the safety boundary that makes
//! it impossible for a misheard phrase — or, later, an LLM fallback — to produce
//! an action the developer never enumerated.

/// A keyboard chord fired into the foreground app via `enigo`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyChord {
    Copy,
    Paste,
    Cut,
    Undo,
    Redo,
    SelectAll,
    Save,
    NewTab,
    CloseTab,
    /// Win+Shift+S on Windows (region snip).
    Screenshot,
    /// Win+D on Windows — minimize everything / show the desktop (reversible).
    ShowDesktop,
    /// F5 — reload the current page / view.
    Refresh,
    /// Ctrl+F — open the in-page / in-app find bar.
    Find,
    /// Ctrl+N — open a new window.
    NewWindow,
    /// Ctrl+Shift+T — reopen the last closed tab.
    ReopenTab,
    /// Ctrl+Tab — switch to the next tab.
    NextTab,
    /// Ctrl+Shift+Tab — switch to the previous tab.
    PrevTab,
    /// Page Down — scroll down one page.
    PageDown,
    /// Page Up — scroll up one page.
    PageUp,
}

impl KeyChord {
    /// Human-readable label for pill feedback ("Copied", "Pasted", …).
    pub fn past_tense(self) -> &'static str {
        match self {
            KeyChord::Copy => "Copied",
            KeyChord::Paste => "Pasted",
            KeyChord::Cut => "Cut",
            KeyChord::Undo => "Undid",
            KeyChord::Redo => "Redid",
            KeyChord::SelectAll => "Selected all",
            KeyChord::Save => "Saved",
            KeyChord::NewTab => "New tab",
            KeyChord::CloseTab => "Closed tab",
            KeyChord::Screenshot => "Screenshot",
            KeyChord::ShowDesktop => "Showed desktop",
            KeyChord::Refresh => "Refreshed",
            KeyChord::Find => "Opened find",
            KeyChord::NewWindow => "New window",
            KeyChord::ReopenTab => "Reopened tab",
            KeyChord::NextTab => "Next tab",
            KeyChord::PrevTab => "Previous tab",
            KeyChord::PageDown => "Page down",
            KeyChord::PageUp => "Page up",
        }
    }
}

/// A media-transport / volume key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaAction {
    PlayPause,
    NextTrack,
    PrevTrack,
    Mute,
    /// Explicit un-mute (distinct from the blind VK_VOLUME_MUTE toggle) — set
    /// deterministically via WASAPI so "unmute"/"sound on" never silences audio
    /// that's already on. Produced by the deterministic matcher only.
    Unmute,
    VolumeUp,
    VolumeDown,
}

impl MediaAction {
    pub fn label(self) -> &'static str {
        match self {
            MediaAction::PlayPause => "Play/Pause",
            MediaAction::NextTrack => "Next track",
            MediaAction::PrevTrack => "Previous track",
            MediaAction::Mute => "Mute",
            MediaAction::Unmute => "Unmute",
            MediaAction::VolumeUp => "Volume up",
            MediaAction::VolumeDown => "Volume down",
        }
    }
}

/// A foreground-window action.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowAction {
    Minimize,
    Maximize,
}

impl WindowAction {
    pub fn label(self) -> &'static str {
        match self {
            WindowAction::Minimize => "Minimized",
            WindowAction::Maximize => "Maximized",
        }
    }
}

/// An action on OmniVox's own scratchpad window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScratchpadAction {
    Open,
    Close,
    /// Wipe the saved cards + note. Destructive, so the pipeline routes it
    /// through the confirm pill before executing.
    Clear,
}

/// A resolved command intent — the closed action enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandIntent {
    /// Launch (or activate) an app by its *spoken* name (not yet resolved to an
    /// AppsFolder entry — resolution happens in the pipeline).
    OpenApp(String),
    KeyChord(KeyChord),
    Media(MediaAction),
    Window(WindowAction),
    /// Open a web/Google search in the user's DEFAULT browser. The string is the
    /// query (may be empty → just open the search page).
    WebSearch(String),
    /// Open a URL / website in the user's DEFAULT browser.
    OpenUrl(String),
    /// Close the current foreground window (graceful WM_CLOSE — the app runs its
    /// own save-prompt). Consequential, so the pipeline routes it through the
    /// confirm pill before executing.
    CloseWindow,
    /// Type text into the target window via the clipboard-verified paste path.
    /// `submit: false` just leaves the text there (harmless — the user reviews
    /// and presses Enter themselves).  `submit: true` also presses Enter to
    /// send it — consequential, so the pipeline routes any intent/chain
    /// containing one through the confirm pill before executing.
    TypeText { text: String, submit: bool },
    /// Act on OmniVox's own scratchpad window.  Executed in the pipeline (it
    /// needs the Tauri `AppHandle`; the executor is OS-only).
    Scratchpad(ScratchpadAction),
}

/// The canonical LLM action vocabulary — SINGLE SOURCE OF TRUTH for the
/// name set that is otherwise triplicated across `from_llm` below, the
/// GBNF grammar (`resources/grammars/command_intent_v1.gbnf`), and the
/// prompt's ACTIONS section (`llm::prompt::COMMAND_SYSTEM_PROMPT`).
/// Tests in this module assert all three stay in sync — adding a verb
/// without updating every surface fails the build's test step.
pub const ACTION_NAMES: &[&str] = &[
    "open_app",
    "focus_app",
    "web_search",
    "open_url",
    "type_text",
    "send_message",
    "copy",
    "paste",
    "cut",
    "undo",
    "redo",
    "select_all",
    "save",
    "new_tab",
    "close_tab",
    "screenshot",
    "show_desktop",
    "refresh",
    "find_in_page",
    "new_window",
    "reopen_tab",
    "next_tab",
    "prev_tab",
    "page_down",
    "page_up",
    "play_pause",
    "next_track",
    "prev_track",
    "mute",
    "volume_up",
    "volume_down",
    "minimize",
    "maximize",
    "close_window",
    "open_scratchpad",
    "close_scratchpad",
    "clear_scratchpad",
    "none",
];

impl CommandIntent {
    /// Map the LLM fallback's `{action, target}` output (snake_case action enum
    /// from `command_intent_v1.gbnf`) into a `CommandIntent`. Returns `None` for
    /// `"none"`, an unknown action, or an `open_app`/`focus_app` with no target.
    ///
    /// The grammar already restricts `action` to this closed set, so the
    /// wildcard arm is just defense-in-depth.
    pub fn from_llm(action: &str, target: &str) -> Option<CommandIntent> {
        let intent = match action {
            "open_app" | "focus_app" => {
                let t = target.trim();
                if t.is_empty() {
                    return None;
                }
                CommandIntent::OpenApp(t.to_string())
            }
            "copy" => CommandIntent::KeyChord(KeyChord::Copy),
            "paste" => CommandIntent::KeyChord(KeyChord::Paste),
            "cut" => CommandIntent::KeyChord(KeyChord::Cut),
            "undo" => CommandIntent::KeyChord(KeyChord::Undo),
            "redo" => CommandIntent::KeyChord(KeyChord::Redo),
            "select_all" => CommandIntent::KeyChord(KeyChord::SelectAll),
            "save" => CommandIntent::KeyChord(KeyChord::Save),
            "new_tab" => CommandIntent::KeyChord(KeyChord::NewTab),
            "close_tab" => CommandIntent::KeyChord(KeyChord::CloseTab),
            "screenshot" => CommandIntent::KeyChord(KeyChord::Screenshot),
            "show_desktop" => CommandIntent::KeyChord(KeyChord::ShowDesktop),
            "refresh" => CommandIntent::KeyChord(KeyChord::Refresh),
            "find_in_page" => CommandIntent::KeyChord(KeyChord::Find),
            "new_window" => CommandIntent::KeyChord(KeyChord::NewWindow),
            "reopen_tab" => CommandIntent::KeyChord(KeyChord::ReopenTab),
            "next_tab" => CommandIntent::KeyChord(KeyChord::NextTab),
            "prev_tab" => CommandIntent::KeyChord(KeyChord::PrevTab),
            "page_down" => CommandIntent::KeyChord(KeyChord::PageDown),
            "page_up" => CommandIntent::KeyChord(KeyChord::PageUp),
            "play_pause" => CommandIntent::Media(MediaAction::PlayPause),
            "next_track" => CommandIntent::Media(MediaAction::NextTrack),
            "prev_track" => CommandIntent::Media(MediaAction::PrevTrack),
            "mute" => CommandIntent::Media(MediaAction::Mute),
            "volume_up" => CommandIntent::Media(MediaAction::VolumeUp),
            "volume_down" => CommandIntent::Media(MediaAction::VolumeDown),
            "minimize" => CommandIntent::Window(WindowAction::Minimize),
            "maximize" => CommandIntent::Window(WindowAction::Maximize),
            "close_window" => CommandIntent::CloseWindow,
            "open_scratchpad" => CommandIntent::Scratchpad(ScratchpadAction::Open),
            "close_scratchpad" => CommandIntent::Scratchpad(ScratchpadAction::Close),
            "clear_scratchpad" => CommandIntent::Scratchpad(ScratchpadAction::Clear),
            "web_search" => CommandIntent::WebSearch(target.trim().to_string()),
            "type_text" | "send_message" => {
                let t = target.trim();
                if t.is_empty() {
                    return None;
                }
                CommandIntent::TypeText {
                    text: t.to_string(),
                    submit: action == "send_message",
                }
            }
            "open_url" => {
                let t = target.trim();
                if t.is_empty() {
                    return None;
                }
                CommandIntent::OpenUrl(t.to_string())
            }
            _ => return None,
        };
        Some(intent)
    }

    /// Parse the LLM fallback's JSON array of `{action, target}` objects into an
    /// ordered sequence of intents.
    ///
    /// All-or-nothing: if *any* object fails to map — a `"none"` (the model's
    /// "not a command" signal), an unknown action, or an empty target where one
    /// is required — the whole utterance is treated as unrecognized and an empty
    /// Vec is returned.  This deliberately avoids running a *partial* chain (e.g.
    /// "open spotify and close internet explorer" silently opening Spotify while
    /// dropping the unsupported close) and reporting it as success.  An empty Vec
    /// is also returned if the JSON doesn't parse.
    pub fn from_llm_list(json: &str) -> Vec<CommandIntent> {
        #[derive(serde::Deserialize)]
        struct Raw {
            action: String,
            #[serde(default)]
            target: String,
        }
        let raws = match serde_json::from_str::<Vec<Raw>>(json.trim()) {
            Ok(raws) => raws,
            Err(_) => return Vec::new(),
        };
        let mut intents = Vec::with_capacity(raws.len());
        for r in &raws {
            match CommandIntent::from_llm(&r.action, &r.target) {
                Some(intent) => intents.push(intent),
                None => return Vec::new(),
            }
        }
        intents
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_llm_maps_actions() {
        assert_eq!(
            CommandIntent::from_llm("open_app", "Spotify"),
            Some(CommandIntent::OpenApp("Spotify".into()))
        );
        assert_eq!(
            CommandIntent::from_llm("focus_app", "Chrome"),
            Some(CommandIntent::OpenApp("Chrome".into()))
        );
        assert_eq!(
            CommandIntent::from_llm("volume_down", ""),
            Some(CommandIntent::Media(MediaAction::VolumeDown))
        );
        assert_eq!(
            CommandIntent::from_llm("minimize", ""),
            Some(CommandIntent::Window(WindowAction::Minimize))
        );
        assert_eq!(
            CommandIntent::from_llm("copy", ""),
            Some(CommandIntent::KeyChord(KeyChord::Copy))
        );
        assert_eq!(
            CommandIntent::from_llm("web_search", "cats"),
            Some(CommandIntent::WebSearch("cats".into()))
        );
        assert_eq!(
            CommandIntent::from_llm("web_search", ""),
            Some(CommandIntent::WebSearch(String::new()))
        );
        assert_eq!(
            CommandIntent::from_llm("open_url", "youtube.com"),
            Some(CommandIntent::OpenUrl("youtube.com".into()))
        );
        assert_eq!(
            CommandIntent::from_llm("show_desktop", ""),
            Some(CommandIntent::KeyChord(KeyChord::ShowDesktop))
        );
        assert_eq!(
            CommandIntent::from_llm("close_window", ""),
            Some(CommandIntent::CloseWindow)
        );
        assert_eq!(
            CommandIntent::from_llm("open_scratchpad", ""),
            Some(CommandIntent::Scratchpad(ScratchpadAction::Open))
        );
        assert_eq!(
            CommandIntent::from_llm("close_scratchpad", ""),
            Some(CommandIntent::Scratchpad(ScratchpadAction::Close))
        );
        assert_eq!(
            CommandIntent::from_llm("clear_scratchpad", ""),
            Some(CommandIntent::Scratchpad(ScratchpadAction::Clear))
        );
        assert_eq!(
            CommandIntent::from_llm("type_text", "hello world"),
            Some(CommandIntent::TypeText {
                text: "hello world".into(),
                submit: false
            })
        );
        assert_eq!(
            CommandIntent::from_llm("send_message", "fix the login bug"),
            Some(CommandIntent::TypeText {
                text: "fix the login bug".into(),
                submit: true
            })
        );
    }

    #[test]
    fn from_llm_rejects_none_unknown_and_empty_target() {
        assert_eq!(CommandIntent::from_llm("none", ""), None);
        assert_eq!(CommandIntent::from_llm("open_app", "   "), None);
        assert_eq!(CommandIntent::from_llm("open_url", ""), None);
        assert_eq!(CommandIntent::from_llm("type_text", "  "), None);
        assert_eq!(CommandIntent::from_llm("send_message", ""), None);
        assert_eq!(CommandIntent::from_llm("bogus_action", "x"), None);
    }

    #[test]
    fn from_llm_list_parses_clean_chain() {
        let json = r#"[{"action":"open_app","target":"Spotify"},{"action":"play_pause","target":""}]"#;
        assert_eq!(
            CommandIntent::from_llm_list(json),
            vec![
                CommandIntent::OpenApp("Spotify".into()),
                CommandIntent::Media(MediaAction::PlayPause),
            ]
        );
    }

    #[test]
    fn from_llm_list_is_all_or_nothing_on_invalid_entry() {
        // A "none" (or any unsupported step) mixed into an otherwise-valid chain
        // rejects the WHOLE utterance rather than silently running a partial.
        let json = r#"[{"action":"open_app","target":"Spotify"},{"action":"none","target":""}]"#;
        assert!(CommandIntent::from_llm_list(json).is_empty());
    }

    /// Extract the quoted action names from the grammar's `action ::=` rule.
    fn grammar_action_names() -> Vec<String> {
        let gbnf = include_str!("../../resources/grammars/command_intent_v1.gbnf");
        let line = gbnf
            .lines()
            .find(|l| l.trim_start().starts_with("action ::="))
            .expect("grammar has an action rule");
        // Alternatives look like  "\"open_app\""  — pull the inner name.
        line.split('|')
            .filter_map(|alt| {
                let alt = alt.trim().trim_start_matches("action ::=").trim();
                alt.strip_prefix("\"\\\"")
                    .and_then(|s| s.strip_suffix("\\\"\""))
                    .map(str::to_string)
            })
            .collect()
    }

    #[test]
    fn grammar_matches_canonical_action_table() {
        let mut grammar: Vec<String> = grammar_action_names();
        let mut canonical: Vec<String> = ACTION_NAMES.iter().map(|s| s.to_string()).collect();
        grammar.sort();
        canonical.sort();
        assert_eq!(
            grammar, canonical,
            "command_intent_v1.gbnf action set drifted from intent::ACTION_NAMES"
        );
    }

    #[test]
    fn from_llm_accepts_exactly_the_canonical_table() {
        for name in ACTION_NAMES {
            if *name == "none" {
                assert_eq!(CommandIntent::from_llm(name, "x"), None);
                continue;
            }
            // A non-empty target satisfies the actions that require one and is
            // ignored by the rest.
            assert!(
                CommandIntent::from_llm(name, "some target").is_some(),
                "canonical action '{name}' is not handled by from_llm"
            );
        }
        assert_eq!(CommandIntent::from_llm("not_an_action", "x"), None);
    }

    #[test]
    fn prompt_documents_every_canonical_action() {
        let prompt = crate::llm::prompt::COMMAND_SYSTEM_PROMPT;
        for name in ACTION_NAMES {
            assert!(
                prompt.contains(name),
                "COMMAND_SYSTEM_PROMPT does not mention action '{name}'"
            );
        }
    }

    #[test]
    fn from_llm_list_handles_single_and_garbage() {
        assert_eq!(
            CommandIntent::from_llm_list(r#"[{"action":"copy","target":""}]"#),
            vec![CommandIntent::KeyChord(KeyChord::Copy)]
        );
        assert!(CommandIntent::from_llm_list(r#"[{"action":"none","target":""}]"#).is_empty());
        assert!(CommandIntent::from_llm_list("not json").is_empty());
        assert!(CommandIntent::from_llm_list("[]").is_empty());
    }
}
