use serde::{Deserialize, Serialize};

/// Slot fields the LLM is allowed to emit, in GBNF order.
///
/// Every field except `goal` is optional and must be omitted (not emitted as
/// null or empty) when the user didn't mention it.  The grammar in
/// `resources/grammars/slot_extraction_v1.gbnf` encodes this.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlotExtraction {
    /// The primary thing the user wants done.  Required.
    pub goal: String,

    /// Background, current behavior, or important context that should not be lost.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<String>,

    /// Hard boundaries the user stated ("keep the Stripe integration intact",
    /// "don't break the auth flow").  Protective / negative-framed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,

    /// Files, components, or code units mentioned by the user.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,

    /// Urgency enum — GBNF restricts to low/normal/high, so hallucinated values
    /// cannot reach us.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub urgency: Option<Urgency>,

    /// User-flow / acceptance criteria — positive-framed statements describing
    /// the end-user experience the change should produce.  Phrasings like
    /// "I should be able to X", "When Y happens, Z should…", "The panel
    /// should always show Q".  This replaces the old `follow_up_tasks` slot
    /// because agentic coding prompts benefit more from outcome statements
    /// than from a second list of TODOs adjacent to the goal.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_behavior: Vec<String>,

    /// Open questions the user wants explored / investigated / answered.
    /// Populated for exploration, research, and advice intents — prompts
    /// where the user isn't asking for an immediate build but wants the
    /// agent to think through something with them.  Typical phrasings:
    /// "how would X scale", "what are the trade-offs of Y", "could we
    /// instead do Z".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<String>,

    /// Alternatives the user is weighing or wants compared.  Populated for
    /// advice / decision / planning intents.  Typical phrasings:
    /// "option A: X", "we could either A or B", "leaning towards X but
    /// also considering Y".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Urgency {
    Low,
    Normal,
    High,
}

impl Urgency {
    pub fn as_str(self) -> &'static str {
        match self {
            Urgency::Low => "low",
            Urgency::Normal => "normal",
            Urgency::High => "high",
        }
    }
}

impl SlotExtraction {
    /// Light clean-up on what the LLM produced.
    ///
    /// Deliberately conservative — an earlier overzealous pass deleted
    /// legitimate user content (anything starting with "I want to…", any
    /// follow-up that looked vaguely like the goal, anything moving between
    /// context and follow-up based on starting verbs).  That over-processing
    /// was the main source of "the structured output lost the meaning of
    /// what I said", so we don't do it anymore.
    ///
    /// What still happens:
    ///   - whitespace trim on every string
    ///   - drop empty or punctuation-only array entries
    ///   - dedupe within a list (exact match after a tolerant normalize)
    ///   - cross-list dedupe between `constraints` and `expected_behavior`:
    ///     when the model emits the same thought in both slots, keep it in
    ///     `expected_behavior` (the richer framing) and drop it from
    ///     `constraints`.  Also strip any list entry that matches the goal.
    ///   - reject `files` entries that are clearly not file / code references
    pub fn normalize(mut self) -> Self {
        self.goal = strip_third_person_self_ref(self.goal.trim());
        self.context = normalize_items(self.context);
        self.constraints = normalize_items(self.constraints);
        self.files = normalize_files(self.files);
        self.expected_behavior = normalize_items(self.expected_behavior);
        self.questions = normalize_items(self.questions);
        self.options = normalize_items(self.options);

        // Cross-list dedupe — prefer the richer slot.  `expected_behavior`
        // wins over `constraints`; both win over `context`.
        let goal_key = compact_key(&self.goal);
        let behavior_keys = self
            .expected_behavior
            .iter()
            .map(|s| compact_key(s))
            .collect::<Vec<_>>();

        self.constraints.retain(|item| {
            let k = compact_key(item);
            k != goal_key && !behavior_keys.contains(&k)
        });

        let constraint_keys = self
            .constraints
            .iter()
            .map(|s| compact_key(s))
            .collect::<Vec<_>>();
        self.context.retain(|item| {
            let k = compact_key(item);
            k != goal_key
                && !behavior_keys.contains(&k)
                && !constraint_keys.contains(&k)
        });

        self.expected_behavior
            .retain(|item| compact_key(item) != goal_key);
        self.questions.retain(|item| compact_key(item) != goal_key);
        self.options.retain(|item| compact_key(item) != goal_key);

        self
    }

