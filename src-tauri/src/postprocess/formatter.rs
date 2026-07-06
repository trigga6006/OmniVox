//! Post-processing text formatter.
//!
//! Detects list patterns in cleaned text and applies bullet formatting.
//! Runs after the processor chain so structural formatting is handled here
//! with deterministic heuristics at zero inference cost.

// ── Marker stripping ────────────────────────────────────────────────────

/// Strip pre-existing list/heading markers from text so the formatter starts
/// clean.  Whisper sometimes hallucinates markdown-style markers from its
/// training data, and users may say "dash" or "bullet point" aloud.
///
/// Strips: `- `, `* `, `• `, `1. `, `## `, `**bold**`, inline `- ` markers.
/// Rejoins everything into flowing prose separated by spaces.
fn strip_existing_markers(text: &str) -> String {
    let mut out = String::with_capacity(text.len());

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Strip leading bullet/list markers
        let stripped = strip_line_marker(trimmed);

        // Strip inline bold markers: **text** or __text__ → text
        let stripped = strip_inline_bold(stripped);

        if stripped.is_empty() {
            continue;
        }

        if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        out.push_str(stripped);
    }

    // Second pass: strip inline markers that appear after sentence punctuation
    // within a single line.  Whisper sometimes emits "sentence. - next item."
    // as a single line.
    strip_inline_markers(&out)
}

/// Remove bullet markers that appear inline after sentence-ending punctuation.
/// E.g., "Here are tasks. - Fix bug. - Run tests." → "Here are tasks. Fix bug. Run tests."
fn strip_inline_markers(text: &str) -> String {
    let mut result = text.to_string();
    // Patterns: ". - ", "! - ", "? - " and variants with * or •
    for marker in &[
        ". - ", "! - ", "? - ", ". * ", "! * ", "? * ", ". • ", "! • ", "? • ",
    ] {
        let punct = &marker[..1]; // Keep the sentence-ending punctuation
        let replacement = format!("{punct} ");
        while result.contains(marker) {
            result = result.replace(marker, &replacement);
        }
    }
    result
}

/// Strip a single leading list/heading marker from a line.
fn strip_line_marker(line: &str) -> &str {
    let s = line.trim_start();

    // Heading markers: "## ", "### ", etc.
    if s.starts_with('#') {
        let after_hashes = s.trim_start_matches('#');
        if after_hashes.starts_with(' ') {
            return after_hashes.trim_start();
        }
    }

    // Bullet markers: "- ", "* ", "• ", "· "
    for marker in &["- ", "* ", "• ", "· "] {
        if s.starts_with(marker) {
            return s[marker.len()..].trim_start();
        }
    }

    // Some existing tests/outputs contain a double-mojibake form of the same
    // markers. Keep these as escapes so the source stays unambiguous.
    for marker in &[
        "\u{00C3}\u{00A2}\u{00E2}\u{201A}\u{00AC}\u{00C2}\u{00A2} ",
        "\u{00C3}\u{201A}\u{00C2}\u{00B7} ",
    ] {
        if s.starts_with(marker) {
            return s[marker.len()..].trim_start();
        }
    }
    if let Some((marker, rest)) = s.split_once(' ') {
        let is_bulletish = marker
            .chars()
            .any(|c| matches!(c, '\u{00A2}' | '\u{00B7}' | '\u{2022}' | '\u{00AC}'))
            && marker.chars().all(|c| !c.is_ascii_alphanumeric());
        if is_bulletish {
            return rest.trim_start();
        }
    }

    // Numbered markers: "1. ", "2) ", "10. ", etc.
    if let Some(rest) = strip_numbered_prefix(s) {
        return rest;
    }

    s
}

/// True when the input already contains obvious structural markers from
/// Markdown / list hallucination. Short dictations normally bypass formatter
/// logic, but these markers are safe to clean even when the text is brief.
fn has_explicit_markers(text: &str) -> bool {
    if text.lines().any(|line| {
        let s = line.trim_start();
        strip_line_marker(s) != s
    }) || strip_inline_markers(text) != text
    {
        return true;
    }

    // Legacy fallback for older mojibake marker spellings already present in
    // tests / saved transcripts. The stripping side handles those generically.
    text.lines().any(|line| {
        let s = line.trim_start();
        s.starts_with("# ")
            || s.starts_with("##")
            || s.starts_with("- ")
            || s.starts_with("* ")
            || s.starts_with("â€¢ ")
            || s.starts_with("Â· ")
            || strip_numbered_prefix(s).is_some()
    })
}

