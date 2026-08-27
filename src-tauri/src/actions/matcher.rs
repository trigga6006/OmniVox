//! Whole-utterance command matcher.
//!
//! In Command Mode the entire spoken utterance is one command — unlike the
//! inline dictation parser (`postprocess::voice_commands`), which splits mixed
//! text + formatting commands.  This keeps matching simple and safe: normalize
//! the utterance, try the closed table of zero-argument commands, then the
//! `<verb> <app>` open-app prefixes.  No match → `None` (a later phase can route
//! that to the LLM; for now the pill reports "didn't catch a command").

use crate::actions::intent::{
    CommandIntent, KeyChord, MediaAction, ScratchpadAction, WindowAction,
};

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

/// Politeness prefixes peeled off the front before the zero-arg lookup
/// ("can you skip the song" → "skip the song").  Multi-word entries match whole.
const LEADING_FILLERS: &[&str] = &[
    "please",
    "hey",
    "can you",
    "could you",
    "would you",
    "will you",
];

/// Politeness suffixes peeled off the end ("skip the song please" → "skip the
/// song", "close it for me" → "close it").
const TRAILING_FILLERS: &[&str] = &["please", "for me", "thanks", "thank you"];

/// Determiner/pronoun filler tokens dropped anywhere ("close the window" →
/// "close window", "turn it up" → "turn up").
const FILLER_TOKENS: &[&str] = &["the", "a", "an", "this", "that", "it", "my"];

/// Peel leading and trailing politeness wrapping off a normalized utterance
/// ("please open notepad" → "open notepad", "open notepad please" → "open
/// notepad").  Repeats on each end until no more match.  Leaves interior
/// tokens untouched — safe to run ahead of the open-app path so articles in
/// app names survive ("open the settings" → "open the settings").
pub(crate) fn peel_politeness(s: &str) -> String {
    let mut cur = s;
    // Peel leading politeness ("can you please …"), repeating until none match.
    'lead: loop {
        for p in LEADING_FILLERS {
            if let Some(rest) = cur.strip_prefix(p).and_then(|r| r.strip_prefix(' ')) {
                cur = rest;
                continue 'lead;
            }
        }
        break;
    }
    // Peel trailing politeness ("… please", "… thank you").
    'trail: loop {
        for suf in TRAILING_FILLERS {
            if let Some(rest) = cur.strip_suffix(suf).and_then(|r| r.strip_suffix(' ')) {
                cur = rest;
                continue 'trail;
            }
        }
        break;
    }
    cur.to_string()
}

/// Safety-critical utterances routed before the deterministic matcher and LLM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tier0Command {
    Cancel,
    Undo,
}

/// Classify the exact tier-0 phrases used by the production Command pipeline.
///
/// Keeping this pure and colocated with normalization lets diagnostics exercise
/// the real decision without copying a safety-critical phrase list.
pub fn match_tier0_command(utterance: &str) -> Option<Tier0Command> {
    let normalized = normalize(utterance);
    let peeled = peel_politeness(&normalized);
    let candidates = [normalized.as_str(), peeled.as_str()];
    if candidates.iter().any(|candidate| {
        matches!(
            *candidate,
            "stop" | "stop it" | "cancel" | "cancel that" | "never mind" | "nevermind" | "abort"
        )
    }) {
        return Some(Tier0Command::Cancel);
    }
    if candidates.iter().any(|candidate| {
        matches!(
            *candidate,
            "undo that" | "undo it" | "undo last command" | "undo the last command"
        )
    }) {
        return Some(Tier0Command::Undo);
    }
    None
}

/// Canonicalize a normalized utterance for the zero-arg lookup: strip leading
/// politeness, trailing politeness, and interior determiner/pronoun fillers so
/// casual phrasings ("can you skip the song please") collapse onto the same
/// key as the bare form ("skip song").  Falls back to the input if stripping
/// leaves nothing.  Used ONLY for the zero-arg table — the open-app path parses
/// the politeness-peeled (but not filler-token-stripped) string so app names
/// keep their articles ("open the settings" → OpenApp("the settings")).
fn strip_fillers(s: &str) -> String {
    let peeled = peel_politeness(s);
    // Drop interior filler tokens (this also collapses the whitespace).
    let stripped = peeled
        .split_whitespace()
        .filter(|w| !FILLER_TOKENS.contains(w))
        .collect::<Vec<_>>()
        .join(" ");
    if stripped.is_empty() {
        s.to_string()
    } else {
        stripped
    }
}

