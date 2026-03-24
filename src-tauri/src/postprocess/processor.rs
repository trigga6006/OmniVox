use crate::error::AppResult;
use crate::postprocess::types::{Correction, ProcessedText, ProcessorConfig};
use crate::storage::types::{DictionaryEntry, Snippet};

/// Trait for text post-processing steps.
pub trait TextProcessor: Send + Sync {
    fn process(&self, text: &str) -> AppResult<ProcessedText>;
}

/// Pipeline that applies multiple post-processing steps to transcribed text.
///
/// Steps run in order: filler removal → contextual fillers → phrase dedup →
/// dictionary replacement → snippet expansion → capitalization →
/// whitespace cleanup → punctuation cleanup.
/// Each step records the corrections it makes for transparency in the UI.
pub struct ProcessorChain {
    config: ProcessorConfig,
    dictionary: Vec<DictionaryEntry>,
    snippets: Vec<Snippet>,
}

impl ProcessorChain {
    pub fn new(config: ProcessorConfig) -> Self {
        Self {
            config,
            dictionary: Vec::new(),
            snippets: Vec::new(),
        }
    }

    /// Update the dictionary entries used for replacement.
    /// Call this when the user adds/removes dictionary entries.
    pub fn set_dictionary(&mut self, entries: Vec<DictionaryEntry>) {
        self.dictionary = entries;
    }

    /// Update the snippets used for trigger-word expansion.
    /// Call this when the user adds/removes snippets.
    pub fn set_snippets(&mut self, snippets: Vec<Snippet>) {
        self.snippets = snippets;
    }
}

impl TextProcessor for ProcessorChain {
    fn process(&self, text: &str) -> AppResult<ProcessedText> {
        let original = text.to_string();
        let mut result = text.to_string();
        let mut corrections = Vec::new();

        // Step 0: Remove filler words and deduplicate repeated words
        if self.config.apply_filler_removal {
            let before = result.clone();
            result = remove_fillers(&result);
            if result != before {
                corrections.push(Correction {
                    original: before,
                    replacement: result.clone(),
                    reason: "filler_removal".to_string(),
                });
            }
        }

        // Step 0b: Context-sensitive fillers (sentence-start "so", "well", filler "like")
        if self.config.apply_filler_removal {
            let before = result.clone();
            result = remove_contextual_fillers(&result);
            if result != before {
                corrections.push(Correction {
                    original: before,
                    replacement: result.clone(),
                    reason: "contextual_filler_removal".to_string(),
                });
            }
        }

        // Step 0c: Deduplicate repeated 2-3 word phrases ("I think I think" → "I think")
        if self.config.apply_filler_removal {
            let before = result.clone();
            result = dedup_phrases(&result);
            if result != before {
                corrections.push(Correction {
                    original: before,
                    replacement: result.clone(),
                    reason: "phrase_dedup".to_string(),
                });
            }
        }

        // Step 1: Dictionary replacements (case-insensitive)
        // Pre-compute lowercase once and short-circuit entries that can't match.
        if self.config.apply_dictionary && !self.dictionary.is_empty() {
            let mut lower_result = result.to_lowercase();
            for entry in &self.dictionary {
                if !entry.is_enabled {
                    continue;
                }
                let lower_phrase = entry.phrase.to_lowercase();
                if !lower_result.contains(&lower_phrase) {
                    continue;
                }
                if let Some(replaced) =
                    replace_case_insensitive(&result, &entry.phrase, &entry.replacement)
                {
                    corrections.push(Correction {
                        original: entry.phrase.clone(),
                        replacement: entry.replacement.clone(),
                        reason: "dictionary".to_string(),
                    });
                    result = replaced;
                    lower_result = result.to_lowercase();
                }
            }
        }

        // Step 2: Snippet expansion (trigger word → expanded content)
        if self.config.apply_dictionary && !self.snippets.is_empty() {
            let mut lower_result = result.to_lowercase();
            for snippet in &self.snippets {
                if !snippet.is_enabled {
                    continue;
                }
                let lower_trigger = snippet.trigger.to_lowercase();
                if !lower_result.contains(&lower_trigger) {
                    continue;
                }
                if let Some(replaced) =
                    replace_case_insensitive(&result, &snippet.trigger, &snippet.content)
                {
                    corrections.push(Correction {
                        original: snippet.trigger.clone(),
                        replacement: snippet.content.clone(),
                        reason: "snippet".to_string(),
                    });
                    result = replaced;
                    lower_result = result.to_lowercase();
                }
            }
        }

        // Step 3: Capitalize first letter of sentences
        if self.config.auto_capitalize {
            let before = result.clone();
            result = capitalize_sentences(&result);
            if result != before {
                corrections.push(Correction {
                    original: before,
                    replacement: result.clone(),
                    reason: "auto_capitalize".to_string(),
                });
            }
        }

        // Step 4: Clean up whitespace (collapse multiple spaces, trim)
        let before = result.clone();
        result = normalize_whitespace(&result);
        if result != before {
            corrections.push(Correction {
                original: before,
                replacement: result.clone(),
                reason: "whitespace_cleanup".to_string(),
            });
        }

        // Step 5: Punctuation cleanup (double periods, space before punctuation, trailing connectors)
        if self.config.apply_filler_removal {
            let before = result.clone();
            result = cleanup_punctuation(&result);
            if result != before {
                corrections.push(Correction {
                    original: before,
                    replacement: result.clone(),
                    reason: "punctuation_cleanup".to_string(),
                });
            }
        }

        Ok(ProcessedText {
            original,
            processed: result,
            corrections,
        })
    }
}

