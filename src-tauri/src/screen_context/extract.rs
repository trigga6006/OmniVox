//! Extract and rank "atypical" technical tokens from screen text.
//!
//! Whisper handles ordinary English well; what it mangles are file paths,
//! identifiers, slash commands, CLI flags, and code symbols — the things
//! that aren't pronounced phonetically as written.  We surface those.
//!
//! Hand-rolled scanner (no regex dep).  Walks the text once, classifying
//! each non-whitespace run by character composition, then ranks by
//! category weight × log-frequency.

use std::collections::HashMap;

/// Token category — drives ranking weight.  Higher weight = more likely
/// to be a thing Whisper would mangle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Category {
    /// `foo.py`, `Cargo.toml` — file with recognized extension.
    PathOrFile,
    /// `--no-verify`, `-n`, `--max-tokens` — CLI flag.
    CliFlag,
    /// `/feat/branch`, `/api/users` — slash-routed path.
    SlashPath,
    /// `https://example.com` — URL.
    Url,
    /// `useEffect`, `getUserById` — camelCase identifier.
    CamelCase,
    /// `snake_case_thing` — snake_case identifier with at least one `_`.
    SnakeCase,
    /// `kebab-case-thing` — kebab-case identifier with at least one `-`.
    KebabCase,
    /// `v1.2.3`, `0.61.0` — version-shaped number.
    Version,
    /// `a1b2c3d`, `83c59b2…` — hex-ish git SHA shape.
    HexHash,
    /// Catch-all for tokens with rare punctuation we don't otherwise classify.
    Misc,
}

impl Category {
    fn weight(self) -> f32 {
        match self {
            Self::PathOrFile => 5.0,
            Self::SlashPath => 4.5,
            Self::CliFlag => 4.0,
            Self::Url => 3.5,
            Self::CamelCase => 3.0,
            Self::SnakeCase => 3.0,
            Self::KebabCase => 2.5,
            Self::HexHash => 2.0,
            Self::Version => 1.5,
            Self::Misc => 1.0,
        }
    }
}

/// Common-English words that should never bias Whisper.  Lower-cased.
/// Kept small and inline — this isn't a replacement for stemming, just a
/// guard against ranking ordinary words ahead of real technical tokens.
const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "you", "your", "this", "that", "from", "have",
    "but", "not", "are", "was", "were", "will", "would", "should", "could",
    "can", "may", "might", "must", "shall", "has", "had", "been", "being",
    "into", "onto", "than", "then", "when", "where", "what", "which", "who",
    "why", "how", "they", "them", "their", "there", "here", "all", "any",
    "some", "one", "two", "three", "four", "five", "now", "today", "yesterday",
    "tomorrow", "about", "above", "after", "before", "below", "between",
    "during", "over", "under", "again", "very", "more", "most", "much",
    "other", "another", "each", "every", "such", "same", "just", "still",
    "only", "even", "also", "back", "down", "out", "off", "yes", "good",
    "great", "really", "okay", "thanks", "please", "sorry", "hello", "hi",
];

fn is_stopword(s: &str) -> bool {
    let lower = s.to_lowercase();
    STOPWORDS.contains(&lower.as_str())
}

/// File extensions we consider "interesting" enough to bias Whisper toward.
const INTERESTING_EXTS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "rb", "java", "kt", "swift",
    "c", "cpp", "cc", "h", "hpp", "cs", "php", "lua", "sh", "bash", "zsh",
    "ps1", "bat", "md", "mdx", "txt", "json", "toml", "yaml", "yml", "xml",
    "html", "css", "scss", "sass", "less", "sql", "graphql", "gql",
    "lock", "cfg", "ini", "env", "gitignore", "dockerfile", "makefile",
];

fn token_chars_ok(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| {
        c.is_ascii_alphanumeric()
            || matches!(
                c,
                '.' | '_' | '-' | '/' | '\\' | ':' | '=' | '@' | '+' | '#' | '~'
            )
    })
}