    /// Same as `normalize`, plus a grounded-against-raw-input pass that
    /// drops slot items the model fabricated without any support from the
    /// dictation.
    ///
    /// Two fabrication classes this catches:
    ///   1. `files` entries the user never mentioned.  The `files` slot is
    ///      the highest-risk fabrication because a downstream coding agent
    ///      may actually try to edit/create the fictional path.  Every
    ///      entry must appear as a substring (case-insensitive) of the raw
    ///      input.
    ///   2. For SHORT inputs (<= 120 chars), non-goal slots are collapsed
    ///      to empty unless the slot item shares a meaningful word with
    ///      the raw text.  Short inputs are where the model's "be helpful"
    ///      bias causes it to pad with invented context/constraints/
    ///      behavior just to match the rich-example pattern.  Shorter
    ///      truthful output is always better than longer fabricated
    ///      output; this enforces that.
    pub fn normalize_with_raw(self, raw_input: &str) -> Self {
        let mut s = self.normalize();
        let raw_lower = raw_input.to_lowercase();

        // (1) Ungrounded files → drop.  We match on the lowercased token,
        // not the full entry, so "src/features/overlay/FloatingPill.tsx"
        // in the output still grounds if the raw said "FloatingPill".
        s.files.retain(|entry| {
            file_is_grounded_in_raw(entry, &raw_lower)
        });

        // (2) Short-input fabrication guard.  Drop non-goal list items
        // that share no content-word with the raw input.
        const SHORT_INPUT_THRESHOLD: usize = 120;
        if raw_input.chars().count() <= SHORT_INPUT_THRESHOLD {
            let raw_words = content_words(&raw_lower);
            s.context.retain(|item| shares_content_word(item, &raw_words));
            s.constraints.retain(|item| shares_content_word(item, &raw_words));
            s.expected_behavior
                .retain(|item| shares_content_word(item, &raw_words));
            s.questions.retain(|item| shares_content_word(item, &raw_words));
            s.options.retain(|item| shares_content_word(item, &raw_words));
        }

        s
    }
}

fn normalize_items(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen_keys: Vec<String> = Vec::new();
    for item in items {
        let trimmed = strip_third_person_self_ref(item.trim());
        if trimmed.is_empty() {
            continue;
        }
        // Drop entries that are literal punctuation/whitespace with no actual
        // content.  This catches grammar-level mishaps where the model emits
        // something like "," or "],".
        if !trimmed.chars().any(|c| c.is_ascii_alphanumeric()) {
            continue;
        }
        // Within-list dedupe uses the tolerant key so "Keep the Paste button"
        // and "keep the paste button" don't both survive.
        let key = compact_key(&trimmed);
        if seen_keys.iter().any(|existing| existing == &key) {
            continue;
        }
        seen_keys.push(key);
        out.push(trimmed);
    }
    out
}

/// Rewrite common "the user / you" third-person self-references produced by
/// the LLM back into first person.  The model occasionally narrates *about*
/// the speaker ("the user should be able to…") instead of *as* the speaker
/// ("I should be able to…") — that's model commentary leaking through and
/// doesn't belong in a prompt the user is about to paste into a coding
/// agent.
///
/// This pass is deliberately conservative.  It only rewrites patterns where
/// "the user" / "you" is clearly standing in for first-person self-reference
/// — verb phrases like "the user wants", "the user should", possessives
/// like "the user's".  Legitimate uses such as "user interface", "user
/// experience", plural "users", or quoted references are preserved.
fn strip_third_person_self_ref(text: &str) -> String {
    // Ordered most-specific first so longer phrases win over substrings.
    // Each entry handles both sentence-initial capitalised form and
    // mid-sentence lowercase form.
    const PAIRS: &[(&str, &str)] = &[
        // Possessives
        ("The user's ", "My "),
        ("the user's ", "my "),
        ("User's ", "My "),
        ("user's ", "my "),
        // Verb phrases — capital
        ("The user wants to ", "I want to "),
        ("The user needs to ", "I need to "),
        ("The user is trying to ", "I am trying to "),
        ("The user would like to ", "I would like to "),
        ("The user should be able to ", "I should be able to "),
        ("The user should ", "I should "),
        ("The user wants ", "I want "),
        ("The user needs ", "I need "),
        ("The user can ", "I can "),
        ("The user could ", "I could "),
        ("The user will ", "I will "),
        ("The user would ", "I would "),
        ("The user must ", "I must "),
        ("The user is ", "I am "),
        ("The user has ", "I have "),
        ("The user expects ", "I expect "),
        ("The user asks ", "I ask "),
        // Verb phrases — lowercase
        ("the user wants to ", "I want to "),
        ("the user needs to ", "I need to "),
        ("the user is trying to ", "I am trying to "),
        ("the user would like to ", "I would like to "),
        ("the user should be able to ", "I should be able to "),
        ("the user should ", "I should "),
        ("the user wants ", "I want "),
        ("the user needs ", "I need "),
        ("the user can ", "I can "),
        ("the user could ", "I could "),
        ("the user will ", "I will "),
        ("the user would ", "I would "),
        ("the user must ", "I must "),
        ("the user is ", "I am "),
        ("the user has ", "I have "),
        ("the user expects ", "I expect "),
        ("the user asks ", "I ask "),
        // Second-person slips — these are also model commentary in this
        // context, since the speaker is narrating to themselves.
        ("You should be able to ", "I should be able to "),
        ("you should be able to ", "I should be able to "),
        ("You should ", "I should "),
        ("you should ", "I should "),
        ("You can ", "I can "),
        ("you can ", "I can "),
        ("You will ", "I will "),
        ("you will ", "I will "),
        ("You must ", "I must "),
        ("you must ", "I must "),
    ];
    let mut out = text.to_string();
    for (from, to) in PAIRS {
        if out.contains(from) {
            out = out.replace(from, to);
        }
    }
    // Fix the artefact where a replaced "I ..." lands mid-sentence after a
    // period and the capitalisation is wrong the other way.  Rare; leave
    // alone — callers show the text verbatim and a mid-sentence "I" reads
    // fine.  (Sentinel comment so future me doesn't re-add a capitalisation
    // fix that ends up double-upper-casing.)
    out
}