/// True if a byte is part of a "word" for dictionary matching purposes.
/// Includes alphanumerics, apostrophes (contractions like "can't"), and
/// hyphens (compound words like "real-time").
fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'\'' || b == b'\xE2' || b == b'-'
    // 0xE2 is the first byte of the UTF-8 sequence for U+2019 (right single
    // quotation mark / typographic apostrophe "…'t").  A full multi-byte
    // check isn't needed here — the byte never appears as a lead byte in
    // any other common context, and false positives are harmless (they only
    // prevent a replacement, never cause one).
}

/// Case-insensitive whole-word replacement. Returns `Some(new_string)` if any
/// replacement was made, `None` if the phrase wasn't found.
fn replace_case_insensitive(text: &str, phrase: &str, replacement: &str) -> Option<String> {
    let lower_text = text.to_lowercase();
    let lower_phrase = phrase.to_lowercase();

    if !lower_text.contains(&lower_phrase) {
        return None;
    }

    // Build result by finding all occurrences (case-insensitive)
    let mut result = String::with_capacity(text.len());
    let mut search_start = 0;

    while let Some(pos) = lower_text[search_start..].find(&lower_phrase) {
        let abs_pos = search_start + pos;

        // Check word boundaries to avoid replacing inside contractions
        // or compound words (e.g. "can" inside "can't").
        let at_word_start =
            abs_pos == 0 || !is_word_char(text.as_bytes()[abs_pos - 1]);
        let end_pos = abs_pos + phrase.len();
        let at_word_end =
            end_pos >= text.len() || !is_word_char(text.as_bytes()[end_pos]);

        if at_word_start && at_word_end {
            result.push_str(&text[search_start..abs_pos]);
            result.push_str(replacement);
            search_start = end_pos;
        } else {
            result.push_str(&text[search_start..abs_pos + 1]);
            search_start = abs_pos + 1;
        }
    }

    result.push_str(&text[search_start..]);

    if result == text {
        None
    } else {
        Some(result)
    }
}

