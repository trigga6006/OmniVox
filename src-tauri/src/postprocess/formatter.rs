//! Post-LLM text formatter.
//!
//! Detects list patterns in cleaned text and applies bullet formatting.
//! Runs *after* the LLM cleanup step so the small model only has to handle
//! grammar/filler cleanup — structural formatting is handled here with
//! deterministic heuristics at zero inference cost.

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
    "things", "items", "points", "tasks", "reasons", "steps",
    "ways", "features", "goals", "topics", "parts", "changes",
    "updates", "issues", "problems", "areas", "aspects", "options",
    "requirements", "examples", "notes", "priorities",
];

/// Split text into sentences on `.` `!` `?`, keeping the delimiter attached.
fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for c in text.chars() {
        current.push(c);
        if matches!(c, '.' | '!' | '?') {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            current.clear();
        }
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
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Strip a leading connector word from a sentence (common in spoken lists).
fn strip_leading_connector(s: &str) -> &str {
    let trimmed = s.trim_start();
    for prefix in &["and ", "then ", "also ", "plus ", "or ", "next ", "finally ", "lastly "] {
        if trimmed.len() >= prefix.len()
            && trimmed[..prefix.len()].eq_ignore_ascii_case(prefix)
        {
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
        "firstly,", "secondly,", "thirdly,", "fourthly,", "fifthly,",
        "firstly ", "secondly ", "thirdly ", "fourthly ", "fifthly ",
        "first,", "second,", "third,", "fourth,", "fifth,",
        "first ", "second ", "third ", "fourth ", "fifth ",
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
    let lower = sentence.trim_start().to_lowercase();
    for prefix in &[
        // -ly variants first (longer match wins).
        "firstly", "secondly", "thirdly", "fourthly", "fifthly",
        "first", "second", "third", "fourth", "fifth",
        "sixth", "seventh", "eighth", "ninth", "tenth",
    ] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            if rest.starts_with(',') || rest.starts_with(':') || rest.starts_with(' ') {
                return true;
            }
        }
    }
    false
}

// ── Header detection ─────────────────────────────────────────────────────

/// How a list header introduces its items.
enum ListHeader {
    /// Explicit count: "these three things" → expect N items.
    Counted(usize),
    /// Implicit: "here's what I need", "these things", "the following", colon.
    /// Use all remaining sentences as items.
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

    // 2. Collection noun without count: "these things", "my tasks"
    //    Requires a determiner/possessive near the collection noun.
    let determiners = ["these", "those", "the", "my", "our", "your", "some", "several"];
    for (i, word) in words.iter().enumerate() {
        if COLLECTION_NOUNS.contains(&word.as_str()) {
            // Check if a determiner appears within 2 words before the noun.
            let start = i.saturating_sub(2);
            for det in words.iter().take(i).skip(start) {
                if determiners.contains(&det.as_str()) {
                    return Some(ListHeader::Implicit);
                }
            }
        }
    }

    let lower = sentence.to_lowercase();

    // 3. Quantifier phrases: "a couple of things", "a few items", "a number of tasks"
    let quantifiers = ["a couple of", "a couple", "a few", "a number of", "a bunch of"];
    for qp in &quantifiers {
        if let Some(pos) = lower.find(qp) {
            let after = lower[pos + qp.len()..].trim_start();
            // Skip "of" if the quantifier doesn't already end with it.
            let check = if after.starts_with("of ") { &after[3..] } else { after };
            let next_word = check.trim_start().split_whitespace().next().unwrap_or("");
            if COLLECTION_NOUNS.contains(&next_word) {
                return Some(ListHeader::Implicit);
            }
        }
    }

    // 4. Signal phrases: "the following", "as follows", "here's what", etc.
    let signals = [
        "the following",
        "as follows",
        "here's what",
        "here is what",
        "here are the",
        "here are some",
        "here is a list",
        "i need to do",
        "we need to do",
        "you need to do",
    ];
    for sig in &signals {
        if lower.contains(sig) {
            return Some(ListHeader::Implicit);
        }
    }

    // 5. Sentence ends with a colon.
    if sentence.trim_end().ends_with(':') {
        return Some(ListHeader::Implicit);
    }

    None
}

// ── Repeated structure detection ─────────────────────────────────────────

/// Check if the sentence at `start` begins a run of 3+ sentences that share
/// the same first 2 words after normalizing (stripping ordinals/connectors).
/// E.g., "First, we need X. Then we need Y. Also we need Z." all share
/// "we need" after normalization.
/// Returns the number of consecutive matching sentences.
fn detect_repeated_prefix(sentences: &[String], start: usize) -> Option<usize> {
    if start + 1 >= sentences.len() {
        return None;
    }

    // Normalize the first sentence to get the base prefix.
    let normalized = normalize_for_prefix(&sentences[start]);
    let prefix = sentence_prefix(normalized, 2);
    if prefix.split_whitespace().count() < 2 {
        return None;
    }

    let mut end = start;
    while end + 1 < sentences.len() {
        let next_normalized = normalize_for_prefix(&sentences[end + 1]);
        let next_prefix = sentence_prefix(next_normalized, 2);

        if next_prefix == prefix {
            end += 1;
        } else {
            break;
        }
    }

    let count = end - start + 1;
    // Require 3+ matches normally, but 2+ if the first sentence starts with
    // "First," — a very strong signal that a list is starting.
    let threshold = if starts_with_ordinal(&sentences[start]) { 2 } else { 3 };
    if count >= threshold {
        Some(count)
    } else {
        None
    }
}

// ── Inline comma list detection ──────────────────────────────────────────

/// Detect an inline comma-separated list within a single sentence.
/// Returns (prefix, items) if found — e.g., "I need" and ["milk", "eggs", "bread"].
fn detect_inline_list(sentence: &str) -> Option<(String, Vec<String>)> {
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
    let avg_item_words: usize = parts[1..].iter()
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

    if items.len() >= 3 && items.iter().all(|it| !it.is_empty()) {
        // Only format as bullets if the items are substantial enough to
        // benefit from vertical layout.  Short 1-2 word enumerations
        // (e.g., "red, blue, and green") read better inline.
        // Threshold: average ≥ 3 words per item, OR 5+ items.
        let avg_words: f64 = items.iter()
            .map(|it| it.split_whitespace().count())
            .sum::<usize>() as f64 / items.len() as f64;

        if avg_words >= 3.0 || items.len() >= 5 {
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

/// Detect list patterns in `text` and format them as bullet lists.
///
/// Patterns detected:
/// 1. **Counted header**: "these three things" → next N sentences become bullets.
/// 2. **Implicit header**: "here's what I need", "these things", colon → all
///    following sentences become bullets.
/// 3. **Ordinal sentences**: "First, … Second, … Third, …" → bullets.
/// 4. **Repeated sentence starters**: 3+ sentences with the same first 2 words.
/// 5. **Inline comma list**: "I need milk, eggs, and bread" → bullets.
///
/// This is a no-op when no list pattern is detected.
pub fn format_lists(text: &str) -> String {
    let sentences = split_sentences(text);

    // Single-sentence: only inline comma lists can match.
    if sentences.len() == 1 {
        if let Some((prefix, items)) = detect_inline_list(&sentences[0]) {
            let mut out = String::new();
            if !prefix.is_empty() {
                out.push_str(&prefix);
                out.push('\n');
            }
            for item in &items {
                out.push_str(&format!("- {item}\n"));
            }
            return out.trim_end().to_string();
        }
        return text.to_string();
    }

    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;

    while i < sentences.len() {
        // Pattern 1 & 2: List header (counted or implicit).
        if let Some(header) = detect_list_header(&sentences[i]) {
            let remaining = sentences.len() - i - 1;
            let items = match header {
                ListHeader::Counted(n) => {
                    if remaining >= n { n } else { remaining }
                }
                // Implicit headers use smart termination: scan forward
                // until a sentence is too different (much longer) from the
                // other items, signalling a topic transition / conclusion.
                ListHeader::Implicit => find_implicit_list_end(&sentences, i),
            };
            if items >= 2 {
                parts.push(sentences[i].clone());
                for j in 1..=items {
                    let item = strip_leading_connector(&sentences[i + j]);
                    parts.push(format!("- {item}"));
                }
                i += items + 1;
                continue;
            }
        }

        // Pattern 3: 3+ consecutive ordinal sentences.
        if starts_with_ordinal(&sentences[i]) {
            let start = i;
            let mut end = i;
            while end + 1 < sentences.len() && starts_with_ordinal(&sentences[end + 1]) {
                end += 1;
            }
            if end - start >= 2 {
                for j in start..=end {
                    parts.push(format!("- {}", sentences[j].trim()));
                }
                i = end + 1;
                continue;
            }
        }

        // Pattern 4: 3+ sentences with the same first 2 words.
        if let Some(count) = detect_repeated_prefix(&sentences, i) {
            for j in i..i + count {
                let item = strip_leading_connector(&sentences[j]);
                parts.push(format!("- {item}"));
            }
            i += count;
            continue;
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
mod tests {
    use super::*;

    #[test]
    fn count_word_header() {
        let input = "I'm testing the cleaning ability to format text. \
                     I want these three things tested. \
                     I want to test the maximum number of outputs. \
                     I want to get the token count at least above 500. \
                     And I want to see how many people are in chat.";
        let result = format_lists(input);
        assert!(result.contains("- I want to test the maximum number of outputs."));
        assert!(result.contains("- I want to get the token count at least above 500."));
        assert!(result.contains("- I want to see how many people are in chat."));
        assert!(result.contains("I'm testing the cleaning ability to format text."));
        assert!(result.contains("I want these three things tested."));
    }

    #[test]
    fn implicit_header_no_count() {
        // "these things" without a number.
        let input = "I want to test these things. \
                     Do a Unicode test. \
                     Do a transformer test. \
                     Get catch up on the burger.";
        let result = format_lists(input);
        assert!(result.contains("- Do a Unicode test."));
        assert!(result.contains("- Do a transformer test."));
        assert!(result.contains("- Get catch up on the burger."));
    }

    #[test]
    fn signal_phrase_the_following() {
        let input = "I need to do the following. \
                     Update the database. \
                     Fix the tests. \
                     Deploy to production.";
        let result = format_lists(input);
        assert!(result.contains("- Update the database."));
        assert!(result.contains("- Fix the tests."));
        assert!(result.contains("- Deploy to production."));
    }

    #[test]
    fn ordinal_sentences() {
        let input = "Here is the plan. \
                     First, set up the database. \
                     Second, write the API endpoints. \
                     Third, build the frontend.";
        let result = format_lists(input);
        assert!(result.contains("- First, set up the database."));
        assert!(result.contains("- Second, write the API endpoints."));
        assert!(result.contains("- Third, build the frontend."));
        assert!(result.starts_with("Here is the plan."));
    }

    #[test]
    fn repeated_sentence_starters() {
        let input = "I want to do a Unicode test. \
                     I want to do a transformer test. \
                     I want to check the output format.";
        let result = format_lists(input);
        assert!(result.contains("- I want to do a Unicode test."));
        assert!(result.contains("- I want to do a transformer test."));
        assert!(result.contains("- I want to check the output format."));
    }

    #[test]
    fn repeated_starters_with_connector() {
        // Last item starts with "And" but shares the same prefix after stripping.
        let input = "I need to fix the bug. \
                     I need to update the docs. \
                     And I need to run the tests.";
        let result = format_lists(input);
        assert!(result.contains("- I need to fix the bug."));
        assert!(result.contains("- I need to update the docs."));
        assert!(result.contains("- I need to run the tests."));
    }

    #[test]
    fn inline_comma_list_short_items_stay_inline() {
        // Short 1-2 word items should stay inline — bulleting them is
        // over-formatting for simple enumerations.
        let input = "I need milk, eggs, bread, and butter.";
        let result = format_lists(input);
        assert_eq!(result, input, "Short items should not be bulleted");
    }

    #[test]
    fn inline_comma_list_substantial_items() {
        // Multi-word items (avg ≥ 3 words) DO get bulleted.
        let input = "I need to update the database, fix the API tests, refactor the auth module, and deploy to production.";
        let result = format_lists(input);
        assert!(result.contains("- to update the database") || result.contains("- update the database"),
                "Substantial items should be bulleted: {result}");
        assert!(result.contains("- fix the API tests"));
        assert!(result.contains("- deploy to production"));
    }

    #[test]
    fn no_list_passthrough() {
        let input = "I went to the store. I bought some milk. I came home.";
        assert_eq!(format_lists(input), input);
    }

    #[test]
    fn too_short_passthrough() {
        let input = "Hello world.";
        assert_eq!(format_lists(input), input);
    }

    #[test]
    fn fewer_items_than_stated_count() {
        let input = "I want to test these three things. \
                     I want to do a Unicode test. \
                     I want to do a transformer test.";
        let result = format_lists(input);
        assert!(result.contains("- I want to do a Unicode test."));
        assert!(result.contains("- I want to do a transformer test."));
        assert!(result.contains("I want to test these three things."));
    }

    #[test]
    fn couple_of_things_with_mixed_connectors() {
        // "a couple of things" header + items with "First,", "Then", "also"
        let input = "I really like where the design is going but there's a couple of things I want to change. \
                     First, we need to move the header down about 3 inches. \
                     Then we need to adjust the desert section and also we need to change where the lens comes in.";
        let result = format_lists(input);
        assert!(result.contains("- "), "Expected bullet items but got: {result}");
        assert!(result.contains("we need to move the header"));
        assert!(result.contains("we need to adjust the desert section"));
    }

    #[test]
    fn first_then_also_pattern() {
        // "First," + "Then" + shared "we need" prefix after stripping connectors.
        let input = "First, we need to update the CSS. \
                     Then we need to fix the layout. \
                     Also we need to add the footer.";
        let result = format_lists(input);
        assert!(result.contains("- "));
        assert!(result.contains("we need to update the CSS."));
        assert!(result.contains("we need to fix the layout."));
        assert!(result.contains("we need to add the footer."));
    }

    #[test]
    fn implicit_list_terminates_at_conclusion() {
        // After a run of short list items, a significantly longer sentence
        // should NOT be bulleted — it's a conclusion / topic transition.
        let input = "Here's the things we added. \
                     We stripped bullet markers. \
                     We stripped heading markers. \
                     We stripped inline bold. \
                     We rejoined all lines into flowing text. \
                     The formatting ability is fully preserved and still handles all the smart list detection properly.";
        let result = format_lists(input);
        // Items should be bulleted.
        assert!(result.contains("- We stripped bullet markers."), "Items should be bulleted: {result}");
        assert!(result.contains("- We stripped heading markers."));
        // The long conclusion should NOT be bulleted.
        assert!(
            !result.contains("- The formatting ability"),
            "Conclusion should NOT be bulleted: {result}"
        );
        assert!(result.contains("The formatting ability is fully preserved"));
    }

    #[test]
    fn header_not_bulleted_couple_things() {
        // "a couple things" (no "of") is a header — should NOT be bulleted.
        let input = "I want to get a couple things done today. \
                     First, I want to check how the LLM removes filters. \
                     Second, I want to fix punctuation. \
                     Thirdly, I want to rewrite or shorten and add length.";
        let result = format_lists(input);
        // Header stays as plain text (no bullet).
        assert!(
            !result.starts_with("- I want to get"),
            "Header should NOT be bulleted: {result}"
        );
        assert!(result.contains("I want to get a couple things done today."));
        // Items are bulleted.
        assert!(result.contains("- "));
        assert!(result.contains("I want to check how the LLM removes filters."));
        assert!(result.contains("I want to fix punctuation."));
        assert!(result.contains("I want to rewrite or shorten and add length."));
    }
}