/// Exact-match table of zero-argument commands.  Keys are in canonical
/// filler-free form (e.g. "take screenshot", not "take a screenshot") because
/// `match_command` also tries the filler-stripped utterance against this table —
/// so "take a screenshot", "close the window", etc. resolve via `strip_fillers`.
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
        "new tab" | "open new tab" => Kc(KeyChord::NewTab),
        "close tab" => Kc(KeyChord::CloseTab),
        "screenshot" | "take screenshot" | "snip" => Kc(KeyChord::Screenshot),
        "refresh" | "reload" | "refresh page" | "reload page" | "refresh screen" => {
            Kc(KeyChord::Refresh)
        }
        "find" | "find in page" | "find on page" | "search page" | "search in page" => {
            Kc(KeyChord::Find)
        }
        "new window" | "open new window" => Kc(KeyChord::NewWindow),
        "reopen tab" | "reopen closed tab" | "reopen last tab" | "restore tab"
        | "bring back tab" => Kc(KeyChord::ReopenTab),
        "next tab" | "switch tab" | "switch tabs" => Kc(KeyChord::NextTab),
        "previous tab" | "last tab" | "back tab" => Kc(KeyChord::PrevTab),
        "page down" | "scroll down" | "go down" => Kc(KeyChord::PageDown),
        "page up" | "scroll up" | "go up" => Kc(KeyChord::PageUp),
        // Bare "stop"/"stop it"/"cancel" are reserved tier-0 cancel phrases,
        // handled in the pipeline BEFORE the matcher — so they must NOT appear
        // here.  The qualified "stop music"/"stop song"/"stop playing" forms are
        // unambiguous transport commands and are safe.
        "play" | "pause" | "play pause" | "resume" | "play music" | "pause music"
        | "pause song" | "pause track" | "stop music" | "stop song" | "stop playback"
        | "stop playing" | "resume music" | "resume playback" | "resume playing"
        | "keep playing" | "continue playing" | "unpause" | "play song" => {
            Media(MediaAction::PlayPause)
        }
        "next track" | "next song" | "skip track" | "skip" | "skip song" | "next"
        | "play next song" | "play next" | "next one" | "skip to next song"
        | "skip to next track" | "skip forward" | "change song" => Media(MediaAction::NextTrack),
        "previous track" | "previous song" | "last track" | "previous" | "last song"
        | "play previous song" | "play last song" | "go back song" | "back song" | "rewind" => {
            Media(MediaAction::PrevTrack)
        }
        "mute" | "mute sound" | "mute audio" | "mute volume" | "silence" | "sound off"
        | "turn off sound" | "mute everything" => Media(MediaAction::Mute),
        // Directional un-mute is routed separately so it can't invert an already-on
        // state (VK_VOLUME_MUTE is a blind toggle; the executor sets mute via WASAPI).
        "unmute" | "unmute sound" | "unmute audio" | "sound on" | "turn on sound" => {
            Media(MediaAction::Unmute)
        }
        "volume up" | "louder" | "turn up" | "turn up volume" | "turn volume up"
        | "raise volume" | "increase volume" | "volume higher" | "make louder" | "crank up"
        | "crank up volume" | "pump up volume" => Media(MediaAction::VolumeUp),
        "volume down" | "quieter" | "turn down" | "turn down volume" | "turn volume down"
        | "lower volume" | "decrease volume" | "volume lower" | "make quieter" | "softer"
        | "quiet down" => Media(MediaAction::VolumeDown),
        "minimize" | "minimise" | "minimize window" | "hide window" => {
            Window(WindowAction::Minimize)
        }
        "maximize" | "maximise" | "maximize window" | "full screen" | "make full screen"
        | "go full screen" | "fullscreen" => Window(WindowAction::Maximize),
        "show desktop"
        | "show me desktop"
        | "minimize everything"
        | "minimise everything"
        | "minimize all"
        | "hide everything" => Kc(KeyChord::ShowDesktop),
        "close window" | "close current window" => CommandIntent::CloseWindow,
        // OmniVox's own scratchpad pad.  Both "scratchpad" and the ASR's usual
        // two-word "scratch pad" spelling are listed; filler stripping folds
        // "open the scratch pad" etc. onto these keys.  Open synonyms beyond
        // these ("bring up …") are caught by the open-verb interception below.
        "open scratchpad" | "open scratch pad" | "show scratchpad" | "show scratch pad" => {
            CommandIntent::Scratchpad(ScratchpadAction::Open)
        }
        "close scratchpad" | "close scratch pad" | "hide scratchpad" | "hide scratch pad" => {
            CommandIntent::Scratchpad(ScratchpadAction::Close)
        }
        "clear scratchpad" | "clear scratch pad" | "empty scratchpad" | "empty scratch pad"
        | "wipe scratchpad" | "wipe scratch pad" => {
            CommandIntent::Scratchpad(ScratchpadAction::Clear)
        }
        _ => return None,
    };
    Some(intent)
}

