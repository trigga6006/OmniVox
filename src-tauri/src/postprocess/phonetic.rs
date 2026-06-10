//! Phonetic auto-correction against the user's vocabulary.
//!
//! The vocabulary feature promises "add the word you want and OmniVox will
//! get it right".  Biasing Whisper's initial prompt helps but is
//! probabilistic — "Claude" still comes out as "cloud" or "clod" on bad
//! days.  This pass is the deterministic backstop: any transcribed word
//! that *sounds like* a vocabulary entry (same consonant skeleton, small
//! edit distance, same first letter) is replaced with the entry verbatim.
//!
//! Guard rails against false positives:
//! - Single-word entries participate only when ≥ 5 chars ("main" must NOT
//!   swallow every "mean"); parts of multi-word entries only when ≥ 4 (the
//!   neighboring words anchor the match).
//! - Phonetic-key equality alone is too coarse ("clot" and "Claude" share a
//!   skeleton), so a Levenshtein ≤ 3 bound and a first-letter match are
//!   also required.
//! - Replacement uses the entry's own casing — that's the point: proper
//!   nouns come out the way the user wrote them.

/// Minimum length for a single-word vocabulary entry to participate.
const MIN_SINGLE_WORD_LEN: usize = 5;
/// Minimum length for each word of a multi-word entry.
const MIN_MULTI_WORD_PART_LEN: usize = 4;
/// Maximum edit distance between a transcribed word and a vocab word.
const MAX_EDIT_DISTANCE: usize = 3;

/// Consonant-skeleton phonetic key.
///
/// Lowercases, drops vowels/h/w/y (except a leading vowel, kept as 'a'),
/// folds voiced/unvoiced consonant pairs into one class, and collapses
/// runs.  "cloud", "clod", "clawed", and "claude" all map to "klt".
fn phonetic_key(word: &str) -> String {
    let mut key = String::new();
    let mut first = true;
    for c in word.chars().flat_map(|c| c.to_lowercase()) {
        if !c.is_ascii_alphabetic() {
            continue;
        }
        let mapped = match c {
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'h' | 'w' => {
                if first {
                    'a' // leading vowel marker; later vowels are dropped
                } else {
                    first = false;
                    continue;
                }
            }
            'b' | 'p' => 'p',
            'd' | 't' => 't',
            'g' | 'k' | 'c' | 'q' => 'k',
            's' | 'z' => 's',
            'f' | 'v' => 'f',
            'm' | 'n' => 'n',
            other => other,
        };
        first = false;
        if key.chars().next_back() != Some(mapped) {
            key.push(mapped);
        }
    }
    key
}