/// Strip a leading numbered list marker like "1. " or "2) " from a string.
/// Returns the remainder, or None if no numbered prefix was found.
fn strip_numbered_prefix(s: &str) -> Option<&str> {
    let bytes = s.as_bytes();
    let mut i = 0;

    // Consume digits
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    // Need at least one digit followed by ". " or ") "
    if i == 0 || i >= bytes.len() {
        return None;
    }

    if (bytes[i] != b'.' && bytes[i] != b')') || i + 1 >= bytes.len() || bytes[i + 1] != b' ' {
        return None;
    }

    let rest = s[i + 2..].trim_start();
    let Some(first) = rest.as_bytes().first() else {
        return None;
    };

    // Do not strip decimal/number continuations such as "1. 5 million" or
    // "2. 2026 goals". Whisper occasionally inserts a space after a decimal
    // point; treating that as a list marker silently corrupts the number.
    if first.is_ascii_digit() {
        return None;
    }

    Some(rest)
}

/// Strip **bold** and __bold__ inline markers.
fn strip_inline_bold(s: &str) -> &str {
    let s = s.trim();
    // Leading + trailing ** or __
    if s.len() >= 4 {
        if s.starts_with("**") && s.ends_with("**") {
            return &s[2..s.len() - 2];
        }
        if s.starts_with("__") && s.ends_with("__") {
            return &s[2..s.len() - 2];
        }
    }
    s
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// Number words → numeric value.
fn parse_count(word: &str) -> Option<usize> {
    match word {
        "two" | "2" => Some(2),
        "three" | "3" => Some(3),
        "four" | "4" => Some(4),
        "five" | "5" => Some(5),
        "six" | "6" => Some(6),
        "seven" | "7" => Some(7),
        "eight" | "8" => Some(8),
        "nine" | "9" => Some(9),
        "ten" | "10" => Some(10),
        _ => None,
    }
}

/// Nouns that signal a list is being introduced.
const COLLECTION_NOUNS: &[&str] = &[
    "things",
    "items",
    "points",
    "tasks",
    "reasons",
    "steps",
    "features",
    "goals",
    "topics",
    "changes",
    "updates",
    "issues",
    "problems",
    "options",
    "requirements",
    "examples",
    "priorities",
];

/// Common abbreviations that end with a period but don't end a sentence.
const ABBREVIATIONS: &[&str] = &[
    "mr", "mrs", "ms", "dr", "prof", "sr", "jr", "st", "ave", "blvd", "dept", "est", "govt", "inc",
    "corp", "ltd", "co", "vs", "etc", "approx", "appt", "dept", "diam", "qty", "temp",
    // Titles & honorifics
    "gen", "sgt", "cpl", "pvt", "capt", "lt", "col", "maj", "cmdr", "rev", "hon",
];

/// True if the period at `dot_pos` in `text` is part of an abbreviation or
/// decimal number rather than a sentence boundary.
fn is_non_sentence_period(text: &str, dot_pos: usize) -> bool {
    let bytes = text.as_bytes();

    // Decimal number: digit before AND digit after the dot ("3.5")
    if dot_pos > 0
        && dot_pos + 1 < bytes.len()
        && bytes[dot_pos - 1].is_ascii_digit()
        && bytes[dot_pos + 1].is_ascii_digit()
    {
        return true;
    }

    // Ellipsis: part of "..." — don't split mid-ellipsis
    if dot_pos + 1 < bytes.len() && bytes[dot_pos + 1] == b'.' {
        return true;
    }
    if dot_pos > 0 && bytes[dot_pos - 1] == b'.' {
        return true;
    }

    // Abbreviation: short word before the dot that's in our list
    // Walk backwards to find the word before the dot.
    let before = &text[..dot_pos];
    let word_start = before
        .rfind(|c: char| !c.is_alphabetic())
        .map(|p| p + 1)
        .unwrap_or(0);
    let word = &before[word_start..];
    if !word.is_empty() && word.len() <= 5 {
        let lower = word.to_lowercase();
        if ABBREVIATIONS.contains(&lower.as_str()) {
            return true;
        }
    }

    // Single-letter abbreviation followed by dot (e.g., "U.S.A.", middle initials)
    if word.len() == 1
        && word
            .chars()
            .next()
            .map(|c| c.is_ascii_uppercase())
            .unwrap_or(false)
    {
        return true;
    }

    false
}

/// Split text into sentences on `.` `!` `?`, keeping the delimiter attached.
///
/// Handles abbreviations (Dr., Mr., U.S.), decimal numbers (3.5), and
/// ellipses (...) without false splits.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut byte_pos: usize = 0;
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        current.push(c);
        let c_len = c.len_utf8();

        if matches!(c, '.' | '!' | '?') {
            // For periods, check if this is actually a sentence boundary.
            let is_boundary = if c == '.' {
                !is_non_sentence_period(text, byte_pos)
            } else {
                true // ! and ? are always sentence boundaries
            };

            if is_boundary {
                let trimmed = current.trim().to_string();
                if !trimmed.is_empty() {
                    sentences.push(trimmed);
                }
                current.clear();
            }
        }

        byte_pos += c_len;
        i += 1;
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }
    sentences
}

