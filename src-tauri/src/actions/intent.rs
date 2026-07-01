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
}

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
            "play_pause" => CommandIntent::Media(MediaAction::PlayPause),
            "next_track" => CommandIntent::Media(MediaAction::NextTrack),
            "prev_track" => CommandIntent::Media(MediaAction::PrevTrack),
            "mute" => CommandIntent::Media(MediaAction::Mute),
            "volume_up" => CommandIntent::Media(MediaAction::VolumeUp),
            "volume_down" => CommandIntent::Media(MediaAction::VolumeDown),
            "minimize" => CommandIntent::Window(WindowAction::Minimize),
            "maximize" => CommandIntent::Window(WindowAction::Maximize),
            "close_window" => CommandIntent::CloseWindow,
            "web_search" => CommandIntent::WebSearch(target.trim().to_string()),
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
    }

    #[test]
    fn from_llm_rejects_none_unknown_and_empty_target() {
        assert_eq!(CommandIntent::from_llm("none", ""), None);
        assert_eq!(CommandIntent::from_llm("open_app", "   "), None);
        assert_eq!(CommandIntent::from_llm("open_url", ""), None);
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