/// True when an open-verb target names the built-in scratchpad ("scratchpad",
/// "the scratch pad", "my scratchpad", …) — so every natural opener ("bring up
/// the scratch pad") resolves deterministically instead of dead-ending in the
/// external-app resolver.
fn is_scratchpad_name(target: &str) -> bool {
    let squashed: String = target
        .split_whitespace()
        .filter(|w| !FILLER_TOKENS.contains(w))
        .collect::<Vec<_>>()
        .concat();
    // Accept the stray plural too ("open my scratch pads") — without an LLM
    // configured it would otherwise dead-end as an app lookup.
    squashed == "scratchpad" || squashed == "scratchpads"
}

/// Return the literal tail of an explicit command prefix without normalizing it.
///
/// `normalize` is intentionally lossy (it lowercases and discards punctuation),
/// which is exactly right for fixed chords but wrong for a command such as
/// `type Hello, Sam!`. Keep the text the recognizer supplied for parameterized
/// commands; the executor's clipboard path will paste this value verbatim.
fn raw_after_prefix<'a>(utterance: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    let input = utterance.trim();
    for prefix in prefixes {
        // Prefixes are ASCII. `get` keeps this safe when the input begins with a
        // multi-byte character, and `eq_ignore_ascii_case` avoids altering the
        // literal target's casing.
        let Some(head) = input.get(..prefix.len()) else {
            continue;
        };
        if !head.eq_ignore_ascii_case(prefix) {
            continue;
        }
        let Some(tail) = input.get(prefix.len()..) else {
            continue;
        };
        // Do not turn "typewriter" or "googleable" into commands. A colon is
        // accepted because speech recognizers commonly render "type: hello".
        let Some(first) = tail.chars().next() else {
            continue;
        };
        if !first.is_whitespace() && first != ':' {
            continue;
        }
        let value = tail
            .trim_start_matches(|c: char| c.is_whitespace() || c == ':')
            .trim();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

/// Recognize a literal typing command. This deliberately has no `send` form:
/// a deterministic matcher must never infer a consequential Enter press from
/// arbitrary text. The existing `TypeText { submit: false }` intent keeps the
/// pipeline's focus binding and undo semantics intact.
fn parse_type_text(utterance: &str) -> Option<CommandIntent> {
    let text = raw_after_prefix(utterance, &["type", "write"])?;
    Some(CommandIntent::TypeText {
        text: text.to_string(),
        submit: false,
    })
}

/// Whether a literal target is an explicit, safe-to-validate web URL/domain.
/// The executor performs the authoritative validation immediately before
/// opening it; this stricter recognition check merely prevents ordinary app
/// names (or prose) from taking the browser fast path.
fn explicit_web_target(target: &str) -> Option<String> {
    let target = target.trim_end_matches(['.', ',', '!', '?']);
    if target.is_empty() || target.chars().any(char::is_whitespace) {
        return None;
    }
    let candidate = if target.starts_with("http://") || target.starts_with("https://") {
        target.to_string()
    } else {
        format!("https://{target}")
    };
    let parsed = url::Url::parse(&candidate).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none_or(str::is_empty)
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }

    // A scheme, localhost, an IP literal, or a dotted host is explicit enough.
    // `open spotify` must remain an app command rather than becoming a browser
    // navigation merely because URL parsing can accept a bare hostname.
    let host = parsed.host_str()?;
    let explicit = target.contains("://")
        || host.eq_ignore_ascii_case("localhost")
        || host.parse::<std::net::IpAddr>().is_ok()
        || host.contains('.');
    explicit.then(|| target.to_string())
}

/// Match a direct browser-navigation phrase while preserving URL punctuation.
fn parse_open_url(utterance: &str) -> Option<CommandIntent> {
    let target = raw_after_prefix(
        utterance,
        &["navigate to", "open up", "open", "go to", "visit"],
    )?;
    explicit_web_target(target).map(CommandIntent::OpenUrl)
}

