use super::extract::{is_useful_for_whisper, rank_tokens};
use super::{build_initial_prompt, ScreenContext, MAX_SCREEN_TOKENS};

#[test]
fn extract_picks_file_with_extension() {
    let text = "Open clipslop.py and edit the function";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "clipslop.py"));
}

#[test]
fn extract_picks_dotted_extensions_for_many_langs() {
    let text = "main.rs Cargo.toml index.tsx settings.json schema.sql README.md";
    let tokens = rank_tokens(text, 20);
    for expected in ["main.rs", "Cargo.toml", "index.tsx", "settings.json", "schema.sql", "README.md"] {
        assert!(
            tokens.iter().any(|t| t == expected),
            "missing {expected} in {tokens:?}"
        );
    }
}

#[test]
fn extract_picks_camel_case_identifiers() {
    let text = "call useEffect inside getUserById then update";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "useEffect"));
    assert!(tokens.iter().any(|t| t == "getUserById"));
}

#[test]
fn extract_picks_snake_case_identifiers() {
    let text = "the get_user_by_id helper calls list_active_sessions";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "get_user_by_id"));
    assert!(tokens.iter().any(|t| t == "list_active_sessions"));
}

#[test]
fn extract_picks_kebab_case() {
    let text = "the feat-screen-context branch contains screen-context-extract";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "feat-screen-context"));
}

#[test]
fn extract_picks_cli_flags() {
    let text = "git commit --no-verify --amend -m message";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "--no-verify"));
    assert!(tokens.iter().any(|t| t == "--amend"));
}

#[test]
fn extract_picks_slash_paths() {
    let text = "PR review at /pulls/1234 and route /api/users/profile";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "/pulls/1234"));
    assert!(tokens.iter().any(|t| t == "/api/users/profile"));
}

#[test]
fn extract_drops_common_english() {
    let text = "the and for with you your this that from have but not";
    let tokens = rank_tokens(text, 20);
    assert!(tokens.is_empty(), "expected zero tokens, got {tokens:?}");
}

#[test]
fn extract_dedupes_case_insensitively() {
    let text = "FloatingPill.tsx FloatingPill.tsx floatingpill.tsx";
    let tokens = rank_tokens(text, 10);
    let count = tokens
        .iter()
        .filter(|t| t.eq_ignore_ascii_case("FloatingPill.tsx"))
        .count();
    assert_eq!(count, 1, "expected one dedup'd token, got {tokens:?}");
}

#[test]
fn extract_respects_max_count() {
    let text =
        "alpha.rs beta.rs gamma.rs delta.rs epsilon.rs zeta.rs eta.rs theta.rs iota.rs kappa.rs";
    let tokens = rank_tokens(text, 3);
    assert_eq!(tokens.len(), 3);
}

#[test]
fn extract_strips_trailing_punctuation() {
    let text = "Edit clipslop.py, then run main.rs.";
    let tokens = rank_tokens(text, 10);
    assert!(tokens.iter().any(|t| t == "clipslop.py"));
    assert!(tokens.iter().any(|t| t == "main.rs"));
}

#[test]
fn extract_handles_empty_and_whitespace() {
    assert!(rank_tokens("", 10).is_empty());
    assert!(rank_tokens("   \n\t  ", 10).is_empty());
}

#[test]
fn extract_picks_versions_lower_priority_than_files() {
    let text = "version 1.2.3 of clipslop.py";
    let tokens = rank_tokens(text, 10);
    let py_pos = tokens.iter().position(|t| t == "clipslop.py").unwrap();
    let ver_pos = tokens.iter().position(|t| t == "1.2.3");
    if let Some(v) = ver_pos {
        assert!(py_pos < v, "file should rank higher than version");
    }
}

#[test]
fn extract_camelcase_does_not_match_capitalized_word() {
    // "Hello" is just a capitalized word, not camelCase.
    let text = "Hello world Cargo Toml";
    let tokens = rank_tokens(text, 10);
    assert!(
        !tokens.iter().any(|t| t == "Hello"),
        "expected Hello not flagged, got {tokens:?}"
    );
}

#[test]
fn build_initial_prompt_returns_none_when_empty() {
    let ctx = ScreenContext::default();
    assert!(build_initial_prompt(&ctx, None).is_none());
    assert!(build_initial_prompt(&ctx, Some("")).is_none());
}

#[test]
fn build_initial_prompt_includes_base_and_screen_tokens() {
    let ctx = ScreenContext {
        tokens: vec!["clipslop.py".into(), "useEffect".into()],
        ..Default::default()
    };
    let prompt = build_initial_prompt(&ctx, Some("OmniVox React")).unwrap();
    assert!(prompt.contains("OmniVox"));
    assert!(prompt.contains("React"));
    assert!(prompt.contains("clipslop.py"));
    assert!(prompt.contains("useEffect"));
}

#[test]
fn build_initial_prompt_dedupes_overlap() {
    let ctx = ScreenContext {
        tokens: vec!["OmniVox".into(), "FooBar".into()],
        ..Default::default()
    };
    let prompt = build_initial_prompt(&ctx, Some("OmniVox React")).unwrap();
    assert_eq!(prompt.matches("OmniVox").count(), 1);
}