fn normalize_files(items: Vec<String>) -> Vec<String> {
    normalize_items(items)
        .into_iter()
        .filter(|item| looks_like_code_reference(item))
        .collect()
}

/// Comparison key for cross-slot dedupe.  Lowercases, drops non-alphanumeric
/// characters, so surface-level whitespace / punctuation / case differences
/// don't hide a real duplicate.
fn compact_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// True if `value` plausibly names a file, path, component, module, or
/// function — i.e. something a coding assistant would want in the `## Files`
/// section of the prompt.  Rejects schema fragments, bare topics, and
/// punctuation-only strings that sometimes slip through the grammar.
fn looks_like_code_reference(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    if value.contains(['[', ']', '{', '}', '"', '`']) {
        return false;
    }
    if value.contains(':') {
        let bytes = value.as_bytes();
        let is_windows_path =
            value.len() >= 3 && bytes[1] == b':' && matches!(bytes[2], b'\\' | b'/');
        if !is_windows_path {
            return false;
        }
    }
    if !value.chars().any(|c| c.is_ascii_alphanumeric()) {
        return false;
    }
    // Accept if it looks pathy (has a slash, dot, underscore, hyphen) OR if
    // it's a camel/PascalCase identifier (uppercase letter present).
    value.contains(['/', '\\', '.', '_', '-']) || value.chars().any(|c| c.is_ascii_uppercase())
}

/// Is `entry` supported by the raw dictation?
///
/// Strategy: split the entry into path-like segments, then require that at
/// least one segment appears as a substring of the lowercased raw input.
/// This is lenient enough to accept "src/features/overlay/FloatingPill.tsx"
/// when the user only said "FloatingPill", and strict enough to reject
/// entirely fabricated paths like "billing.tsx" when the user never
/// mentioned billing.
fn file_is_grounded_in_raw(entry: &str, raw_lower: &str) -> bool {
    if raw_lower.is_empty() {
        return false;
    }
    for segment in entry.split(['/', '\\']) {
        let stem = segment.split('.').next().unwrap_or(segment);
        if stem.len() < 3 {
            continue;
        }
        let lower = stem.to_lowercase();
        if raw_lower.contains(&lower) {
            return true;
        }
        // Split PascalCase / camelCase into words and check each: the user
        // might have said "floating pill" even though the model output
        // "FloatingPill".
        for word in split_camel_case(stem) {
            if word.len() >= 4 && raw_lower.contains(&word.to_lowercase()) {
                return true;
            }
        }
    }
    false
}

/// Break a CamelCase or camelCase identifier into whitespace-separated
/// lowercase words.  "FloatingPill" → ["Floating", "Pill"].  Non-letter
/// characters end the current word.
fn split_camel_case(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_upper = false;
    for c in s.chars() {
        if !c.is_ascii_alphanumeric() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            prev_upper = false;
            continue;
        }
        let is_upper = c.is_ascii_uppercase();
        if is_upper && !prev_upper && !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        current.push(c);
        prev_upper = is_upper;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Minimal stopword set.  Deliberately small so the content-word check is
/// tolerant to rephrasing but still rejects items with no substantive
/// overlap.  Adding too many words here makes the check more aggressive
/// (more false-positives); keep this lean.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "but", "can", "could",
    "did", "do", "does", "for", "from", "had", "has", "have", "he", "her",
    "his", "i", "if", "in", "into", "is", "it", "its", "just", "like", "me",
    "my", "not", "now", "of", "on", "or", "our", "out", "over", "should",
    "so", "some", "such", "than", "that", "the", "their", "them", "then",
    "there", "these", "they", "this", "to", "too", "up", "us", "was", "we",
    "were", "what", "when", "which", "while", "who", "why", "will", "with",
    "would", "you", "your",
];

/// Extract content words from a string (lowercase, non-stopword, length ≥ 3).
/// Returns a sorted-unique Vec for cheap `contains` checks on small sets.
fn content_words(text_lower: &str) -> Vec<String> {
    let mut out: Vec<String> = text_lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() >= 3)
        .filter(|w| !STOPWORDS.contains(w))
        .map(|w| w.to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// True if `item` shares at least one content word with `raw_words`.  Used
/// as the short-input fabrication guard: a slot entry with zero word
/// overlap to the raw dictation is almost certainly model-invented.
fn shares_content_word(item: &str, raw_words: &[String]) -> bool {
    if raw_words.is_empty() {
        // If we can't extract any content words from the raw input, don't
        // second-guess the model — let whatever it produced pass.
        return true;
    }
    let item_lower = item.to_lowercase();
    let item_words = content_words(&item_lower);
    if item_words.is_empty() {
        // No content words in the entry either — treat as trivially grounded.
        return true;
    }
    item_words.iter().any(|w| raw_words.contains(w))
}