/// Match only unambiguous explicit search phrases. In particular, bare
/// `google cats` stays out of the fast path: it can plausibly mean an app/site
/// name (for example Google Chrome or Google Maps), not a web search.
fn parse_web_search(utterance: &str) -> Option<CommandIntent> {
    const SEARCH_PREFIXES: &[&str] = &[
        "search the web for",
        "search the web",
        "search web for",
        "web search for",
        "web search",
        "search google for",
        "search on google for",
        "google search for",
        "google search",
        "search for",
        "look up",
    ];
    // Do not reinterpret an incomplete long form such as "search the web for"
    // as the shorter "search the web" with a query of just "for".
    let raw = utterance.trim();
    if [
        "search the web for",
        "search web for",
        "web search for",
        "search google for",
        "search on google for",
        "google search for",
    ]
    .iter()
    .any(|prefix| raw.eq_ignore_ascii_case(prefix))
    {
        return None;
    }
    raw_after_prefix(raw, SEARCH_PREFIXES).map(|query| CommandIntent::WebSearch(query.to_string()))
}

/// Match a browser search page with no query. Kept separate from
/// `raw_after_prefix`, which deliberately requires a non-empty tail.
fn parse_empty_web_search(utterance: &str) -> Option<CommandIntent> {
    let raw = utterance.trim();
    [
        "search the web",
        "search web",
        "web search",
        "google search",
    ]
    .iter()
    .any(|phrase| raw.eq_ignore_ascii_case(phrase))
    .then_some(CommandIntent::WebSearch(String::new()))
}

/// Multi-step commands are intentionally bounded and conservative. Splitting
/// a literal typing/search target on "and" would be surprising, so only the
/// action kinds that are already immediate, non-confirmed deterministic actions
/// can participate. App launch, window close, scratchpad mutation, and typing
/// therefore stay single-command paths with their existing safeguards.
fn safe_chain_intent(intent: &CommandIntent) -> bool {
    matches!(
        intent,
        CommandIntent::KeyChord(_)
            | CommandIntent::Media(_)
            | CommandIntent::Window(_)
            | CommandIntent::WebSearch(_)
            | CommandIntent::OpenUrl(_)
    )
}

/// Split at explicit spoken conjunctions while retaining each segment's
/// original spelling for literal text, URLs, and search queries. The three-step
/// ceiling prevents a long dictation fragment from turning into an accidental
/// batch of OS actions.
fn conjunction_segments(utterance: &str) -> Option<Vec<&str>> {
    const SEPARATORS: &[&str] = &[" and then ", ", then ", " then ", " and ", ", and "];
    let lower = utterance.to_ascii_lowercase();
    let mut segments = Vec::new();
    let mut start = 0;

    loop {
        let next = SEPARATORS
            .iter()
            .filter_map(|separator| {
                lower[start..]
                    .find(separator)
                    .map(|offset| (start + offset, *separator))
            })
            .min_by_key(|(index, _)| *index);
        let Some((index, separator)) = next else {
            break;
        };
        let segment = utterance[start..index].trim();
        if segment.is_empty() {
            return None;
        }
        segments.push(segment);
        start = index + separator.len();
        if segments.len() == 3 {
            // More than three segments is deliberately not a deterministic
            // batch; the LLM (or a future explicit batch UI) can interpret it.
            return None;
        }
    }

    if segments.is_empty() {
        return None;
    }
    let last = utterance[start..].trim();
    if last.is_empty() {
        return None;
    }
    segments.push(last);
    Some(segments)
}

