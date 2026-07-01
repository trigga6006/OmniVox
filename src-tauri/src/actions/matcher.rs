//! Whole-utterance command matcher.
//!
//! In Command Mode the entire spoken utterance is one command — unlike the
//! inline dictation parser (`postprocess::voice_commands`), which splits mixed
//! text + formatting commands.  This keeps matching simple and safe: normalize
//! the utterance, try the closed table of zero-argument commands, then the
//! `<verb> <app>` open-app prefixes.  No match → `None` (a later phase can route
//! that to the LLM; for now the pill reports "didn't catch a command").

use crate::actions::intent::{CommandIntent, KeyChord, MediaAction, WindowAction};

/// Verbs/phrases that introduce an "open app" command; the remainder is the app
/// name.  Multi-word verbs ("switch to") are matched whole.  Kept generous so
/// natural phrasings ("bring up Spotify", "pull up Chrome") resolve without
/// needing the LLM fallback.
const OPEN_VERBS: &[&str] = &[
    "switch to",
    "take me to",
    "bring up",
    "pull up",
    "fire up",
    "get me",
    "show me",
    "jump to",
    "go to",
    "open up",
    "open",
    "launch",
    "start",
    "run",
    "focus",
];

/// Lowercase, fold every non-alphanumeric run to a single space, trim.
/// "Open Spotify."  → "open spotify";  "Select All!" → "select all".
pub fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let spaced: String = lowered
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Exact-match table of zero-argument commands (input already normalized).
fn zero_arg(norm: &str) -> Option<CommandIntent> {
    use CommandIntent::{KeyChord as Kc, Media, Window};
    let intent = match norm {
        "copy" => Kc(KeyChord::Copy),
        "paste" => Kc(KeyChord::Paste),
        "cut" => Kc(KeyChord::Cut),
        "undo" => Kc(KeyChord::Undo),
        "redo" => Kc(KeyChord::Redo),
        "select all" | "highlight all" => Kc(KeyChord::SelectAll),
        "save" => Kc(KeyChord::Save),
        "new tab" => Kc(KeyChord::NewTab),
        "close tab" => Kc(KeyChord::CloseTab),
        "screenshot" | "take a screenshot" | "take screenshot" | "snip" => Kc(KeyChord::Screenshot),
        "play" | "pause" | "play pause" | "resume" => Media(MediaAction::PlayPause),
        "next track" | "next song" | "skip track" | "skip" => Media(MediaAction::NextTrack),
        "previous track" | "previous song" | "last track" => Media(MediaAction::PrevTrack),
        "mute" | "unmute" => Media(MediaAction::Mute),
        "volume up" | "louder" => Media(MediaAction::VolumeUp),
        "volume down" | "quieter" => Media(MediaAction::VolumeDown),
        "minimize" | "minimise" | "minimize window" => Window(WindowAction::Minimize),
        "maximize" | "maximise" | "maximize window" | "full screen" => {
            Window(WindowAction::Maximize)
        }
        "show desktop" | "show the desktop" | "minimize everything" | "minimise everything"
        | "minimize all" | "hide everything" => Kc(KeyChord::ShowDesktop),
        "close window" | "close this window" | "close the window" | "close current window" => {
            CommandIntent::CloseWindow
        }
        _ => return None,
    };
    Some(intent)
}