/// Capitalize the first alphabetic character of a string.
fn capitalize_first(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(c) => {
            let mut result = String::with_capacity(s.len());
            for uc in c.to_uppercase() {
                result.push(uc);
            }
            result.push_str(chars.as_str());
            result
        }
    }
}

/// Strip a leading connector word from a sentence (common in spoken lists).
fn strip_leading_connector(s: &str) -> &str {
    let trimmed = s.trim_start();
    for prefix in &[
        "and ", "then ", "also ", "plus ", "or ", "next ", "finally ", "lastly ",
    ] {
        if trimmed.len() >= prefix.len() && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return trimmed[prefix.len()..].trim_start();
        }
    }
    trimmed
}

/// Strip leading ordinal markers ("First,", "Secondly,", etc.) from a sentence.
fn strip_leading_ordinal(s: &str) -> &str {
    let trimmed = s.trim_start();
    let lower = trimmed.to_lowercase();
    // Longer (-ly) variants first so they match before shorter ones.
    for ord in &[
        "firstly,",
        "secondly,",
        "thirdly,",
        "fourthly,",
        "fifthly,",
        "firstly ",
        "secondly ",
        "thirdly ",
        "fourthly ",
        "fifthly ",
        "first,",
        "second,",
        "third,",
        "fourth,",
        "fifth,",
        "first ",
        "second ",
        "third ",
        "fourth ",
        "fifth ",
    ] {
        if lower.starts_with(ord) {
            return trimmed[ord.len()..].trim_start();
        }
    }
    trimmed
}

/// True if the sentence leads with a spoken ordinal ("First,", "secondly …").
///
/// Used only to upgrade counted-header items to a numbered list — an
/// ordinal-only run is deliberately NOT a standalone list trigger (that
/// caused surprise bullets in ordinary dictation; the regression tests pin
/// ordinal-only runs to prose).
fn starts_with_ordinal(sentence: &str) -> bool {
    let trimmed = sentence.trim_start();
    strip_leading_ordinal(trimmed).len() != trimmed.len()
}

// ── Header detection ─────────────────────────────────────────────────────

/// Check if a sentence introduces a counted list ("these three things").
/// Returns the announced item count.
fn detect_list_header(sentence: &str) -> Option<usize> {
    let words: Vec<String> = sentence
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();

    // Explicit count + collection noun: "these three things"
    for (i, word) in words.iter().enumerate() {
        if let Some(count) = parse_count(word) {
            let end = (i + 3).min(words.len());
            for item in words.iter().take(end).skip(i + 1) {
                if COLLECTION_NOUNS.contains(&item.as_str()) {
                    return Some(count);
                }
            }
        }
    }

    None
}

// ── Join formatted parts ─────────────────────────────────────────────────

