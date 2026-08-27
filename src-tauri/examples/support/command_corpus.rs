//! Fixed Command Mode corpus shared by the fair LLM and candidate router runs.

pub struct CommandExpectation {
    pub action: &'static str,
    pub target: &'static str,
}

pub struct CommandCase {
    pub utterance: &'static str,
    /// Empty means that Command Mode must decline the utterance.
    pub expected: &'static [CommandExpectation],
}

macro_rules! step {
    ($action:literal, $target:literal) => {
        CommandExpectation {
            action: $action,
            target: $target,
        }
    };
}

pub const COMMAND_CASES: &[CommandCase] = &[
    CommandCase {
        utterance: "open spotify",
        expected: &[step!("open_app", "spotify")],
    },
    CommandCase {
        utterance: "bring up chrome",
        expected: &[step!("open_app", "chrome")],
    },
    CommandCase {
        utterance: "i want to listen to some music",
        expected: &[step!("open_app", "spotify")],
    },
    CommandCase {
        utterance: "fire up discord",
        expected: &[step!("open_app", "discord")],
    },
    CommandCase {
        utterance: "can you launch visual studio code for me",
        expected: &[step!("open_app", "visual studio code")],
    },
    CommandCase {
        utterance: "pull up my email",
        expected: &[step!("open_app", "outlook")],
    },
    CommandCase {
        utterance: "make a google search",
        expected: &[step!("web_search", "")],
    },
    CommandCase {
        utterance: "search for the best pizza in chicago",
        expected: &[step!("web_search", "best pizza in chicago")],
    },
    CommandCase {
        utterance: "look up the weather in denver",
        expected: &[step!("web_search", "weather in denver")],
    },
    CommandCase {
        utterance: "google how to tie a tie",
        expected: &[step!("web_search", "how to tie a tie")],
    },
    CommandCase {
        utterance: "go to youtube",
        expected: &[step!("open_url", "youtube.com")],
    },
    CommandCase {
        utterance: "open the github website",
        expected: &[step!("open_url", "github.com")],
    },
    CommandCase {
        utterance: "copy that",
        expected: &[step!("copy", "")],
    },
    CommandCase {
        utterance: "paste it here",
        expected: &[step!("paste", "")],
    },
    CommandCase {
        utterance: "undo",
        expected: &[step!("undo", "")],
    },
    CommandCase {
        utterance: "select everything",
        expected: &[step!("select_all", "")],
    },
    CommandCase {
        utterance: "save the file",
        expected: &[step!("save", "")],
    },
    CommandCase {
        utterance: "open a new tab",
        expected: &[step!("new_tab", "")],
    },
    CommandCase {
        utterance: "close this tab",
        expected: &[step!("close_tab", "")],
    },
    CommandCase {
        utterance: "take a screenshot",
        expected: &[step!("screenshot", "")],
    },
    CommandCase {
        utterance: "show me the desktop",
        expected: &[step!("show_desktop", "")],
    },
    CommandCase {
        utterance: "minimize everything",
        expected: &[step!("show_desktop", "")],
    },
    CommandCase {
        utterance: "hide all my windows",
        expected: &[step!("show_desktop", "")],
    },
    CommandCase {
        utterance: "close this window",
        expected: &[step!("close_window", "")],
    },
    CommandCase {
        utterance: "close this window please",
        expected: &[step!("close_window", "")],
    },
    CommandCase {
        utterance: "get this window off my screen",
        expected: &[step!("close_window", "")],
    },
    CommandCase {
        utterance: "turn it down",
        expected: &[step!("volume_down", "")],
    },
    CommandCase {
        utterance: "turn the volume up",
        expected: &[step!("volume_up", "")],
    },
    CommandCase {
        utterance: "skip this song",
        expected: &[step!("next_track", "")],
    },
    CommandCase {
        utterance: "mute it",
        expected: &[step!("mute", "")],
    },
    CommandCase {
        utterance: "pause the music",
        expected: &[step!("play_pause", "")],
    },
    CommandCase {
        utterance: "shrink this window",
        expected: &[step!("minimize", "")],
    },
    CommandCase {
        utterance: "make this full screen",
        expected: &[step!("maximize", "")],
    },
    // Ordered multi-step chains: missing, extra, or reordered steps are wrong.
    CommandCase {
        utterance: "copy and then paste and save",
        expected: &[step!("copy", ""), step!("paste", ""), step!("save", "")],
    },
    CommandCase {
        utterance: "copy and paste and save and undo",
        expected: &[
            step!("copy", ""),
            step!("paste", ""),
            step!("save", ""),
            step!("undo", ""),
        ],
    },
    CommandCase {
        utterance: "mute and minimize this window",
        expected: &[step!("mute", ""), step!("minimize", "")],
    },
    // Non-commands and deliberately unsupported commands.
    CommandCase {
        utterance: "what time is it",
        expected: &[],
    },
    CommandCase {
        utterance: "close internet explorer",
        expected: &[],
    },
    CommandCase {
        utterance: "quit photoshop",
        expected: &[],
    },
    CommandCase {
        utterance: "the quick brown fox jumps over the lazy dog",
        expected: &[],
    },
    CommandCase {
        utterance: "remind me to call my mom at five",
        expected: &[],
    },
];

/// Distinct from every measured case to avoid immediate repeat/order bias.
pub const COMMAND_WARMUP_UTTERANCES: &[&str] = &[
    "please turn the sound off",
    "could you select all of this",
    "please open a fresh tab",
];