/// Parse a single Command-Mode utterance into a [`CommandIntent`].
///
/// Zero-arg commands are tried first so fixed phrases ("new tab") win over the
/// parameterized paths and open-verb prefixes ("open …").
fn match_single_command(utterance: &str) -> Option<CommandIntent> {
    let norm = normalize(utterance);
    if norm.is_empty() {
        return None;
    }

    // Zero-arg commands are tried against the raw normalized string first, then
    // against the filler-stripped canonical form — so legacy phrasings ("close
    // the window", "can you skip the song please") resolve without bloating the
    // table, while app names in the open-verb path below keep their articles.
    if let Some(intent) = zero_arg(&norm) {
        return Some(intent);
    }
    if let Some(intent) = zero_arg(&strip_fillers(&norm)) {
        return Some(intent);
    }

    // Parameterized forms must preserve capitalization and punctuation. Parse
    // them from the raw utterance before the normalized fallbacks below.
    if let Some(intent) = parse_type_text(utterance) {
        return Some(intent);
    }
    if let Some(intent) = parse_open_url(utterance) {
        return Some(intent);
    }
    if let Some(intent) = parse_web_search(utterance) {
        return Some(intent);
    }
    if let Some(intent) = parse_empty_web_search(utterance) {
        return Some(intent);
    }

    // Peel politeness ("please open notepad", "open notepad please") before
    // the open-verb prefixes so it doesn't pollute the app name — but keep
    // interior articles ("open the settings" → OpenApp("the settings")).
    let peeled = peel_politeness(&norm);

    // Deterministic web search — skip the LLM (and its multi-second first-use
    // model load) for unambiguous "search for X" phrasings.  Runs AFTER the
    // zero-arg table (so "search page"/"find in page" stay the Find chord) and
    // BEFORE the greedy open-verb loop (so "search for X" isn't parsed as
    // OpenApp("for X")).  Bare "search" is excluded (ambiguous with in-app
    // find), and so is "google" — "google chrome" / "google maps" usually mean
    // "open that app/site", so those defer to the LLM to disambiguate.
    const SEARCH_VERBS: &[&str] = &[
        "search the web for",
        "search the web",
        "search for",
        "look up",
    ];
    for verb in SEARCH_VERBS {
        if let Some(rest) = peeled.strip_prefix(verb) {
            // Word boundary: bare verb → open the search page; "<verb> <query>" →
            // search it.  "googled"/"searching" (no following space) fall through
            // to the open-verb loop below.
            if rest.is_empty() {
                return Some(CommandIntent::WebSearch(String::new()));
            }
            if let Some(query) = rest.strip_prefix(' ') {
                let query = query.trim();
                return Some(CommandIntent::WebSearch(query.to_string()));
            }
        }
    }

    for verb in OPEN_VERBS {
        if let Some(rest) = peeled.strip_prefix(verb) {
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
                    // The built-in scratchpad intercepts app resolution.
                    if is_scratchpad_name(target) {
                        return Some(CommandIntent::Scratchpad(ScratchpadAction::Open));
                    }
                    return Some(CommandIntent::OpenApp(target.to_string()));
                }
            }
        }
    }

    None
}

/// Parse a command into an ordered, all-or-nothing deterministic sequence.
///
/// The sequence fast path is intentionally narrow: only up to three explicit
/// conjunction-separated segments are accepted, and every segment must be an
/// independently recognized safe deterministic action. Returning `None` for a
/// partially understood sequence ensures the caller never silently performs a
/// prefix of what the user said.
pub fn match_command_sequence(utterance: &str) -> Option<Vec<CommandIntent>> {
    if let Some(segments) = conjunction_segments(utterance) {
        let intents: Option<Vec<_>> = segments.into_iter().map(match_single_command).collect();
        if let Some(intents) = intents.filter(|intents| intents.iter().all(safe_chain_intent)) {
            return Some(intents);
        }
    }
    match_single_command(utterance).map(|intent| vec![intent])
}