#[test]
fn build_initial_prompt_caps_total_terms() {
    // Generate more tokens than the cap to verify truncation.
    let many: Vec<String> = (0..400).map(|i| format!("token{i}")).collect();
    let ctx = ScreenContext {
        tokens: many,
        ..Default::default()
    };
    let prompt = build_initial_prompt(&ctx, None).unwrap();
    let term_count = prompt.split_whitespace().count();
    assert!(term_count <= 180, "term cap exceeded: {term_count}");
}

#[test]
fn screen_context_default_has_empty_tokens() {
    let ctx = ScreenContext::default();
    assert!(ctx.is_empty());
    assert_eq!(ctx.tokens.len(), 0);
}

#[test]
fn extract_max_constants_in_sane_range() {
    // Sanity-check the public constants haven't drifted.
    const {
        assert!(MAX_SCREEN_TOKENS >= 10 && MAX_SCREEN_TOKENS <= 100);
    }
}

#[test]
fn whisper_filter_keeps_alphabetic_tokens() {
    assert!(is_useful_for_whisper("clipslop.py"));
    assert!(is_useful_for_whisper("useEffect"));
    assert!(is_useful_for_whisper("get_user_by_id"));
    assert!(is_useful_for_whisper("--no-verify"));
    assert!(is_useful_for_whisper("feat-screen-context"));
    assert!(is_useful_for_whisper("FloatingPill.tsx"));
    assert!(is_useful_for_whisper("Cargo.toml"));
}

#[test]
fn whisper_filter_drops_numeric_dominant() {
    // The exact failure mode that broke "app data" dictation: Whisper biased
    // toward emitting numbers because its prompt was full of these.
    assert!(!is_useful_for_whisper("1.2.3"));
    assert!(!is_useful_for_whisper("0.61.0"));
    assert!(!is_useful_for_whisper("v0.2.5"));
    assert!(!is_useful_for_whisper("2024.01.15"));
    assert!(!is_useful_for_whisper("13:42:00"));
    assert!(!is_useful_for_whisper("83c59b2"));
    assert!(!is_useful_for_whisper("a1b2c3d4e5f6"));
    assert!(!is_useful_for_whisper("550e8400-e29b-41d4-a716-446655440000"));
}

#[test]
fn whisper_filter_requires_minimum_alpha() {
    // Two letters isn't enough — common file extensions like ".rs" should
    // never pass through alone (the full filename does, since the stem adds
    // letters).
    assert!(!is_useful_for_whisper("a"));
    assert!(!is_useful_for_whisper("ab"));
    // Boundary cases on the 40 %-alphabetic threshold.  Stems must contain
    // a non-hex letter so the all-hex SHA filter doesn't reject them.
    assert!(is_useful_for_whisper("123code"));   // 4/7 ≈ 57 %, has 'o'
    assert!(is_useful_for_whisper("1234zxy"));   // 3/7 ≈ 43 %, no hex
    assert!(!is_useful_for_whisper("12345zxy")); // 3/8 = 37.5 %, fails alpha share
}

#[test]
fn build_initial_prompt_filters_versions_and_hashes_from_whisper() {
    // Real-world regression: with a release build console open, the screen
    // had "v0.2.5", "27.85s", "57.29s", "1420", paths, etc.  Without the
    // filter, Whisper output digits in place of dictated words.
    let ctx = ScreenContext {
        tokens: vec![
            "clipslop.py".into(),
            "v0.2.5".into(),
            "27.85s".into(),
            "83c59b2".into(),
            "useEffect".into(),
            "1.2.3.4".into(),
        ],
        ..Default::default()
    };
    let prompt = build_initial_prompt(&ctx, None).unwrap();
    assert!(prompt.contains("clipslop.py"));
    assert!(prompt.contains("useEffect"));
    assert!(!prompt.contains("v0.2.5"));
    assert!(!prompt.contains("83c59b2"));
    assert!(!prompt.contains("1.2.3.4"));
}

#[test]
fn build_initial_prompt_caps_screen_contribution_at_15() {
    // Even with many alphabetic tokens, the screen-context portion of the
    // Whisper prompt is capped at 15 — beyond that, Whisper's decoder is
    // overly biased toward prompt content even on unrelated dictation.
    let many: Vec<String> = (0..50).map(|i| format!("identTok{i}")).collect();
    let ctx = ScreenContext {
        tokens: many,
        ..Default::default()
    };
    let prompt = build_initial_prompt(&ctx, None).unwrap();
    let term_count = prompt.split_whitespace().count();
    assert_eq!(term_count, 15);
}

#[test]
fn build_initial_prompt_returns_none_when_all_tokens_filtered() {
    // If the only screen tokens are versions/hashes (numeric noise), the
    // Whisper prompt should fall back to the base vocabulary unchanged.
    let ctx = ScreenContext {
        tokens: vec!["1.2.3".into(), "v0.2.5".into(), "83c59b2".into()],
        ..Default::default()
    };
    assert!(build_initial_prompt(&ctx, None).is_none());
    let with_base = build_initial_prompt(&ctx, Some("OmniVox React")).unwrap();
    assert_eq!(with_base, "OmniVox React");
}
