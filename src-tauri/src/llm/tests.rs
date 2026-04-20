use crate::llm::grammar::{SLOT_EXTRACTION_ROOT, SLOT_EXTRACTION_V1};
use crate::llm::schema::{SlotExtraction, Urgency};
use crate::llm::template::render_markdown;

#[test]
fn grammar_has_root_rule() {
    assert!(
        SLOT_EXTRACTION_V1.contains("root"),
        "grammar should define root"
    );
    assert_eq!(SLOT_EXTRACTION_ROOT, "root");
}

#[test]
fn grammar_string_cap_fits_real_goals() {
    // Past regression: a 220-char string cap clipped real goals mid-sentence.
    assert!(
        SLOT_EXTRACTION_V1.contains("char{1,520}"),
        "bounded-string cap should be large enough to hold a real goal"
    );
}

#[test]
fn grammar_uses_expected_behavior_key() {
    // The rename from follow_up_tasks → expected_behavior must be reflected
    // in the grammar or the LLM will keep emitting the old key name.
    assert!(
        SLOT_EXTRACTION_V1.contains("expected_behavior"),
        "grammar must reference expected_behavior"
    );
    assert!(
        !SLOT_EXTRACTION_V1.contains("follow_up_tasks"),
        "grammar must not still reference follow_up_tasks"
    );
}

#[test]
fn grammar_includes_questions_and_options_for_dynamic_intents() {
    // Exploration and advice prompts use the `questions` and `options`
    // slots respectively.  If either disappears from the grammar, the
    // model silently loses the ability to emit them.
    assert!(SLOT_EXTRACTION_V1.contains("questions"));
    assert!(SLOT_EXTRACTION_V1.contains("options"));
}

#[test]
fn schema_parses_goal_only() {
    let json = r#"{"goal":"Refactor auth"}"#;
    let s: SlotExtraction = serde_json::from_str(json).unwrap();
    assert_eq!(s.goal, "Refactor auth");
    assert!(s.context.is_empty());
    assert!(s.constraints.is_empty());
    assert!(s.files.is_empty());
    assert!(s.urgency.is_none());
    assert!(s.expected_behavior.is_empty());
}

#[test]
fn schema_parses_all_fields() {
    let json = r#"{
        "goal":"Refactor checkout",
        "context":["Current failures only happen on long prompts"],
        "constraints":["Do not break Stripe"],
        "files":["billing.tsx","cart.tsx"],
        "urgency":"high",
        "expected_behavior":["I should be able to complete checkout without the prompt failing"],
        "questions":["what is the failure mode on long prompts"],
        "options":["retry with backoff","chunk the prompt"]
    }"#;
    let s: SlotExtraction = serde_json::from_str(json).unwrap();
    assert_eq!(s.goal, "Refactor checkout");
    assert_eq!(s.context, vec!["Current failures only happen on long prompts"]);
    assert_eq!(s.constraints, vec!["Do not break Stripe"]);
    assert_eq!(s.files, vec!["billing.tsx", "cart.tsx"]);
    assert_eq!(s.urgency, Some(Urgency::High));
    assert_eq!(
        s.expected_behavior,
        vec!["I should be able to complete checkout without the prompt failing"]
    );
    assert_eq!(s.questions, vec!["what is the failure mode on long prompts"]);
    assert_eq!(s.options, vec!["retry with backoff", "chunk the prompt"]);
}

#[test]
fn schema_parses_exploration_shape() {
    // Exploration intent: goal + context + questions.  No constraints,
    // no files, no expected_behavior.  That's a valid shape.
    let json = r#"{
        "goal":"explore what it would take to support more languages",
        "context":["Whisper handles multilingual out of the box"],
        "questions":[
            "what is the VAD story for non-English",
            "how much memory per language"
        ]
    }"#;
    let s: SlotExtraction = serde_json::from_str(json).unwrap();
    assert_eq!(s.goal, "explore what it would take to support more languages");
    assert!(s.constraints.is_empty());
    assert!(s.files.is_empty());
    assert!(s.expected_behavior.is_empty());
    assert!(s.options.is_empty());
    assert_eq!(s.questions.len(), 2);
}

#[test]
fn schema_parses_advice_shape() {
    // Advice intent: goal + context + options + constraints.  No files,
    // no expected_behavior is still a valid shape.
    let json = r#"{
        "goal":"decide between SQLite and a JSON file for transcripts",
        "context":["SQLite gives us queries","JSON is easy to debug"],
        "options":["use SQLite","use a flat JSON file"],
        "constraints":["saving must not block dictation"]
    }"#;
    let s: SlotExtraction = serde_json::from_str(json).unwrap();
    assert_eq!(s.options.len(), 2);
    assert_eq!(s.constraints, vec!["saving must not block dictation"]);
    assert!(s.questions.is_empty());
    assert!(s.expected_behavior.is_empty());
}

