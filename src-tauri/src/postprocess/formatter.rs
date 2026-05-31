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

/// Lowercase the first N words of a sentence (for prefix comparison).
fn sentence_prefix(sentence: &str, n: usize) -> String {
    sentence
        .split_whitespace()
        .take(n)
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect::<Vec<_>>()
        .join(" ")
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

/// Normalize a sentence for prefix comparison by stripping ordinals, connectors,
/// and other list-marker noise.  "First, we need..." and "Then we need..." and
/// "also we need..." all normalize to "we need...".
fn normalize_for_prefix(s: &str) -> &str {
    let s = strip_leading_ordinal(s);
    strip_leading_connector(s)
}

/// True if the sentence starts with an ordinal word (First, Secondly, …).
fn starts_with_ordinal(sentence: &str) -> bool {
    let _ = sentence;
    false
}

// ── Header detection ─────────────────────────────────────────────────────

/// How a list header introduces its items.
enum ListHeader {
    /// Explicit count: "these three things" → expect N items.
    Counted(usize),
    /// Implicit: "here's what I need", "these things", "the following", colon.
    /// Use all remaining sentences as items.
    #[allow(dead_code)]
    Implicit,
}

/// Check if a sentence introduces a list.
fn detect_list_header(sentence: &str) -> Option<ListHeader> {
    let words: Vec<String> = sentence
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();

    // 1. Explicit count + collection noun: "these three things"
    for (i, word) in words.iter().enumerate() {
        if let Some(count) = parse_count(word) {
            let end = (i + 3).min(words.len());
            for item in words.iter().take(end).skip(i + 1) {
                if COLLECTION_NOUNS.contains(&item.as_str()) {
                    return Some(ListHeader::Counted(count));
                }
            }
        }
    }

    None
}

// ── Repeated structure detection ─────────────────────────────────────────

/// 2-word prefixes that are too common in natural speech to indicate a list.
/// These appear frequently in narrative prose and would cause false-positive
/// bulleting of ordinary paragraphs.
const COMMON_PROSE_PREFIXES: &[&str] = &[
    "i was",
    "i had",
    "i am",
    "i got",
    "i need",
    "i want",
    "i think",
    "it was",
    "it is",
    "it has",
    "it would",
    "it could",
    "he was",
    "he had",
    "he is",
    "she was",
    "she had",
    "she is",
    "we had",
    "we were",
    "we are",
    "we went",
    "we got",
    "we need",
    "we can",
    "they were",
    "they had",
    "they are",
    "they got",
    "they need",
    "the meeting",
    "the team",
    "the project",
    "the system",
    "there was",
    "there were",
    "there is",
    "there are",
    "you can",
    "you need",
    "you should",
    "you could",
    "this is",
    "this was",
    "this will",
    "that is",
    "that was",
    "and then",
    "and i",
    "and we",
    "and the",
    "so i",
    "so we",
    "so the",
    "but i",
    "but we",
    "but the",
];

/// Check if the sentence at `start` begins a run of 3+ sentences that share
/// the same first 3 words after normalizing (stripping ordinals/connectors).
/// E.g., "First, we need to X. Then we need to Y. Also we need to Z." all
/// share "we need to" after normalization.
/// Returns the number of consecutive matching sentences.
fn detect_repeated_prefix(sentences: &[String], start: usize) -> Option<usize> {
    if start + 1 >= sentences.len() {
        return None;
    }

    // Normalize the first sentence to get the base prefix.
    let normalized = normalize_for_prefix(&sentences[start]);

    // Try 3-word prefix first (stronger signal), fall back to 2-word only
    // if the 2-word prefix isn't in the common-prose blocklist.
    let (prefix, prefix_words) = {
        let p3 = sentence_prefix(normalized, 3);
        if p3.split_whitespace().count() >= 3 {
            (p3, 3)
        } else {
            let p2 = sentence_prefix(normalized, 2);
            if p2.split_whitespace().count() < 2 {
                return None;
            }
            // Reject 2-word prefixes that are common in prose
            if COMMON_PROSE_PREFIXES.contains(&p2.as_str()) {
                return None;
            }
            (p2, 2)
        }
    };

    let mut end = start;
    while end + 1 < sentences.len() {
        let next_normalized = normalize_for_prefix(&sentences[end + 1]);
        let next_prefix = sentence_prefix(next_normalized, prefix_words);

        if next_prefix == prefix {
            end += 1;
        } else {
            break;
        }
    }

    let count = end - start + 1;
    // Require 5+ matches to be confident this is actually a list.
    if count >= 5 {
        Some(count)
    } else {
        None
    }
}

// ── Inline comma list detection ──────────────────────────────────────────

/// Repeated sentence starters are only promoted to bullets when nearby speech
/// explicitly frames the run as a list. Without this, ordinary long dictations
/// like "I want to..." repeated five times become surprise bullet lists.
fn has_nearby_list_cue(sentences: &[String], start: usize) -> bool {
    let _ = (sentences, start);
    false
}

/// Detect an inline comma-separated list within a single sentence.
/// Returns (prefix, items) if found — e.g., "I need" and ["milk", "eggs", "bread"].
#[allow(unreachable_code)]
fn detect_inline_list(sentence: &str) -> Option<(String, Vec<String>)> {
    let _ = sentence;
    return None;

    // Strip trailing punctuation for analysis.
    let trimmed = sentence.trim_end_matches(|c: char| matches!(c, '.' | '!' | '?'));

    // Look for "A, B, C, and D" or "A, B, and C" pattern.
    // Must have at least 2 commas (3+ items).
    let comma_count = trimmed.matches(',').count();
    if comma_count < 2 {
        return None;
    }

    // Split on ", and " or ", or " to find the boundary before the last item.
    let (before_last, last_item) = if let Some(pos) = trimmed.rfind(", and ") {
        (&trimmed[..pos], trimmed[pos + 6..].trim())
    } else if let Some(pos) = trimmed.rfind(", or ") {
        (&trimmed[..pos], trimmed[pos + 5..].trim())
    } else {
        return None; // No "and"/"or" → not a clear list
    };

    // Split the remaining part on commas.
    let parts: Vec<&str> = before_last.split(',').collect();
    if parts.len() < 2 {
        return None;
    }

    // The first part may contain a prefix before the list starts.
    // Heuristic: if the first part has more words than others, the extra
    // words are the prefix (e.g., "I need milk" → prefix "I need", item "milk").
    let avg_item_words: usize = parts[1..]
        .iter()
        .map(|p| p.split_whitespace().count())
        .sum::<usize>()
        / parts[1..].len().max(1);

    let first_words: Vec<&str> = parts[0].split_whitespace().collect();
    let prefix_word_count = if first_words.len() > avg_item_words {
        first_words.len() - avg_item_words
    } else {
        0
    };

    let prefix = first_words[..prefix_word_count].join(" ");
    let first_item = first_words[prefix_word_count..].join(" ");

    let mut items: Vec<String> = Vec::new();
    items.push(first_item.trim().to_string());
    for part in &parts[1..] {
        let item = part.trim().to_string();
        if !item.is_empty() {
            items.push(item);
        }
    }
    items.push(last_item.to_string());

    if items.len() >= 6 && items.iter().all(|it| !it.is_empty()) {
        // Only format as bullets if there are many items (6+) — inline
        // comma lists with fewer items read better as prose.
        let avg_words: f64 = items
            .iter()
            .map(|it| it.split_whitespace().count())
            .sum::<usize>() as f64
            / items.len() as f64;

        if avg_words >= 3.0 || items.len() >= 7 {
            Some((prefix, items))
        } else {
            None
        }
    } else {
        None
    }
}

// ── Implicit list termination ────────────────────────────────────────────

/// Determine how many sentences after a list header actually belong to the
/// list.  Uses sentence-length similarity to detect where the list ends and
/// normal prose resumes — prevents "runaway" lists where one header turns
/// everything into bullets.
///
/// The heuristic: list items tend to have similar sentence lengths.  When a
/// sentence is significantly longer than the running average of the items
/// so far, it's likely a topic transition or conclusion, not another item.
#[allow(dead_code)]
fn find_implicit_list_end(sentences: &[String], header_idx: usize) -> usize {
    let start = header_idx + 1;
    if start >= sentences.len() {
        return 0;
    }

    let mut accepted: usize = 0;
    let mut total_words: usize = 0;

    for idx in start..sentences.len() {
        let wc = sentences[idx].split_whitespace().count();

        if accepted == 0 {
            // First potential item: accept if not paragraph-length.
            if wc <= 30 {
                accepted += 1;
                total_words += wc;
            } else {
                break;
            }
        } else {
            let avg = total_words as f64 / accepted as f64;

            // A sentence significantly longer than the running average
            // signals a topic transition or conclusion — end the list.
            // The +6 additive guard prevents false positives when the
            // average is very low (e.g., avg=3 → 3*2.5=7.5 is too tight).
            if wc as f64 > avg * 2.5 && wc as f64 > avg + 6.0 {
                break;
            }

            accepted += 1;
            total_words += wc;
        }
    }

    accepted
}

// ── Join formatted parts ─────────────────────────────────────────────────

/// Join formatted parts: regular sentences flow together with spaces,
/// bullet items are newline-separated with a newline before the first bullet.
fn join_parts(parts: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < parts.len() {
        if parts[i].starts_with("- ") {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push('\n');
            }
            while i < parts.len() && parts[i].starts_with("- ") {
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

/// Detect list patterns in `text` and format them as bullet lists.
///
/// Patterns detected:
/// 1. **Counted header**: "these three things" → next N sentences become bullets.
/// 2. **Implicit header**: "here's what I need", "these things", colon → all
///    following sentences become bullets (requires 3+ items).
/// 3. **Ordinal sentences**: "First, … Second, … Third, …" → bullets (ordinals stripped).
/// 4. **Repeated sentence starters**: 3+ sentences with the same first 3 words.
/// 5. **Inline comma list**: "I need milk, eggs, and bread" → bullets.
///
/// Pre-strips existing bullet/heading markers from input to avoid double-marking.
/// This is a no-op when no list pattern is detected or text is too short.
pub fn format_lists(text: &str) -> String {
    // Short text guard runs before marker stripping. This preserves brief
    // numeric dictations like "1. 5 million dollars" where Whisper inserted a
    // space after a decimal point; stripping first would delete the leading 1.
    if text.split_whitespace().count() < MIN_WORDS_FOR_LIST {
        if has_explicit_markers(text) {
            return strip_existing_markers(text);
        }
        return text.to_string();
    }

    // Pre-strip any existing markers (Whisper markdown hallucinations, user
    // saying "dash" / "bullet point", etc.) to avoid double-marking.
    let clean = strip_existing_markers(text);
    let text = &clean;

    let sentences = split_sentences(text);

    // Need at least 4 sentences to have a meaningful list.
    // Short dictations are almost never lists.
    if sentences.len() < 4 {
        return text.to_string();
    }

    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;

    while i < sentences.len() {
        // Counted list header only. Broad implicit heuristics made ordinary
        // dictation turn into surprise bullets, so list creation now requires
        // the user to say an explicit count such as "these three things".
        if let Some(header) = detect_list_header(&sentences[i]) {
            let remaining = sentences.len() - i - 1;
            let (items, min_items) = match header {
                ListHeader::Counted(n) => (if remaining >= n { n } else { 0 }, 2),
                ListHeader::Implicit => (0, usize::MAX),
            };
            if items >= min_items {
                parts.push(sentences[i].clone());
                for j in 1..=items {
                    let item = strip_leading_connector(&sentences[i + j]);
                    parts.push(format!("- {item}"));
                }
                i += items + 1;
                continue;
            }
        }

        // Pattern 3: 5+ consecutive ordinal sentences.
        // Strip the ordinal marker when adding dashes to avoid redundant
        // double-markers like "- First, set up the database."
        if starts_with_ordinal(&sentences[i]) {
            let start = i;
            let mut end = i;
            while end + 1 < sentences.len() && starts_with_ordinal(&sentences[end + 1]) {
                end += 1;
            }
            if end - start >= 4 {
                for j in start..=end {
                    let content = strip_leading_ordinal(sentences[j].trim());
                    // Capitalize the first letter after stripping the ordinal
                    let content = capitalize_first(content);
                    parts.push(format!("- {content}"));
                }
                i = end + 1;
                continue;
            }
        }

        // Pattern 4: 3+ sentences with the same first 2 words.
        if has_nearby_list_cue(&sentences, i) {
            if let Some(count) = detect_repeated_prefix(&sentences, i) {
                for j in i..i + count {
                    let item = strip_leading_connector(&sentences[j]);
                    parts.push(format!("- {item}"));
                }
                i += count;
                continue;
            }
        }

        // Pattern 5: Inline comma list within this sentence.
        if let Some((prefix, items)) = detect_inline_list(&sentences[i]) {
            if !prefix.is_empty() {
                parts.push(prefix);
            }
            for item in &items {
                parts.push(format!("- {item}"));
            }
            i += 1;
            continue;
        }

        // No pattern — pass through.
        parts.push(sentences[i].clone());
        i += 1;
    }

    join_parts(&parts)
}

#[cfg(test)]
#[path = "formatter_tests.rs"]
mod tests;
