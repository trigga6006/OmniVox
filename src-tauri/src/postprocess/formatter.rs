//! Post-LLM text formatter.
//!
//! Detects list patterns in cleaned text and applies bullet formatting.
//! Runs *after* the LLM cleanup step so the small model only has to handle
//! grammar/filler cleanup — structural formatting is handled here with
//! deterministic heuristics at zero inference cost.

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

/// Check if a sentence introduces a numbered list.
/// Returns the expected item count when a pattern like "these three things"
/// or "the following 5 items" is found.
fn detect_list_header(sentence: &str) -> Option<usize> {
    let words: Vec<String> = sentence
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase()
        })
        .collect();

    for (i, word) in words.iter().enumerate() {
        if let Some(count) = parse_count(word) {
            // Look for a collection noun within 2 words after the count.
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

/// True if the sentence starts with an ordinal word (First, Second, …).
fn starts_with_ordinal(sentence: &str) -> bool {
    let lower = sentence.trim_start().to_lowercase();
    for prefix in &[
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

/// Strip a leading "And " from a sentence (common in spoken lists).
fn strip_leading_and(s: &str) -> &str {
    let trimmed = s.trim_start();
    if trimmed.len() >= 4 {
        let prefix = &trimmed[..4];
        if prefix.eq_ignore_ascii_case("and ") {
            return trimmed[4..].trim_start();
        }
    }
    trimmed
}

/// Join formatted parts: regular sentences flow together with spaces,
/// bullet items are newline-separated with a newline before the first bullet.
fn join_parts(parts: &[String]) -> String {
    let mut out = String::new();
    let mut i = 0;

    while i < parts.len() {
        if parts[i].starts_with("- ") {
            // Newline before bullet section (unless at the very start).
            if !out.is_empty() {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            // Emit all consecutive bullets.
            while i < parts.len() && parts[i].starts_with("- ") {
                out.push_str(&parts[i]);
                out.push('\n');
                i += 1;
            }
        } else {
            // Regular sentence — append with a space separator.
            if !out.is_empty() && !out.ends_with('\n') && !out.ends_with(' ') {
                out.push(' ');
            }
            out.push_str(&parts[i]);
            i += 1;
        }
    }

    out.trim_end().to_string()
}

/// Detect list patterns in `text` and format them as bullet lists.
///
/// This is a no-op when no list pattern is detected — safe to call on every
/// transcription without overhead.
pub fn format_lists(text: &str) -> String {
    let sentences = split_sentences(text);

    // Need at least a header + 2 items (or 3 ordinal sentences).
    if sentences.len() < 3 {
        return text.to_string();
    }

    let mut parts: Vec<String> = Vec::new();
    let mut i = 0;

    while i < sentences.len() {
        // Pattern 1: "these three things" header → next N sentences become bullets.
        if let Some(count) = detect_list_header(&sentences[i]) {
            let remaining = sentences.len() - i - 1;
            if count >= 2 && remaining >= count {
                parts.push(sentences[i].clone());
                for j in 1..=count {
                    let item = strip_leading_and(&sentences[i + j]);
                    parts.push(format!("- {item}"));
                }
                i += count + 1;
                continue;
            }
        }

        // Pattern 2: 3+ consecutive sentences starting with ordinals.
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
        // Header and intro should still be present.
        assert!(result.contains("I'm testing the cleaning ability to format text."));
        assert!(result.contains("I want these three things tested."));
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
    fn strips_leading_and() {
        let input = "I have two tasks. Do the first thing. And do the second thing.";
        let result = format_lists(input);
        assert!(result.contains("- Do the first thing."));
        assert!(result.contains("- do the second thing."));
    }
}