/// Classify a single token into a category, or return None if it's not
/// interesting (plain word, numbers, garbage punctuation).
fn classify(token: &str) -> Option<Category> {
    if token.len() < 3 || token.len() > 80 {
        return None;
    }
    if !token_chars_ok(token) {
        return None;
    }

    let bytes = token.as_bytes();
    let starts_with_letter = bytes[0].is_ascii_alphabetic() || bytes[0] == b'_';

    // URL — the cheapest discriminator.
    if token.starts_with("http://") || token.starts_with("https://") {
        return Some(Category::Url);
    }

    // CLI flag: starts with `-` or `--` followed by a letter.
    if let Some(rest) = token.strip_prefix("--").or_else(|| token.strip_prefix('-')) {
        if rest.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
            && !rest.is_empty()
            && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Some(Category::CliFlag);
        }
    }

    // Slash path: starts with `/` and has at least one more path segment.
    if token.starts_with('/') && token[1..].contains(|c: char| c.is_ascii_alphanumeric()) {
        // Don't mistake "//", "/" alone, or pure punctuation tails.
        if token.len() >= 3 {
            return Some(Category::SlashPath);
        }
    }

    // File / path with recognized extension.
    if let Some(ext_start) = token.rfind('.') {
        let ext = &token[ext_start + 1..].to_ascii_lowercase();
        if INTERESTING_EXTS.contains(&ext.as_str()) {
            // Also require a non-trivial stem so we don't flag "v0.1.0".
            if ext_start > 0 {
                return Some(Category::PathOrFile);
            }
        }
    }

    // Path with separators.
    if (token.contains('/') || token.contains('\\')) && starts_with_letter {
        return Some(Category::PathOrFile);
    }

    // Version-shaped: digits with at least one dot, e.g. "1.2.3" or "v0.61.0".
    if version_shaped(token) {
        return Some(Category::Version);
    }

    // Hex-ish hash: 7-40 hex chars.
    if (7..=40).contains(&token.len())
        && token.chars().all(|c| c.is_ascii_hexdigit())
        && token.chars().any(|c| c.is_ascii_alphabetic())
    {
        return Some(Category::HexHash);
    }

    // Identifiers — bail on stopwords first so "the" and friends never qualify.
    if is_stopword(token) {
        return None;
    }

    if is_camel_case(token) {
        return Some(Category::CamelCase);
    }
    if token.contains('_') && starts_with_letter && !token.contains(' ') {
        return Some(Category::SnakeCase);
    }
    if token.contains('-')
        && starts_with_letter
        && !token.starts_with('-')
        && token.matches('-').count() >= 1
    {
        return Some(Category::KebabCase);
    }

    // Tokens with @ # : = + ~ that aren't otherwise classified — useful as
    // verbatim hints (e.g. "@anthropic-ai/sdk", "user@host", "key=value").
    if token.chars().any(|c| matches!(c, '@' | '#' | ':' | '=' | '+' | '~')) {
        return Some(Category::Misc);
    }

    None
}

fn version_shaped(s: &str) -> bool {
    let s = s.strip_prefix('v').unwrap_or(s);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 || parts.len() > 4 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

fn is_camel_case(s: &str) -> bool {
    let mut has_lower = false;
    let mut has_upper_after_lower = false;
    let mut prev_lower = false;
    for c in s.chars() {
        if c.is_ascii_lowercase() {
            has_lower = true;
            prev_lower = true;
        } else if c.is_ascii_uppercase() {
            if prev_lower {
                has_upper_after_lower = true;
            }
            prev_lower = false;
        } else if c.is_ascii_digit() {
            prev_lower = false;
        } else {
            return false;
        }
    }
    has_lower && has_upper_after_lower
}

/// Walk `text` and yield candidate tokens — runs of non-whitespace,
/// trimmed of leading/trailing punctuation that can't appear inside
/// identifiers (parens, brackets, commas, periods at end).
fn tokenize(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| c.is_whitespace()) {
        if raw.is_empty() {
            continue;
        }
        let trimmed = trim_edges(raw);
        if !trimmed.is_empty() {
            out.push(trimmed);
        }
    }
    out
}