#[test]
fn schema_parses_urgency_lowercase() {
    for (s, expected) in [
        ("low", Urgency::Low),
        ("normal", Urgency::Normal),
        ("high", Urgency::High),
    ] {
        let json = format!(r#"{{"goal":"g","urgency":"{s}"}}"#);
        let parsed: SlotExtraction = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.urgency, Some(expected));
    }
}

#[test]
fn schema_normalize_dedupes_and_trims_lists() {
    let s = SlotExtraction {
        goal: "  Investigate timeout  ".into(),
        context: vec!["  current behavior  ".into(), "current behavior".into(), "".into()],
        constraints: vec![" do not touch Qwen ".into(), "Do not touch Qwen".into()],
        expected_behavior: vec![" the app should still run ".into(), "The app should still run".into()],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.goal, "Investigate timeout");
    assert_eq!(s.context, vec!["current behavior"]);
    assert_eq!(s.constraints, vec!["do not touch Qwen"]);
    assert_eq!(s.expected_behavior, vec!["the app should still run"]);
}

#[test]
fn schema_normalize_drops_punctuation_only_entries() {
    let s = SlotExtraction {
        goal: "g".into(),
        context: vec![",".into(), "],".into(), "real context".into()],
        constraints: vec!["-".into(), "a real constraint".into()],
        expected_behavior: vec!["real behavior".into(), "   ".into()],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.context, vec!["real context"]);
    assert_eq!(s.constraints, vec!["a real constraint"]);
    assert_eq!(s.expected_behavior, vec!["real behavior"]);
}

#[test]
fn schema_normalize_filters_bogus_file_entries() {
    let s = SlotExtraction {
        goal: "g".into(),
        files: vec![
            "billing.tsx".into(),
            "StructuredModeSection".into(),
            "files[]:".into(),
            ":urgency:normal".into(),
            "],".into(),
        ],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.files, vec!["billing.tsx", "StructuredModeSection"]);
}

#[test]
fn schema_normalize_preserves_natural_phrasing_in_context() {
    // Regression: a previous pass stripped anything starting with "I want to"
    // or "Another quick tweak" as a "meta-preface".
    let s = SlotExtraction {
        goal: "g".into(),
        context: vec![
            "I want to make the panel feel more premium".into(),
            "Another quick tweak to the UI".into(),
        ],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.context.len(), 2);
}

#[test]
fn schema_normalize_dedupes_constraint_that_also_appears_in_behavior() {
    // User's chief complaint: same item appeared in both constraints and
    // follow_up_tasks.  Expected_behavior is the richer framing, so the
    // duplicate gets removed from constraints.
    let s = SlotExtraction {
        goal: "keep the panel open".into(),
        constraints: vec![
            "the menu should stay open".into(),
            "do not touch the paste button".into(),
        ],
        expected_behavior: vec!["The menu should stay open".into()],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.constraints, vec!["do not touch the paste button"]);
    assert_eq!(s.expected_behavior, vec!["The menu should stay open"]);
}

#[test]
fn schema_normalize_drops_list_items_that_repeat_the_goal() {
    let s = SlotExtraction {
        goal: "keep the pill overlay menu open while switching modes".into(),
        constraints: vec![
            "keep the pill overlay menu open while switching modes".into(),
            "don't break auto-switch".into(),
        ],
        expected_behavior: vec![
            "keep the pill overlay menu open while switching modes".into(),
            "I should be able to click off to close it".into(),
        ],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.constraints, vec!["don't break auto-switch"]);
    assert_eq!(
        s.expected_behavior,
        vec!["I should be able to click off to close it"]
    );
}

#[test]
fn schema_normalize_rewrites_third_person_user_reference_in_behavior() {
    // The model occasionally narrates ABOUT the speaker ("the user should…")
    // instead of AS the speaker.  That's model commentary leaking through
    // and has to be rewritten before the user pastes into a coding agent.
    let s = SlotExtraction {
        goal: "The user wants to add a provider picker".into(),
        expected_behavior: vec![
            "The user should be able to pick which agent receives the prompt".into(),
            "the user's dictation should route to the chosen agent".into(),
        ],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.goal, "I want to add a provider picker");
    assert_eq!(
        s.expected_behavior,
        vec![
            "I should be able to pick which agent receives the prompt",
            "my dictation should route to the chosen agent",
        ]
    );
}

#[test]
fn schema_normalize_preserves_user_interface_etc() {
    // Safety: we must not damage legitimate uses like "user interface",
    // "user experience", or plural "users".  The rewriter targets only
    // "the user <verb>" and possessive patterns.
    let s = SlotExtraction {
        goal: "improve the user interface".into(),
        context: vec![
            "the user experience feels sluggish".into(),
            "users on low-RAM machines see stalls".into(),
        ],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.goal, "improve the user interface");
    assert_eq!(
        s.context,
        vec!["the user experience feels sluggish", "users on low-RAM machines see stalls"]
    );
}