/// Source-compatible single-command matcher. Multi-step callers should use
/// [`match_command_sequence`] and preserve the pipeline's existing chain
/// confirmation/target-binding behavior.
pub fn match_command(utterance: &str) -> Option<CommandIntent> {
    let mut intents = match_command_sequence(utterance)?;
    (intents.len() == 1).then(|| intents.remove(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_zero_routes_cancel_and_undo_before_matching() {
        for utterance in [
            "stop",
            "please stop",
            "cancel that please",
            "never mind",
            "abort",
        ] {
            assert_eq!(match_tier0_command(utterance), Some(Tier0Command::Cancel));
        }
        for utterance in ["undo that", "could you undo the last command please"] {
            assert_eq!(match_tier0_command(utterance), Some(Tier0Command::Undo));
        }
        for utterance in ["stop music", "cancel my subscription", "undo"] {
            assert_eq!(match_tier0_command(utterance), None);
        }
    }

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
        assert_eq!(
            match_command("copy"),
            Some(CommandIntent::KeyChord(KeyChord::Copy))
        );
        assert_eq!(
            match_command("Select All!"),
            Some(CommandIntent::KeyChord(KeyChord::SelectAll))
        );
        assert_eq!(
            match_command("new tab"),
            Some(CommandIntent::KeyChord(KeyChord::NewTab))
        );
        assert_eq!(
            match_command("screenshot"),
            Some(CommandIntent::KeyChord(KeyChord::Screenshot))
        );
    }

    #[test]
    fn zero_arg_media_and_window() {
        assert_eq!(
            match_command("play"),
            Some(CommandIntent::Media(MediaAction::PlayPause))
        );
        assert_eq!(
            match_command("mute"),
            Some(CommandIntent::Media(MediaAction::Mute))
        );
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
        assert_eq!(
            match_command("close this window"),
            Some(CommandIntent::CloseWindow)
        );
        assert_eq!(
            match_command("close window"),
            Some(CommandIntent::CloseWindow)
        );
        // "close tab" must stay the tab chord, never CloseWindow.
        assert_eq!(
            match_command("close tab"),
            Some(CommandIntent::KeyChord(KeyChord::CloseTab))
        );
    }

    #[test]
    fn zero_arg_wins_over_open_verb() {
        // "new tab" must be the chord, never OpenApp("tab").
        assert_eq!(
            match_command("new tab"),
            Some(CommandIntent::KeyChord(KeyChord::NewTab))
        );
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

    #[test]
    fn filler_stripping_headline_fixes() {
        // The reported bug: "skip the/this song" must be NextTrack, not the LLM's
        // play_pause misfire.
        let next = Some(CommandIntent::Media(MediaAction::NextTrack));
        assert_eq!(match_command("skip the song"), next);
        assert_eq!(match_command("skip this song"), next);
        assert_eq!(match_command("skip it"), next);
        assert_eq!(
            match_command("pause the music"),
            Some(CommandIntent::Media(MediaAction::PlayPause))
        );
        let up = Some(CommandIntent::Media(MediaAction::VolumeUp));
        assert_eq!(match_command("turn it up"), up);
        assert_eq!(match_command("turn up the volume"), up);
        // Leading + trailing politeness stripped together.
        assert_eq!(match_command("can you skip the song please"), next);
        assert_eq!(
            match_command("please mute"),
            Some(CommandIntent::Media(MediaAction::Mute))
        );
    }

    #[test]
    fn filler_stripping_keeps_chord_phrasings() {
        assert_eq!(
            match_command("copy that"),
            Some(CommandIntent::KeyChord(KeyChord::Copy))
        );
        assert_eq!(
            match_command("copy this"),
            Some(CommandIntent::KeyChord(KeyChord::Copy))
        );
        assert_eq!(
            match_command("paste it"),
            Some(CommandIntent::KeyChord(KeyChord::Paste))
        );
        assert_eq!(
            match_command("close this tab"),
            Some(CommandIntent::KeyChord(KeyChord::CloseTab))
        );
        // Legacy "take a screenshot" / "show the desktop" canonicalize.
        assert_eq!(
            match_command("take a screenshot"),
            Some(CommandIntent::KeyChord(KeyChord::Screenshot))
        );
        assert_eq!(
            match_command("show the desktop"),
            Some(CommandIntent::KeyChord(KeyChord::ShowDesktop))
        );
        // Legacy close-window phrasings collapse to CloseWindow.
        assert_eq!(
            match_command("close the window"),
            Some(CommandIntent::CloseWindow)
        );
        assert_eq!(
            match_command("close this window"),
            Some(CommandIntent::CloseWindow)
        );
    }

    #[test]
    fn bare_stop_is_not_a_command() {
        // "stop"/"stop it"/"cancel" are tier-0 cancel phrases handled before the
        // matcher — they must never resolve to a media action here.
        assert_eq!(match_command("stop"), None);
        assert_eq!(match_command("stop it"), None);
        assert_eq!(match_command("cancel"), None);
        // But the qualified transport forms are fine.
        assert_eq!(
            match_command("stop music"),
            Some(CommandIntent::Media(MediaAction::PlayPause))
        );
        assert_eq!(
            match_command("stop playing"),
            Some(CommandIntent::Media(MediaAction::PlayPause))
        );
    }

    #[test]
    fn open_new_tab_and_window_beat_open_verb() {
        // zero_arg is checked before the OPEN_VERBS loop, so these are chords,
        // never OpenApp("new tab") / OpenApp("new window").
        assert_eq!(
            match_command("open a new tab"),
            Some(CommandIntent::KeyChord(KeyChord::NewTab))
        );
        assert_eq!(
            match_command("open new window"),
            Some(CommandIntent::KeyChord(KeyChord::NewWindow))
        );
    }

    #[test]
    fn stripping_does_not_break_app_names() {
        // The open-verb path only peels leading/trailing politeness, not
        // interior filler tokens, so articles in an app name are preserved.
        assert_eq!(
            match_command("open spotify"),
            Some(CommandIntent::OpenApp("spotify".into()))
        );
        assert_eq!(
            match_command("open the settings"),
            Some(CommandIntent::OpenApp("the settings".into()))
        );
    }

    #[test]
    fn open_verb_peels_politeness_from_app_name() {
        // Politeness wrapping must not pollute the app name, and must not
        // block the deterministic path when it comes before the verb.
        assert_eq!(
            match_command("open notepad please"),
            Some(CommandIntent::OpenApp("notepad".into()))
        );
        assert_eq!(
            match_command("can you open spotify"),
            Some(CommandIntent::OpenApp("spotify".into()))
        );
        assert_eq!(
            match_command("please open notepad"),
            Some(CommandIntent::OpenApp("notepad".into()))
        );
    }

    #[test]
    fn new_key_chords_match() {
        for (utter, chord) in [
            ("refresh", KeyChord::Refresh),
            ("reload the page", KeyChord::Refresh),
            ("find in page", KeyChord::Find),
            ("new window", KeyChord::NewWindow),
            ("reopen closed tab", KeyChord::ReopenTab),
            ("next tab", KeyChord::NextTab),
            ("previous tab", KeyChord::PrevTab),
            ("page down", KeyChord::PageDown),
            ("scroll up", KeyChord::PageUp),
        ] {
            assert_eq!(
                match_command(utter),
                Some(CommandIntent::KeyChord(chord)),
                "failed for: {utter}"
            );
        }
    }

    #[test]
    fn expanded_media_phrases() {
        assert_eq!(
            match_command("change the song"),
            Some(CommandIntent::Media(MediaAction::NextTrack))
        );
        assert_eq!(
            match_command("go back a song"),
            Some(CommandIntent::Media(MediaAction::PrevTrack))
        );
        assert_eq!(
            match_command("make it louder"),
            Some(CommandIntent::Media(MediaAction::VolumeUp))
        );
        assert_eq!(
            match_command("lower volume"),
            Some(CommandIntent::Media(MediaAction::VolumeDown))
        );
        assert_eq!(
            match_command("silence"),
            Some(CommandIntent::Media(MediaAction::Mute))
        );
    }

    #[test]
    fn unmute_is_distinct_from_mute() {
        // Directional un-mute must NOT collapse onto the toggle, so the executor
        // can set state deterministically instead of inverting it.
        assert_eq!(
            match_command("mute"),
            Some(CommandIntent::Media(MediaAction::Mute))
        );
        assert_eq!(
            match_command("unmute"),
            Some(CommandIntent::Media(MediaAction::Unmute))
        );
        assert_eq!(
            match_command("turn on sound"),
            Some(CommandIntent::Media(MediaAction::Unmute))
        );
        assert_eq!(
            match_command("sound off"),
            Some(CommandIntent::Media(MediaAction::Mute))
        );
    }

    #[test]
    fn show_me_the_desktop_resolves_deterministically() {
        // A documented example chip — must not dead-end as OpenApp("the desktop").
        assert_eq!(
            match_command("show me the desktop"),
            Some(CommandIntent::KeyChord(KeyChord::ShowDesktop))
        );
    }

    #[test]
    fn scratchpad_phrases_match() {
        let open = Some(CommandIntent::Scratchpad(ScratchpadAction::Open));
        assert_eq!(match_command("open scratchpad"), open);
        assert_eq!(match_command("open the scratch pad"), open);
        assert_eq!(match_command("show me the scratchpad"), open);
        assert_eq!(match_command("bring up my scratch pad"), open);
        assert_eq!(match_command("pull up the scratchpad"), open);
        assert_eq!(match_command("can you open the scratch pad please"), open);
        let close = Some(CommandIntent::Scratchpad(ScratchpadAction::Close));
        assert_eq!(match_command("close the scratchpad"), close);
        assert_eq!(match_command("hide the scratch pad"), close);
        let clear = Some(CommandIntent::Scratchpad(ScratchpadAction::Clear));
        assert_eq!(match_command("clear the scratchpad"), clear);
        assert_eq!(match_command("empty the scratch pad"), clear);
        assert_eq!(match_command("wipe the scratchpad"), clear);
        // Similar-but-different names still resolve as external apps.
        assert_eq!(
            match_command("open scratch"),
            Some(CommandIntent::OpenApp("scratch".into()))
        );
    }

    #[test]
    fn deterministic_web_search() {
        assert_eq!(
            match_command("search for the weather today"),
            Some(CommandIntent::WebSearch("the weather today".into()))
        );
        assert_eq!(
            match_command("look up rust lifetimes"),
            Some(CommandIntent::WebSearch("rust lifetimes".into()))
        );
        assert_eq!(
            match_command("search the web for pizza near me"),
            Some(CommandIntent::WebSearch("pizza near me".into()))
        );
        // Bare "search the web" → open the search page.
        assert_eq!(
            match_command("search the web"),
            Some(CommandIntent::WebSearch(String::new()))
        );
        // "search page" stays the Find chord (zero-arg wins first).
        assert_eq!(
            match_command("search page"),
            Some(CommandIntent::KeyChord(KeyChord::Find))
        );
        // "google X" is NOT a deterministic search — it defers to the LLM so
        // "google chrome" / "google maps" can be disambiguated as app/site opens.
        assert_eq!(match_command("google cats"), None);
        assert_eq!(match_command("google chrome"), None);
        // "open google" still opens the app/site.
        assert_eq!(
            match_command("open google"),
            Some(CommandIntent::OpenApp("google".into()))
        );
    }

    #[test]
    fn literal_type_and_write_preserve_the_recognized_text() {
        assert_eq!(
            match_command("Type Hello, Sam!"),
            Some(CommandIntent::TypeText {
                text: "Hello, Sam!".into(),
                submit: false,
            })
        );
        assert_eq!(
            match_command("write: Keep the API key in a vault."),
            Some(CommandIntent::TypeText {
                text: "Keep the API key in a vault.".into(),
                submit: false,
            })
        );
        // The word must be a command prefix, never a substring in ordinary
        // dictation/prose. Deterministic typing also never implies Enter.
        assert_eq!(match_command("typewriter repair is expensive"), None);
        assert_eq!(match_command("I need to write a report tomorrow"), None);
        assert_eq!(match_command("write"), None);
    }

    #[test]
    fn explicit_google_forms_preserve_the_search_query() {
        assert_eq!(
            match_command("Search Google for Rust async cancellation"),
            Some(CommandIntent::WebSearch("Rust async cancellation".into()))
        );
        assert_eq!(
            match_command("google search: local speech recognition"),
            Some(CommandIntent::WebSearch("local speech recognition".into()))
        );
        assert_eq!(
            match_command("web search for cafés near me"),
            Some(CommandIntent::WebSearch("cafés near me".into()))
        );
        // Bare Google phrases remain ambiguous with app/site names.
        assert_eq!(match_command("google chrome"), None);
        assert_eq!(match_command("google cats"), None);
        assert_eq!(match_command("search google for"), None);
    }

    #[test]
    fn explicit_urls_bypass_app_resolution_but_reject_url_like_tricks() {
        assert_eq!(
            match_command("open GitHub.com/OmniVox?tab=readme"),
            Some(CommandIntent::OpenUrl(
                "GitHub.com/OmniVox?tab=readme".into()
            ))
        );
        assert_eq!(
            match_command("visit https://example.com/docs."),
            Some(CommandIntent::OpenUrl("https://example.com/docs".into()))
        );
        assert_eq!(
            match_command("navigate to localhost:3000"),
            Some(CommandIntent::OpenUrl("localhost:3000".into()))
        );
        // Credentials must never be treated as a grounded deterministic URL.
        assert_ne!(
            match_command("open https://trusted.example@evil.example"),
            Some(CommandIntent::OpenUrl(
                "https://trusted.example@evil.example".into()
            ))
        );
        // Ordinary app names stay on the app-resolution path.
        assert_eq!(
            match_command("open spotify"),
            Some(CommandIntent::OpenApp("spotify".into()))
        );
    }

    #[test]
    fn bounded_conjunctions_are_all_or_nothing_and_source_compatible() {
        assert_eq!(
            match_command_sequence("copy and then paste and save"),
            Some(vec![
                CommandIntent::KeyChord(KeyChord::Copy),
                CommandIntent::KeyChord(KeyChord::Paste),
                CommandIntent::KeyChord(KeyChord::Save),
            ])
        );
        // Existing single-command callers never accidentally run only the first
        // part of a chain.
        assert_eq!(match_command("copy and then paste"), None);
        // An unsafe/unknown segment invalidates the entire deterministic chain.
        assert_eq!(match_command_sequence("copy and close window"), None);
        assert_eq!(match_command_sequence("open spotify and play"), None);
        assert_eq!(
            match_command_sequence("copy and paste and save and undo"),
            None
        );
        // Do not split literal typing/search content on ordinary conjunctions.
        assert_eq!(
            match_command_sequence("type copy and paste"),
            Some(vec![CommandIntent::TypeText {
                text: "copy and paste".into(),
                submit: false,
            }])
        );
        assert_eq!(
            match_command_sequence("search for peanut butter and jelly"),
            Some(vec![CommandIntent::WebSearch(
                "peanut butter and jelly".into()
            )])
        );
    }
}