/// True if a formatted part is a list item (`- ` bullet or `1. ` number).
fn is_list_item(part: &str) -> bool {
    if part.starts_with("- ") {
        return true;
    }
    let bytes = part.as_bytes();
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    digits > 0 && bytes.len() > digits + 1 && bytes[digits] == b'.' && bytes[digits + 1] == b' '
}

/// Join formatted parts: regular sentences flow together with spaces,
/// list items (`- ` or `1. `) are newline-separated with a newline before
/// the first item.
fn join_parts(parts: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < parts.len() {
        if is_list_item(&parts[i]) {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            while i < parts.len() && is_list_item(&parts[i]) {
                out.push_str(&parts[i]);
                out.push('\n');
                i += 1;
            }
        } else {
            if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&parts[i]);
            i += 1;
        }
    }

    out.trim_end().to_string()
}

// ── Main entry point ─────────────────────────────────────────────────────

/// Minimum word count before list detection kicks in.  Short dictations
/// are almost never lists and shouldn't be reformatted.
const MIN_WORDS_FOR_LIST: usize = 40;

/// Detect list patterns in `text` and format them as lists.
///
/// The only auto-list trigger is an explicit **counted header** — "these
/// three things", "I have four steps" — where the user announces the list
/// out loud.  The N sentences after the header become items: `- ` bullets
/// normally, or a `1.` / `2.` numbered list when the items themselves lead
/// with spoken ordinals ("First, … Second, …").  Broad implicit heuristics
/// (repeated sentence starters, ordinal-only runs, inline comma lists) were
/// deliberately removed after they turned ordinary dictation into surprise
/// bullets; explicit list dictation is handled by the "bullet point" /
/// "number item" voice commands instead.
///
/// Because a counted header is an explicit signal, it is honored even on
/// short dictations ("I need three things. Milk. Eggs. Bread.").  All other
/// text below [`MIN_WORDS_FOR_LIST`] passes through untouched.
///
/// Pre-strips existing bullet/heading markers from input to avoid
/// double-marking.  A no-op when no list pattern is detected.
pub fn format_lists(text: &str) -> String {
    let long_enough = text.split_whitespace().count() >= MIN_WORDS_FOR_LIST;

    // Marker stripping: long text is always cleaned; short text only when it
    // carries explicit markers. The guard preserves brief numeric dictations
    // like "1. 5 million dollars" where Whisper inserted a space after a
    // decimal point; stripping first would delete the leading 1.
    let clean;
    let text: &str = if long_enough || has_explicit_markers(text) {
        clean = strip_existing_markers(text);
        &clean
    } else {
        text
    };

    // A counted list needs at least header + two items.
    let sentences = split_sentences(text);
    if sentences.len() < 3 {
        return text.to_string();
    }

    let mut parts: Vec<String> = Vec::new();
    let mut made_list = false;
    let mut i = 0;

    while i < sentences.len() {
        // Counted list header only. Broad implicit heuristics made ordinary
        // dictation turn into surprise bullets, so list creation requires
        // the user to say an explicit count such as "these three things".
        if let Some(n) = detect_list_header(&sentences[i]) {
            let remaining = sentences.len() - i - 1;
            if n >= 2 && remaining >= n {
                parts.push(sentences[i].clone());
                // When the items themselves lead with spoken ordinals
                // ("First, … Second, …") the user is dictating an ordered
                // list: number the items and strip the redundant ordinals.
                let ordinal_items = (1..=n)
                    .filter(|j| starts_with_ordinal(&sentences[i + j]))
                    .count();
                let numbered = ordinal_items * 2 >= n;
                for j in 1..=n {
                    let item = strip_leading_connector(&sentences[i + j]);
                    if numbered {
                        let item = capitalize_first(strip_leading_ordinal(item));
                        parts.push(format!("{j}. {item}"));
                    } else {
                        parts.push(format!("- {item}"));
                    }
                }
                made_list = true;
                i += n + 1;
                continue;
            }
        }

        // No pattern — pass through.
        parts.push(sentences[i].clone());
        i += 1;
    }

    // Short text that produced no list must round-trip byte-exact — don't
    // let sentence re-joining normalize its whitespace.
    if !made_list && !long_enough {
        return text.to_string();
    }

    join_parts(&parts)
}

#[cfg(test)]
#[path = "formatter_tests.rs"]
mod tests;