#[test]
fn schema_normalize_rewrites_second_person_leak() {
    let s = SlotExtraction {
        goal: "g".into(),
        expected_behavior: vec![
            "You should be able to dictate into the panel".into(),
            "you can click off to dismiss".into(),
        ],
        ..Default::default()
    }
    .normalize();
    assert_eq!(
        s.expected_behavior,
        vec![
            "I should be able to dictate into the panel",
            "I can click off to dismiss",
        ]
    );
}

#[test]
fn schema_normalize_drops_context_that_matches_constraint_or_behavior() {
    let s = SlotExtraction {
        goal: "g".into(),
        context: vec![
            "do not break auth".into(),
            "a real background fact".into(),
            "I should be able to paste instantly".into(),
        ],
        constraints: vec!["do not break auth".into()],
        expected_behavior: vec!["I should be able to paste instantly".into()],
        ..Default::default()
    }
    .normalize();
    assert_eq!(s.context, vec!["a real background fact"]);
}

#[test]
fn schema_normalize_with_raw_drops_ungrounded_files() {
    // Canonical fabrication pattern: the model invents a file the user
    // never named.  With the grounded-files check, these go away.
    let raw = "Fix the pill overlay so clicking a mode doesn't close it.";
    let s = SlotExtraction {
        goal: "g".into(),
        files: vec![
            "billing.tsx".into(),      // user never said this
            "FloatingPill.tsx".into(), // user said "pill overlay" → grounds
            "Unrelated.rs".into(),     // pure fabrication
        ],
        ..Default::default()
    }
    .normalize_with_raw(raw);
    assert_eq!(s.files, vec!["FloatingPill.tsx"]);
}

#[test]
fn schema_normalize_with_raw_grounds_camelcase_file_to_spoken_word() {
    // User said two words; LLM output as CamelCase file.  Should still
    // ground because we split camel case.
    let raw = "Tweak the floating pill overlay animation.";
    let s = SlotExtraction {
        goal: "g".into(),
        files: vec!["FloatingPill.tsx".into()],
        ..Default::default()
    }
    .normalize_with_raw(raw);
    assert_eq!(s.files, vec!["FloatingPill.tsx"]);
}

#[test]
fn schema_normalize_with_raw_short_input_drops_ungrounded_slots() {
    // Threshold-length fabrication: user input is short, but the LLM
    // invented context / constraints / behavior with zero word overlap.
    // Those get stripped; the goal survives.
    let raw = "Make the panel slide out more smoothly.";
    let s = SlotExtraction {
        goal: "make the panel slide out more smoothly".into(),
        context: vec!["the build system currently has a race condition".into()],
        constraints: vec!["do not break the login flow".into()],
        expected_behavior: vec![
            "I should be able to submit payment instantly".into(),
            "the panel should feel buttery".into(),  // shares "panel" → survives
        ],
        ..Default::default()
    }
    .normalize_with_raw(raw);
    assert_eq!(s.goal, "make the panel slide out more smoothly");
    assert!(s.context.is_empty(), "context was all fabrication");
    assert!(s.constraints.is_empty(), "constraints was all fabrication");
    assert_eq!(
        s.expected_behavior,
        vec!["the panel should feel buttery"],
        "only the grounded behavior item survives"
    );
}

#[test]
fn schema_normalize_with_raw_long_input_does_not_apply_short_gate() {
    // Long input: even if the model rephrases heavily, we trust it more.
    // The short-input gate only kicks in below 120 chars.
    let long_raw = "Another thing I want to fix is the pill overlay panel flickers \
                    on open, and the shadow clips at the bottom edge when the recording \
                    indicator is visible simultaneously.";
    let s = SlotExtraction {
        goal: "fix pill overlay flicker".into(),
        context: vec!["some rephrased context that uses different words entirely".into()],
        ..Default::default()
    }
    .normalize_with_raw(long_raw);
    // The short-input gate did NOT fire, so the rephrased context survives.
    assert_eq!(s.context.len(), 1);
}

#[test]
fn schema_normalize_with_raw_keeps_goal_even_without_overlap() {
    // The goal is never dropped by the grounding pass — the LLM always
    // provides one and it is our primary output.  Only list-valued slots
    // can be stripped by the guard.
    let raw = "Hello world";
    let s = SlotExtraction {
        goal: "completely unrelated goal text".into(),
        ..Default::default()
    }
    .normalize_with_raw(raw);
    assert_eq!(s.goal, "completely unrelated goal text");
}

#[test]
fn template_goal_only() {
    let s = SlotExtraction {
        goal: "Refactor auth".into(),
        ..Default::default()
    };
    let md = render_markdown(&s);
    assert_eq!(md, "## Goal\nRefactor auth\n");
}

