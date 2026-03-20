use crate::error::AppResult;
use crate::postprocess::types::{Correction, ProcessedText, ProcessorConfig};
use crate::storage::types::DictionaryEntry;

/// Trait for text post-processing steps.
pub trait TextProcessor: Send + Sync {
    fn process(&self, text: &str) -> AppResult<ProcessedText>;
}

/// Pipeline that applies multiple post-processing steps to transcribed text.
///
/// Steps run in order: dictionary replacement → capitalization → whitespace cleanup.
/// Each step records the corrections it makes for transparency in the UI.
pub struct ProcessorChain {
    config: ProcessorConfig,
    dictionary: Vec<DictionaryEntry>,
}

impl ProcessorChain {
    pub fn new(config: ProcessorConfig) -> Self {
        Self {
            config,
            dictionary: Vec::new(),
        }
    }

    /// Update the dictionary entries used for replacement.
    /// Call this when the user adds/removes dictionary entries.
    pub fn set_dictionary(&mut self, entries: Vec<DictionaryEntry>) {
        self.dictionary = entries;
    }
}

impl TextProcessor for ProcessorChain {
    fn process(&self, text: &str) -> AppResult<ProcessedText> {
        let original = text.to_string();
        let mut result = text.to_string();
        let mut corrections = Vec::new();

        // Step 1: Dictionary replacements (case-insensitive)
        if self.config.apply_dictionary {
            for entry in &self.dictionary {
                if !entry.is_enabled {
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
                }
            }
        }

        // Step 2: Capitalize first letter of sentences
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

        // Step 3: Clean up whitespace (collapse multiple spaces, trim)
        let before = result.clone();
        result = normalize_whitespace(&result);
        if result != before {
            corrections.push(Correction {
                original: before,
                replacement: result.clone(),
                reason: "whitespace_cleanup".to_string(),
            });
        }

        Ok(ProcessedText {
            original,
            processed: result,
            corrections,
        })
    }
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

        // Check word boundaries to avoid replacing substrings
        let at_word_start =
            abs_pos == 0 || !text.as_bytes()[abs_pos - 1].is_ascii_alphanumeric();
        let end_pos = abs_pos + phrase.len();
        let at_word_end =
            end_pos >= text.len() || !text.as_bytes()[end_pos].is_ascii_alphanumeric();

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
