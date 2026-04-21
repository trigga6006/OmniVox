//! "Voxify" trigger-word detection for Structured Mode.
//!
//! When the user has the voice-command gate on (see
//! `AppSettings::structured_voice_command`), they opt into structuring per
//! utterance by saying "Voxify" at the end of their dictation.  This
//! module recognises that trigger and strips it from the text so the
//! word itself never ends up in the pasted output.
//!
//! Whisper mishearings:
//! "Voxify" is not in any ASR vocabulary, so Whisper frequently substitutes
//! the leading /v/ with a phonetically adjacent consonant (F, B, P, W).
//! Rather than training the user to enunciate, we accept a set of phonetic
//! aliases (see [`TRIGGER_ALIASES`]).  None of them are real English words,
//! so including them as triggers doesn't risk false activation on normal
//! dictation content.
//!
//! Detection rules:
//!   - Case-insensitive ("Voxify", "voxify", "VOXIFY" all match).
//!   - Must be the last word of the transcription, allowing trailing
//!     punctuation (period / comma / exclamation / question mark) and
//!     whitespace.
//!   - If the text is ONLY the trigger word (nothing before it), we
//!     treat that as not-a-trigger and return the original text
//!     unchanged — there's nothing to structure, so swallowing the word
//!     would silently produce an empty output.

/// Accepted phonetic aliases for "Voxify".  All lowercase; the match is
/// case-insensitive.  Ordered roughly by how commonly Whisper lands on
/// each substitution — the original on top, common confusables next.
///
/// Keep every entry non-lexical (i.e. not a real English word) so that
/// a user who happens to dictate the word in a different context
/// doesn't accidentally activate the trigger.  "Foxify" is technically a
/// jargon-ish verb in fandom spaces but is rare enough in coding dictation
/// to be safe.
const TRIGGER_ALIASES: &[&str] = &[
    "voxify",  // canonical
    "foxify",  // /f/ for /v/ (most common Whisper miss)
    "boxify",  // /b/ for /v/
    "poxify",  // /p/ for /v/
    "woxify",  // /w/ for /v/
    "vexify",  // different vowel (e)
    "vaxify",  // different vowel (a)
    "oxify",   // initial consonant elided
    // Whisper also drops the /i/ between /f/ and /aɪ/ when the user says
    // "Voxify" quickly (the vowel collapses to a schwa).  Each canonical
    // form has a no-`i` twin; all are still non-lexical.
    "voxfy",
    "foxfy",
    "boxfy",
    "poxfy",
    "woxfy",
    "vexfy",
    "vaxfy",
    "oxfy",
];

fn matches_any_trigger(word: &str) -> bool {
    TRIGGER_ALIASES
        .iter()
        .any(|alias| word.eq_ignore_ascii_case(alias))
}

/// Detect whether `text` ends with the "Voxify" trigger word and return
/// the text with the trigger stripped plus a flag indicating detection.
///
/// Returns `(stripped_text, voxify_said)`:
/// - when the trigger is absent: the original text and `false`
/// - when only the trigger is present: the original text and `false`
/// - when the trigger is present at the end of real content: the
///   content with the trigger (and any trailing punctuation) removed,
///   and `true`
pub fn detect_and_strip_trigger(text: &str) -> (String, bool) {
    // 1. Pull off trailing whitespace + sentence-end punctuation.
    let trimmed = text.trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '.' | '!' | '?' | ',' | ';' | ':')
    });

    if trimmed.is_empty() {
        return (text.to_string(), false);
    }

    // 2. Find the start of the last word (first whitespace char scanning
    //    from the right, exclusive).
    let last_word_start = trimmed
        .char_indices()
        .rev()
        .find(|(_, c)| c.is_whitespace())
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0);

    let last_word = &trimmed[last_word_start..];
    if !matches_any_trigger(last_word) {
        return (text.to_string(), false);
    }

    // 3. Everything before the last word, with any lingering whitespace
    //    or sentence-end punctuation (".", ",", etc.) trimmed off.
    let before = trimmed[..last_word_start].trim_end_matches(|c: char| {
        c.is_whitespace() || matches!(c, '.' | '!' | '?' | ',' | ';' | ':')
    });

    if before.is_empty() {
        // User said only "Voxify" with nothing to structure — not a
        // real trigger, leave the text alone.
        return (text.to_string(), false);
    }

    (before.to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::detect_and_strip_trigger;

    #[test]
    fn strips_trigger_at_end_with_period() {
        let (stripped, said) =
            detect_and_strip_trigger("Refactor the checkout flow. Voxify.");
        assert_eq!(stripped, "Refactor the checkout flow");
        assert!(said);
    }

    #[test]
    fn strips_trigger_case_insensitive() {
        let (stripped, said) =
            detect_and_strip_trigger("Summarize the changes. VOXIFY");
        assert_eq!(stripped, "Summarize the changes");
        assert!(said);

        let (stripped, said) =
            detect_and_strip_trigger("Summarize the changes. voxify");
        assert_eq!(stripped, "Summarize the changes");
        assert!(said);
    }

    #[test]
    fn leaves_text_alone_when_trigger_absent() {
        let (stripped, said) =
            detect_and_strip_trigger("Format this prompt.");
        assert_eq!(stripped, "Format this prompt.");
        assert!(!said);
    }

    #[test]
    fn does_not_strip_trigger_mid_sentence() {
        // "Voxify" in the middle is not a trigger — only end counts.
        let (stripped, said) =
            detect_and_strip_trigger("Can you voxify this for me?");
        assert_eq!(stripped, "Can you voxify this for me?");
        assert!(!said);
    }

    #[test]
    fn strips_phonetic_aliases_whisper_confuses_for_voxify() {
        // Every alias in TRIGGER_ALIASES should activate the trigger —
        // Whisper occasionally hears any of these instead of "Voxify"
        // because the word isn't in its training vocabulary.
        for alias in [
            "Foxify", "Boxify", "Poxify", "Woxify", "Vaxify", "Oxify", "VEXIFY",
            // No-`i` variants (schwa collapse between /f/ and /aɪ/).
            "Voxfy", "Foxfy", "Boxfy", "Poxfy", "Woxfy", "Vexfy", "Vaxfy", "Oxfy",
        ] {
            let input = format!("Refactor the auth flow. {alias}.");
            let (stripped, said) = detect_and_strip_trigger(&input);
            assert!(said, "alias {alias:?} should trigger");
            assert_eq!(stripped, "Refactor the auth flow", "alias {alias:?}");
        }
    }

    #[test]
    fn handles_only_trigger_word() {
        // Just "Voxify" alone — nothing to structure, so we don't count it.
        let (stripped, said) = detect_and_strip_trigger("Voxify");
        assert_eq!(stripped, "Voxify");
        assert!(!said);

        let (stripped, said) = detect_and_strip_trigger("Voxify.");
        assert_eq!(stripped, "Voxify.");
        assert!(!said);
    }

    #[test]
    fn handles_trailing_whitespace_and_punctuation() {
        let (stripped, said) =
            detect_and_strip_trigger("Refactor the auth flow.   Voxify!  ");
        assert_eq!(stripped, "Refactor the auth flow");
        assert!(said);
    }

    #[test]
    fn handles_empty_input() {
        let (stripped, said) = detect_and_strip_trigger("");
        assert_eq!(stripped, "");
        assert!(!said);
    }

    #[test]
    fn handles_whitespace_only_input() {
        let (stripped, said) = detect_and_strip_trigger("   \n  ");
        assert_eq!(stripped, "   \n  ");
        assert!(!said);
    }
}