#[test]
fn template_full_render() {
    let s = SlotExtraction {
        goal: "Refactor the checkout flow".into(),
        context: vec!["The short path already works".into()],
        constraints: vec!["Do not break the Stripe integration".into()],
        files: vec!["billing.tsx".into(), "cart.tsx".into()],
        urgency: Some(Urgency::High),
        expected_behavior: vec![
            "I should be able to complete checkout on long prompts".into(),
        ],
        ..Default::default()
    };
    let md = render_markdown(&s);
    let expected = "## Goal\nRefactor the checkout flow\n\
                    \n## Context\n- The short path already works\n\
                    \n## Constraints\n- Do not break the Stripe integration\n\
                    \n## Files / Components\n- `billing.tsx`\n- `cart.tsx`\n\
                    \n## Urgency\nhigh\n\
                    \n## Expected Behavior\n- I should be able to complete checkout on long prompts\n";
    assert_eq!(md, expected);
}

#[test]
fn template_renders_exploration_sections() {
    // Exploration: goal + context + questions, no implementation slots.
    let s = SlotExtraction {
        goal: "explore multilingual scaling".into(),
        context: vec!["Whisper handles multilingual".into()],
        questions: vec![
            "what is the VAD story for non-English".into(),
            "how much memory per language".into(),
        ],
        ..Default::default()
    };
    let md = render_markdown(&s);
    assert!(md.contains("## Goal\nexplore multilingual scaling\n"));
    assert!(md.contains("## Context\n- Whisper handles multilingual\n"));
    assert!(md.contains("## Open Questions\n- what is the VAD story for non-English\n- how much memory per language\n"));
    // Must NOT render empty sections for the slots an exploration prompt
    // doesn't use.
    assert!(!md.contains("## Expected Behavior"));
    assert!(!md.contains("## Options"));
    assert!(!md.contains("## Constraints"));
}

#[test]
fn template_renders_advice_sections() {
    let s = SlotExtraction {
        goal: "decide storage format".into(),
        options: vec!["SQLite".into(), "flat JSON".into()],
        constraints: vec!["must not block dictation".into()],
        ..Default::default()
    };
    let md = render_markdown(&s);
    assert!(md.contains("## Options\n- SQLite\n- flat JSON\n"));
    assert!(md.contains("## Constraints\n- must not block dictation\n"));
    assert!(!md.contains("## Open Questions"));
    assert!(!md.contains("## Expected Behavior"));
}

#[test]
fn schema_normalize_with_raw_guards_questions_and_options() {
    // The grounding guards must cover the new slots too — otherwise the
    // model could fabricate questions/options on a short input and get
    // away with it because the old guards only covered the original slots.
    let raw = "thinking about scaling whisper to more languages";
    let s = SlotExtraction {
        goal: "explore multilingual scaling".into(),
        questions: vec![
            "what is the VAD story for languages".into(), // shares whisper/languages
            "should we migrate the billing database".into(), // fabrication
        ],
        options: vec![
            "stick with whisper".into(),           // shares whisper
            "rewrite the auth flow".into(),        // fabrication
        ],
        ..Default::default()
    }
    .normalize_with_raw(raw);
    assert_eq!(
        s.questions,
        vec!["what is the VAD story for languages"]
    );
    assert_eq!(s.options, vec!["stick with whisper"]);
}

#[test]
fn template_expected_behavior_renders_as_list() {
    let s = SlotExtraction {
        goal: "g".into(),
        expected_behavior: vec!["a".into(), "b".into(), "c".into()],
        ..Default::default()
    };
    let md = render_markdown(&s);
    assert!(md.ends_with("## Expected Behavior\n- a\n- b\n- c\n"));
}

#[test]
fn template_skips_empty_string_list_entries() {
    let s = SlotExtraction {
        goal: "g".into(),
        context: vec!["".into(), "kept".into()],
        constraints: vec!["real".into(), "  ".into(), "".into()],
        ..Default::default()
    };
    let md = render_markdown(&s);
    assert_eq!(md.matches("- ").count(), 2);
    assert!(md.contains("- kept"));
    assert!(md.contains("- real"));
}

#[test]
fn prompt_template_uses_qwen_markers() {
    let p = crate::llm::prompt::format_prompt("hello");
    assert!(p.starts_with("<|im_start|>system\n"));
    assert!(p.contains("ACTUAL DICTATION:\nhello"));
    assert!(p.contains("/no_think"));
    assert!(p.ends_with("<|im_start|>assistant\n"));
    // Sanity: the example's goal must not leak as if it were user text.
    assert!(!p.contains("Refactor the checkout flow"));
    // Sanity: the new slot name is in the system prompt, the old one isn't.
    assert!(p.contains("expected_behavior"));
    assert!(!p.contains("follow_up_tasks"));
}