/// Plain Levenshtein distance over lowercase chars.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().flat_map(|c| c.to_lowercase()).collect();
    let b: Vec<char> = b.chars().flat_map(|c| c.to_lowercase()).collect();
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut curr = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        curr[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr[j + 1] = (prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[b.len()]
}

fn first_letter_matches(a: &str, b: &str) -> bool {
    match (
        a.chars().next().and_then(|c| c.to_lowercase().next()),
        b.chars().next().and_then(|c| c.to_lowercase().next()),
    ) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Does transcribed `word` sound like vocabulary `target`?
fn sounds_like(word: &str, target: &str) -> bool {
    if word.eq_ignore_ascii_case(target) {
        return true;
    }
    first_letter_matches(word, target)
        && phonetic_key(word) == phonetic_key(target)
        && edit_distance(word, target) <= MAX_EDIT_DISTANCE
}

/// A word's position in the original text.
#[derive(Debug, Clone, Copy)]
struct WordSpan {
    start: usize,
    end: usize,
}

/// Split text into word spans (letters, digits, apostrophes).
fn word_spans(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in text.char_indices() {
        let is_word = c.is_alphanumeric() || c == '\'' || c == '’';
        match (is_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                spans.push(WordSpan { start: s, end: i });
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        spans.push(WordSpan {
            start: s,
            end: text.len(),
        });
    }
    spans
}

/// Whether only whitespace separates two adjacent spans (no punctuation —
/// "cloud. Code" across a sentence boundary must not fuse into an entry).
fn joined_by_space(text: &str, a: WordSpan, b: WordSpan) -> bool {
    text[a.end..b.start].chars().all(|c| c.is_whitespace())
}

/// Apply phonetic vocabulary correction to `text`.
///
/// For each enabled vocabulary entry (longest first so "Claude Code" wins
/// over "Claude"), scan the transcript for word sequences that sound like
/// it and splice in the entry verbatim.
pub fn apply_phonetic_vocab(text: &str, vocabulary: &[String]) -> String {
    if text.is_empty() || vocabulary.is_empty() {
        return text.to_string();
    }

    // Longest entries first — multi-word entries take precedence.
    let mut entries: Vec<&String> = vocabulary
        .iter()
        .filter(|v| !v.trim().is_empty())
        .collect();
    entries.sort_by_key(|v| std::cmp::Reverse(v.chars().count()));

    let mut result = text.to_string();

    for entry in entries {
        let parts: Vec<&str> = entry.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }
        let eligible = if parts.len() == 1 {
            parts[0].chars().count() >= MIN_SINGLE_WORD_LEN
        } else {
            parts
                .iter()
                .all(|p| p.chars().count() >= MIN_MULTI_WORD_PART_LEN)
        };
        if !eligible {
            continue;
        }

        // Re-tokenize per entry: earlier replacements changed offsets.
        loop {
            let spans = word_spans(&result);
            let mut replaced = false;

            let mut i = 0;
            while i < spans.len() {
                // Candidate window: `parts.len()` consecutive words joined
                // only by whitespace.
                if i + parts.len() <= spans.len() {
                    let window = &spans[i..i + parts.len()];
                    let contiguous = window
                        .windows(2)
                        .all(|w| joined_by_space(&result, w[0], w[1]));
                    if contiguous {
                        let all_match = window
                            .iter()
                            .zip(parts.iter())
                            .all(|(span, part)| {
                                sounds_like(&result[span.start..span.end], part)
                            });
                        let already_exact =
                            result[window[0].start..window[window.len() - 1].end] == **entry;
                        if all_match && !already_exact {
                            let start = window[0].start;
                            let end = window[window.len() - 1].end;
                            result = format!("{}{}{}", &result[..start], entry, &result[end..]);
                            replaced = true;
                            break;
                        }
                    }
                }

                // Single-word entries also match a two-word split of the
                // entry ("omni cue" → "OmniCue").  Fusion is near-exact only
                // (edit distance ≤ 1): with phonetic tolerance here, a word
                // that already matches would swallow its neighbor
                // ("Claude to" → "Claudeto" sounds like "Claude").
                if parts.len() == 1 && i + 2 <= spans.len() {
                    let (a, b) = (spans[i], spans[i + 1]);
                    let word_a = &result[a.start..a.end];
                    let word_b = &result[b.start..b.end];
                    if joined_by_space(&result, a, b)
                        && !word_a.eq_ignore_ascii_case(parts[0])
                        && !word_b.eq_ignore_ascii_case(parts[0])
                    {
                        let fused = format!("{word_a}{word_b}");
                        if first_letter_matches(&fused, parts[0])
                            && edit_distance(&fused, parts[0]) <= 1
                        {
                            result =
                                format!("{}{}{}", &result[..a.start], entry, &result[b.end..]);
                            replaced = true;
                            break;
                        }
                    }
                }

                i += 1;
            }

            if !replaced {
                break;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(words: &[&str]) -> Vec<String> {
        words.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn corrects_cloud_to_claude() {
        let v = vocab(&["Claude"]);
        assert_eq!(
            apply_phonetic_vocab("ask cloud to review this", &v),
            "ask Claude to review this"
        );
    }

    #[test]
    fn corrects_clod_and_clawed() {
        let v = vocab(&["Claude"]);
        assert_eq!(apply_phonetic_vocab("tell clod about it", &v), "tell Claude about it");
        assert_eq!(apply_phonetic_vocab("I asked clawed twice", &v), "I asked Claude twice");
    }

    #[test]
    fn preserves_adjacent_punctuation_and_capitalization() {
        let v = vocab(&["Claude"]);
        assert_eq!(
            apply_phonetic_vocab("Cloud, can you help?", &v),
            "Claude, can you help?"
        );
        assert_eq!(apply_phonetic_vocab("thanks claude!", &v), "thanks Claude!");
    }

    #[test]
    fn corrects_multi_word_entry() {
        let v = vocab(&["Claude Code"]);
        assert_eq!(
            apply_phonetic_vocab("open cloud code and run it", &v),
            "open Claude Code and run it"
        );
    }

    #[test]
    fn multi_word_does_not_fuse_across_sentence_boundary() {
        let v = vocab(&["Claude Code"]);
        let text = "it lives in the cloud. Code review is next";
        assert_eq!(apply_phonetic_vocab(text, &v), text);
    }

    #[test]
    fn fuses_split_compound_words() {
        let v = vocab(&["OmniCue"]);
        assert_eq!(
            apply_phonetic_vocab("send it to omni cue please", &v),
            "send it to OmniCue please"
        );
    }

    #[test]
    fn short_vocab_words_do_not_participate() {
        // "main" (4 chars) must never swallow "mean".
        let v = vocab(&["main"]);
        let text = "I mean it this time";
        assert_eq!(apply_phonetic_vocab(text, &v), text);
    }

    #[test]
    fn rejects_distant_words_with_same_skeleton() {
        // "clot" shares the consonant skeleton but is 4 edits away.
        let v = vocab(&["Claude"]);
        let text = "the blood clot dissolved";
        assert_eq!(apply_phonetic_vocab(text, &v), text);
    }

    #[test]
    fn rejects_different_first_letter() {
        let v = vocab(&["Claude"]);
        let text = "he played the lute";
        assert_eq!(apply_phonetic_vocab(text, &v), text);
    }

    #[test]
    fn exact_match_left_untouched() {
        let v = vocab(&["Claude"]);
        let text = "Claude is already correct";
        assert_eq!(apply_phonetic_vocab(text, &v), text);
    }

    #[test]
    fn corrects_multiple_occurrences() {
        let v = vocab(&["Claude"]);
        assert_eq!(
            apply_phonetic_vocab("cloud helped, then cloud refactored", &v),
            "Claude helped, then Claude refactored"
        );
    }

    #[test]
    fn longer_entry_wins_over_shorter() {
        let v = vocab(&["Claude", "Claude Code"]);
        assert_eq!(
            apply_phonetic_vocab("start cloud code now", &v),
            "start Claude Code now"
        );
    }

    #[test]
    fn fusion_never_swallows_neighboring_words() {
        let v = vocab(&["Claude"]);
        // "Claude to" must not fuse into "Claudeto" ≈ "Claude".
        assert_eq!(
            apply_phonetic_vocab("ask Claude to do it", &v),
            "ask Claude to do it"
        );
        // Phonetic-near fusions are rejected too — only near-exact splits fuse.
        assert_eq!(
            apply_phonetic_vocab("cloud do the thing", &v),
            "Claude do the thing"
        );
    }

    #[test]
    fn empty_inputs_are_noops() {
        assert_eq!(apply_phonetic_vocab("", &vocab(&["Claude"])), "");
        assert_eq!(apply_phonetic_vocab("hello", &[]), "hello");
    }

    #[test]
    fn phonetic_keys_collapse_as_expected() {
        assert_eq!(phonetic_key("cloud"), phonetic_key("claude"));
        assert_eq!(phonetic_key("clod"), phonetic_key("claude"));
        assert_eq!(phonetic_key("clawed"), phonetic_key("claude"));
        assert_ne!(phonetic_key("cloud"), phonetic_key("crowd"));
    }
}