/// Parse a Command-Mode utterance into a [`CommandIntent`].
///
/// Zero-arg commands are tried first so fixed phrases ("new tab") win over the
/// open-verb prefixes ("open …").
pub fn match_command(utterance: &str) -> Option<CommandIntent> {
    let norm = normalize(utterance);
    if norm.is_empty() {
        return None;
    }

    if let Some(intent) = zero_arg(&norm) {
        return Some(intent);
    }

    for verb in OPEN_VERBS {
        if let Some(rest) = norm.strip_prefix(verb) {
            // Require a word boundary so "opener" / "started" don't match.
            if let Some(target) = rest.strip_prefix(' ') {
                let target = target.trim();
                if !target.is_empty() {
                    // A conjunction means this is probably a multi-step chain
                    // ("open spotify and play") — defer to the LLM so it splits
                    // into ordered intents instead of treating the whole tail as
                    // one (garbage) app name like "spotify and play".
                    if target.contains(" and ") || target.contains(" then ") {
                        return None;
                    }
                    return Some(CommandIntent::OpenApp(target.to_string()));
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_app_strips_verb_and_punctuation() {
        assert_eq!(
            match_command("Open Spotify."),
            Some(CommandIntent::OpenApp("spotify".into()))
        );
        assert_eq!(
            match_command("launch Visual Studio Code"),
            Some(CommandIntent::OpenApp("visual studio code".into()))
        );
        assert_eq!(
            match_command("switch to Chrome"),
            Some(CommandIntent::OpenApp("chrome".into()))
        );
        assert_eq!(
            match_command("go to Discord"),
            Some(CommandIntent::OpenApp("discord".into()))
        );
    }

    #[test]
    fn natural_open_synonyms() {
        for (utter, app) in [
            ("bring up Spotify", "spotify"),
            ("pull up Chrome", "chrome"),
            ("fire up Discord", "discord"),
            ("take me to Slack", "slack"),
            ("jump to Notion", "notion"),
        ] {
            assert_eq!(
                match_command(utter),
                Some(CommandIntent::OpenApp(app.into())),
                "failed for: {utter}"
            );
        }
    }

    #[test]
    fn open_verb_needs_a_target_and_boundary() {
        assert_eq!(match_command("open"), None);
        assert_eq!(match_command("open   "), None);
        // Word boundary: "opening" must NOT match the "open" verb.
        assert_eq!(match_command("opening the door"), None);
    }

    #[test]
    fn zero_arg_chords() {
        assert_eq!(match_command("copy"), Some(CommandIntent::KeyChord(KeyChord::Copy)));
        assert_eq!(
            match_command("Select All!"),
            Some(CommandIntent::KeyChord(KeyChord::SelectAll))
        );
        assert_eq!(match_command("new tab"), Some(CommandIntent::KeyChord(KeyChord::NewTab)));
        assert_eq!(
            match_command("screenshot"),
            Some(CommandIntent::KeyChord(KeyChord::Screenshot))
        );
    }

    #[test]
    fn zero_arg_media_and_window() {
        assert_eq!(match_command("play"), Some(CommandIntent::Media(MediaAction::PlayPause)));
        assert_eq!(match_command("mute"), Some(CommandIntent::Media(MediaAction::Mute)));
        assert_eq!(
            match_command("minimize window"),
            Some(CommandIntent::Window(WindowAction::Minimize))
        );
    }

    #[test]
    fn zero_arg_show_desktop_and_close_window() {
        assert_eq!(
            match_command("show desktop"),
            Some(CommandIntent::KeyChord(KeyChord::ShowDesktop))
        );
        assert_eq!(
            match_command("minimize everything"),
            Some(CommandIntent::KeyChord(KeyChord::ShowDesktop))
        );
        assert_eq!(match_command("close this window"), Some(CommandIntent::CloseWindow));
        assert_eq!(match_command("close window"), Some(CommandIntent::CloseWindow));
        // "close tab" must stay the tab chord, never CloseWindow.
        assert_eq!(
            match_command("close tab"),
            Some(CommandIntent::KeyChord(KeyChord::CloseTab))
        );
    }

    #[test]
    fn zero_arg_wins_over_open_verb() {
        // "new tab" must be the chord, never OpenApp("tab").
        assert_eq!(match_command("new tab"), Some(CommandIntent::KeyChord(KeyChord::NewTab)));
    }

    #[test]
    fn non_commands_return_none() {
        assert_eq!(match_command(""), None);
        assert_eq!(match_command("hello world this is dictation"), None);
    }

    #[test]
    fn open_with_conjunction_defers_to_llm() {
        // "open X and Y" is a multi-step chain — the matcher must NOT swallow the
        // whole tail as one app name; it returns None so the LLM splits it.
        assert_eq!(match_command("open spotify and play"), None);
        assert_eq!(match_command("open chrome then minimize"), None);
        // A plain single open still resolves through the matcher.
        assert_eq!(
            match_command("open spotify"),
            Some(CommandIntent::OpenApp("spotify".into()))
        );
    }
}