/// Strip leading/trailing punctuation that's never part of a token.
fn trim_edges(s: &str) -> &str {
    let s = s.trim_start_matches(['(', '[', '{', '"', '\'', '`', ',', ';']);
    let mut s = s;
    // Trailing dots in normal English shouldn't kill `foo.py` — only strip
    // if the dot is preceded by a non-identifier char (rare) or if it's a
    // sentence terminator (followed by nothing meaningful).
    while let Some(last) = s.chars().last() {
        if matches!(
            last,
            ')' | ']' | '}' | '"' | '\'' | '`' | ',' | ';' | '!' | '?'
        ) {
            s = &s[..s.len() - last.len_utf8()];
        } else {
            break;
        }
    }
    // Conditionally strip a trailing dot — only when removing it leaves
    // something that doesn't itself look like a path/file (so `foo.py.` →
    // `foo.py` but `foo.py` is left alone).
    if s.ends_with('.') {
        let without = &s[..s.len() - 1];
        if !without.contains('.') || without.ends_with('.') {
            // No interior dot to anchor on — leave the trailing dot alone.
        } else {
            s = without;
        }
    }
    s
}

/// Predicate for `build_initial_prompt`: is this token *safe* to feed into
/// Whisper as a vocabulary bias?
///
/// Whisper biases toward the *style* of its `initial_prompt`, not just the
/// exact tokens.  A prompt full of numbers, hashes, or punctuation makes the
/// decoder produce numbers / hashes / punctuation in place of dictated
/// words — even unrelated ones.  So Whisper bias must be alphabetic-dominant.
///
/// Phase 2 (Qwen) is unaffected: it sees the full ranked list since the
/// tokens are explicit user-turn context, not generative bias.
pub(crate) fn is_useful_for_whisper(token: &str) -> bool {
    let total = token.chars().count();
    if total == 0 {
        return false;
    }
    let alpha = token.chars().filter(|c| c.is_ascii_alphabetic()).count();
    // Need at least 3 letters and 40 %+ alphabetic content — a `clipslop.py`
    // (8 letters out of 11 chars = 72 %) passes; a `13:42:00` (0 % alpha) or
    // `1.2.3.4` (0 %) does not.
    if alpha < 3 {
        return false;
    }
    if alpha * 100 < total * 40 {
        return false;
    }
    // Pure-numeric version shapes ("1.2.3", "v0.61.0", "2024.01.15")
    // already fail the alpha test, but a loose secondary guard catches
    // versions with a "v" prefix that scrape past the alpha minimum on
    // borderline cases.
    if version_shaped_loose(token) {
        return false;
    }
    // Pure hex SHAs / hashes (any length).  We also drop dashed UUIDs.
    if token
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '-')
        && token.chars().any(|c| c.is_ascii_digit())
    {
        return false;
    }
    true
}

fn version_shaped_loose(s: &str) -> bool {
    let s = s.strip_prefix('v').unwrap_or(s);
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    parts
        .iter()
        .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// Top-level: extract and rank up to `max` tokens from `text`.
///
/// Ranking: category_weight × log(1 + frequency).  Frequency rewards tokens
/// the user is clearly looking at (file repeated in tree + tab + breadcrumb).
pub fn rank_tokens(text: &str, max: usize) -> Vec<String> {
    let mut counts: HashMap<String, (Category, u32)> = HashMap::new();

    for tok in tokenize(text) {
        if let Some(cat) = classify(tok) {
            let key = tok.to_string();
            counts
                .entry(key)
                .and_modify(|(_, c)| *c += 1)
                .or_insert((cat, 1));
        }
    }

    // Dedupe case-insensitively, keep the most-frequent original casing.
    let mut by_lower: HashMap<String, (String, Category, u32)> = HashMap::new();
    for (orig, (cat, count)) in counts {
        let lower = orig.to_lowercase();
        by_lower
            .entry(lower)
            .and_modify(|entry| {
                entry.2 += count;
                if count > entry.2 / 2 {
                    entry.0 = orig.clone();
                }
            })
            .or_insert((orig, cat, count));
    }

    let mut ranked: Vec<(String, f32)> = by_lower
        .into_iter()
        .map(|(_, (orig, cat, count))| {
            let score = cat.weight() * (1.0 + (count as f32).ln());
            (orig, score)
        })
        .collect();

    // Sort by score desc, then alphabetical for determinism.
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    ranked.truncate(max);
    ranked.into_iter().map(|(s, _)| s).collect()
}