/// Capitalize the first letter of the string and the first letter after
/// sentence-ending punctuation (. ! ?).
fn capitalize_sentences(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = true;

    for ch in text.chars() {
        if capitalize_next && ch.is_alphabetic() {
            result.extend(ch.to_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
            if ch == '.' || ch == '!' || ch == '?' {
                capitalize_next = true;
            }
        }
    }

    result
}

/// Remove filler words and deduplicate consecutive repeated words.
///
/// Handles single-word fillers (um, uh, hmm, er, ah), multi-word fillers
/// (you know, I mean, sort of, kind of), and repeated consecutive words
/// ("I I" → "I", "the the" → "the").  Cleans up orphaned punctuation
/// left behind after removal.
fn remove_fillers(text: &str) -> String {
    // Single-word fillers — always safe to remove
    const SINGLE_FILLERS: &[&str] = &[
        "um", "uh", "uhh", "hmm", "hm", "er", "ah", "erm", "eh",
        "basically", "actually", "literally",
    ];

    // Multi-word filler phrases — removed as a unit
    const PHRASE_FILLERS: &[&str] = &[
        "you know what i mean", // longest first to avoid partial matches
        "you know", "i mean", "sort of", "kind of", "okay so",
    ];

    let mut result = text.to_string();

    // 1. Remove multi-word filler phrases first (before splitting into words)
    for phrase in PHRASE_FILLERS {
        // Case-insensitive removal with word boundaries
        let lower = result.to_lowercase();
        let mut new = String::with_capacity(result.len());
        let mut search_start = 0;

        while let Some(pos) = lower[search_start..].find(phrase) {
            let abs_pos = search_start + pos;
            let end_pos = abs_pos + phrase.len();

            // Check word boundaries
            let at_start = abs_pos == 0
                || !result.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
            let at_end = end_pos >= result.len()
                || !result.as_bytes()[end_pos].is_ascii_alphanumeric();

            if at_start && at_end {
                new.push_str(&result[search_start..abs_pos]);
                // Skip a trailing comma + space if present (", you know," → ",")
                search_start = end_pos;
            } else {
                new.push_str(&result[search_start..abs_pos + 1]);
                search_start = abs_pos + 1;
            }
        }
        new.push_str(&result[search_start..]);
        result = new;
    }

    // 2. Split into words and remove single-word fillers + consecutive dupes
    let words: Vec<&str> = result.split_whitespace().collect();
    let mut cleaned: Vec<&str> = Vec::with_capacity(words.len());

    for word in &words {
        // Strip trailing punctuation for comparison
        let bare = word.trim_matches(|c: char| !c.is_alphanumeric());
        let lower_bare = bare.to_lowercase();

        // Skip single-word fillers
        if SINGLE_FILLERS.contains(&lower_bare.as_str()) {
            continue;
        }

        // Skip consecutive duplicate words ("I I" → "I")
        if let Some(prev) = cleaned.last() {
            let prev_bare = prev.trim_matches(|c: char| !c.is_alphanumeric());
            if !bare.is_empty()
                && bare.to_lowercase() == prev_bare.to_lowercase()
            {
                continue;
            }
        }

        cleaned.push(word);
    }

    let mut result = cleaned.join(" ");

    // 3. Clean up orphaned punctuation left by removals
    // ", ," → ","   and  ",  ." → "."
    while result.contains(", ,") {
        result = result.replace(", ,", ",");
    }
    while result.contains("  ") {
        result = result.replace("  ", " ");
    }
    // Remove leading comma after removal
    result = result.trim_start_matches(", ").to_string();
    result = result.trim_start_matches(',').trim_start().to_string();

    result.trim().to_string()
}

/// Remove context-sensitive filler words that are only fillers in specific positions.
///
/// - "so" / "well" / "right" / "okay" at sentence start (after . ! ? or start of text)
/// - ", like," filler pattern (but NOT "I like pizza")
fn remove_contextual_fillers(text: &str) -> String {
    const SENTENCE_START_FILLERS: &[&str] = &[
        "so basically ", "so ", "well ", "right ", "okay ",
    ];

    // Split on sentence boundaries, strip leading fillers from each sentence
    let mut result = String::with_capacity(text.len());
    let mut remainder = text;

    // Process the text as a series of sentences
    while !remainder.is_empty() {
        // Find next sentence boundary
        let boundary = remainder
            .find(|c: char| c == '.' || c == '!' || c == '?')
            .map(|pos| pos + 1)
            .unwrap_or(remainder.len());

        let sentence = &remainder[..boundary];
        let trimmed = sentence.trim_start();

        // Try stripping each sentence-start filler (longest first)
        let mut stripped = trimmed;
        for filler in SENTENCE_START_FILLERS {
            if stripped.to_lowercase().starts_with(filler) {
                stripped = &stripped[filler.len()..];
                break;
            }
        }

        if !result.is_empty() && !stripped.is_empty() && !result.ends_with(' ') {
            result.push(' ');
        }
        result.push_str(stripped);
        remainder = &remainder[boundary..];
    }

    // Remove ", like," filler pattern (comma-delimited filler "like")
    let mut cleaned = result;
    for pattern in &[", like,", ", like ", ",like,", ",like "] {
        while cleaned.to_lowercase().contains(pattern) {
            let lower = cleaned.to_lowercase();
            if let Some(pos) = lower.find(pattern) {
                let replacement = if pattern.ends_with(',') { "," } else { " " };
                cleaned = format!("{}{}{}", &cleaned[..pos], replacement, &cleaned[pos + pattern.len()..]);
            }
        }
    }

    cleaned
}

/// Remove repeated consecutive 2-3 word phrases.
///
/// "I think I think we should" → "I think we should"
/// "we need to we need to fix" → "we need to fix"
///
/// Only removes when the EXACT phrase repeats consecutively (case-insensitive).
fn dedup_phrases(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 4 {
        return text.to_string();
    }

    let mut result: Vec<&str> = Vec::with_capacity(words.len());
    let mut i = 0;

    while i < words.len() {
        let mut found_dup = false;

        // Try 3-word phrases first, then 2-word
        for phrase_len in (2..=3).rev() {
            if i + phrase_len * 2 <= words.len() {
                let phrase_a: Vec<String> = words[i..i + phrase_len]
                    .iter()
                    .map(|w| w.to_lowercase())
                    .collect();
                let phrase_b: Vec<String> = words[i + phrase_len..i + phrase_len * 2]
                    .iter()
                    .map(|w| w.to_lowercase())
                    .collect();

                if phrase_a == phrase_b {
                    // Keep the first occurrence, skip the duplicate
                    for j in i..i + phrase_len {
                        result.push(words[j]);
                    }
                    i += phrase_len * 2;
                    found_dup = true;
                    break;
                }
            }
        }

        if !found_dup {
            result.push(words[i]);
            i += 1;
        }
    }

    result.join(" ")
}

/// Clean up punctuation artifacts from transcription and filler removal.
///
/// - Double periods ("..") → "."  (preserves real ellipsis "...")
/// - Space before punctuation ("word .") → "word."
/// - Trailing connectors ("and" / "so" / "but") from interrupted speech → removed
/// - Ensures text ends with sentence-ending punctuation (adds "." if missing)
fn cleanup_punctuation(text: &str) -> String {
    let mut result = text.to_string();

    // Space before sentence-ending punctuation
    result = result.replace(" .", ".");
    result = result.replace(" ,", ",");
    result = result.replace(" !", "!");
    result = result.replace(" ?", "?");
    result = result.replace(" ;", ";");
    result = result.replace(" :", ":");

    // Double periods (but preserve real ellipsis "...")
    // Replace ".." with "." only when it's not part of "..."
    while result.contains("..") && !result.contains("...") {
        result = result.replace("..", ".");
    }
    // Handle "..." followed by extra "." → "..."
    result = result.replace("....", "...");

    // Trailing connectors (from interrupted speech)
    let trimmed = result.trim_end();
    for trailing in &[" and", " so", " but", " or", " because"] {
        if trimmed.to_lowercase().ends_with(trailing) {
            let new_len = trimmed.len() - trailing.len();
            result = trimmed[..new_len].to_string();
            break;
        }
    }

    let result = result.trim().to_string();

    // Ensure the text ends with sentence-ending punctuation.
    // Whisper often omits the trailing period on the last sentence.
    if !result.is_empty() {
        let last_char = result.chars().last().unwrap();
        if !matches!(last_char, '.' | '!' | '?' | ':' | ';' | '"' | '\'' | ')' | ']') {
            return format!("{result}.");
        }
    }

    result
}

/// Collapse runs of whitespace into single spaces and trim.
fn normalize_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_space = false;

    for ch in text.trim().chars() {
        if ch.is_whitespace() {
            if !prev_was_space {
                result.push(' ');
                prev_was_space = true;
            }
        } else {
            result.push(ch);
            prev_was_space = false;
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Filler removal ──────────────────────────────────────────
    #[test]
    fn removes_basic_fillers() {
        let result = remove_fillers("I um wanted to uh test this");
        assert_eq!(result, "I wanted to test this");
    }

    #[test]
    fn removes_expanded_fillers() {
        let result = remove_fillers("I basically wanted to actually test this literally");
        assert!(!result.contains("basically"));
        assert!(!result.contains("actually"));
        assert!(!result.contains("literally"));
    }

    #[test]
    fn removes_phrase_fillers() {
        let result = remove_fillers("I you know wanted to test this");
        assert!(!result.contains("you know"));
    }

    #[test]
    fn dedup_consecutive_words() {
        let result = remove_fillers("I I wanted to to go");
        assert_eq!(result, "I wanted to go");
    }

    // ── Contextual filler removal ───────────────────────────────
    #[test]
    fn removes_so_at_sentence_start() {
        let result = remove_contextual_fillers("So I went to the store.");
        assert_eq!(result.trim(), "I went to the store.");
    }

    #[test]
    fn removes_well_at_sentence_start() {
        let result = remove_contextual_fillers("Well I think we should go.");
        assert_eq!(result.trim(), "I think we should go.");
    }

    #[test]
    fn keeps_so_in_middle() {
        let result = remove_contextual_fillers("I was so tired.");
        assert_eq!(result.trim(), "I was so tired.");
    }

    #[test]
    fn removes_filler_like() {
        let result = remove_contextual_fillers("I was, like, really tired.");
        assert!(!result.contains(", like,"));
        assert!(result.contains("really tired"));
    }

    #[test]
    fn keeps_verb_like() {
        let result = remove_contextual_fillers("I like pizza.");
        assert!(result.contains("like"));
    }

    // ── Phrase dedup ────────────────────────────────────────────
    #[test]
    fn dedup_two_word_phrase() {
        let result = dedup_phrases("I think I think we should go");
        assert_eq!(result, "I think we should go");
    }

    #[test]
    fn dedup_three_word_phrase() {
        let result = dedup_phrases("we need to we need to fix the bug");
        assert_eq!(result, "we need to fix the bug");
    }

    #[test]
    fn no_false_dedup_non_consecutive() {
        let result = dedup_phrases("I want to go and I want to stay");
        assert_eq!(result, "I want to go and I want to stay");
    }

    #[test]
    fn short_text_unchanged() {
        let result = dedup_phrases("hello world");
        assert_eq!(result, "hello world");
    }

    // ── Punctuation cleanup ─────────────────────────────────────
    #[test]
    fn fixes_space_before_period() {
        assert_eq!(cleanup_punctuation("Hello ."), "Hello.");
    }

    #[test]
    fn fixes_space_before_comma() {
        assert_eq!(cleanup_punctuation("Hello , world."), "Hello, world.");
    }

    #[test]
    fn fixes_double_period() {
        assert_eq!(cleanup_punctuation("Hello.."), "Hello.");
    }

    #[test]
    fn preserves_ellipsis() {
        assert_eq!(cleanup_punctuation("Wait..."), "Wait...");
    }

    #[test]
    fn removes_trailing_and() {
        assert_eq!(
            cleanup_punctuation("I went to the store and"),
            "I went to the store."
        );
    }

    #[test]
    fn removes_trailing_so() {
        assert_eq!(cleanup_punctuation("I was thinking so"), "I was thinking.");
    }

    #[test]
    fn adds_trailing_period() {
        assert_eq!(cleanup_punctuation("Hello world"), "Hello world.");
    }

    #[test]
    fn no_double_trailing_period() {
        assert_eq!(cleanup_punctuation("Hello world."), "Hello world.");
    }

    #[test]
    fn preserves_trailing_question_mark() {
        assert_eq!(cleanup_punctuation("Is this working?"), "Is this working?");
    }
}
