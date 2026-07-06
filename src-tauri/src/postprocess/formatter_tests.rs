use super::*;

// ── Marker stripping ──────────────────────────────────────────

#[test]
fn strips_dash_bullets() {
    assert_eq!(
        strip_existing_markers("- First item\n- Second item\n- Third item"),
        "First item Second item Third item"
    );
}

#[test]
fn strips_asterisk_bullets() {
    assert_eq!(
        strip_existing_markers("* Buy milk\n* Buy eggs"),
        "Buy milk Buy eggs"
    );
}

#[test]
fn strips_numbered_list() {
    assert_eq!(
        strip_existing_markers("1. First\n2. Second\n3. Third"),
        "First Second Third"
    );
}

#[test]
fn does_not_strip_decimal_like_numbered_prefix() {
    assert_eq!(
        strip_existing_markers("1. 5 million dollars"),
        "1. 5 million dollars"
    );
    assert_eq!(format_lists("1. 5 million dollars"), "1. 5 million dollars");
}

#[test]
fn does_not_strip_numeric_continuation_prefix() {
    assert_eq!(
        strip_existing_markers("2. 2026 goals are still written as a number"),
        "2. 2026 goals are still written as a number"
    );
}

#[test]
fn strips_heading_markers() {
    assert_eq!(
        strip_existing_markers("## My List\n- Item one\n- Item two"),
        "My List Item one Item two"
    );
}

#[test]
fn strips_bold_markers() {
    assert_eq!(strip_inline_bold("**Important**"), "Important");
    assert_eq!(strip_inline_bold("__Also bold__"), "Also bold");
}

#[test]
fn strips_unicode_bullets() {
    assert_eq!(strip_existing_markers("• First\n• Second"), "First Second");
}

#[test]
fn no_markers_passthrough() {
    let input = "Just a normal sentence with no markers.";
    assert_eq!(strip_existing_markers(input), input);
}

// ── Sentence splitting ────────────────────────────────────────

#[test]
fn splits_basic_sentences() {
    let result = split_sentences("Hello world. How are you? Great!");
    assert_eq!(result, vec!["Hello world.", "How are you?", "Great!"]);
}

#[test]
fn handles_abbreviations() {
    let result = split_sentences("Dr. Smith went to the U.S. embassy. He arrived early.");
    // Should NOT split at "Dr." or "U." or "S."
    assert_eq!(result.len(), 2, "Got: {result:?}");
    assert!(result[0].contains("Dr. Smith"));
    assert!(result[0].contains("U.S."));
}

#[test]
fn handles_decimal_numbers() {
    let result = split_sentences("The price is 3.5 million dollars. That seems high.");
    assert_eq!(result.len(), 2, "Got: {result:?}");
    assert!(result[0].contains("3.5"));
}

#[test]
fn handles_ellipsis() {
    let result = split_sentences("I was thinking... maybe we should go.");
    // Ellipsis should not split into multiple sentences
    assert_eq!(result.len(), 1, "Got: {result:?}");
}

// ── Counted header (Pattern 1) ────────────────────────────────

#[test]
fn count_word_header() {
    let input = "I'm testing the cleaning ability to format text for the project we're building. \
                 I want these three tasks tested before we ship it. \
                 I want to test the maximum number of outputs from the API service. \
                 I want to get the token count at least above 500 for each response. \
                 And I want to see how many people are in the active chat rooms.";
    let result = format_lists(input);
    assert!(result.contains("- I want to test the maximum number of outputs from the API service."));
    assert!(
        result.contains("- I want to get the token count at least above 500 for each response.")
    );
    assert!(result.contains("- I want to see how many people are in the active chat rooms."));
    assert!(result.contains(
        "I'm testing the cleaning ability to format text for the project we're building."
    ));
    assert!(result.contains("I want these three tasks tested before we ship it."));
}

#[test]
fn counted_things_header_formats_exact_items() {
    let input = "I want to do these three things before I stop working on the release tonight. \
                 Update the settings panel so the copy is easier to scan for regular users. \
                 Fix the dictation formatter so it only creates lists from explicit counted requests. \
                 Run the regression tests and make sure plain dictation stays as prose.";
    let result = format_lists(input);
    assert!(result.contains("I want to do these three things"));
    assert!(result
        .contains("- Update the settings panel so the copy is easier to scan for regular users."));
    assert!(result.contains(
        "- Fix the dictation formatter so it only creates lists from explicit counted requests."
    ));
    assert!(
        result.contains("- Run the regression tests and make sure plain dictation stays as prose.")
    );
}

#[test]
fn fewer_items_than_stated_count() {
    // Short text under MIN_WORDS_FOR_LIST — should pass through.
    let input = "I want to test these three things. \
                 I want to do a Unicode test. \
                 I want to do a transformer test.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Short text should not be bulleted: {result}"
    );
}

#[test]
fn implicit_task_header_stays_prose_without_count() {
    let input = "Here are the tasks I want to complete before the next project release. \
                 I want to do a full Unicode compatibility test on the frontend application. \
                 I want to do a transformer performance test on the backend service layer. \
                 I want to check the output format for correctness and readability. \
                 I want to verify the error handling works for all edge cases. \
                 I want to run the full integration suite against the staging server.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Implicit task headers should stay prose without an explicit count: {result}"
    );
}

// ── Implicit header (Pattern 2) — requires 5+ items ──────────

#[test]
fn implicit_header_no_count() {
    // "these tasks" without a number — needs 5+ items and 40+ words.
    let input = "I want to go over and test these tasks for the project we are working on. \
                 Do a Unicode compatibility test on the frontend interfaces. \
                 Do a transformer performance test on all the API endpoints. \
                 Check the output format for correctness and accuracy overall. \
                 Run the full regression suite against the production environment. \
                 Verify the deployment pipeline works end to end correctly.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Implicit headers should stay prose unless the user gives a count: {result}"
    );
}

#[test]
fn implicit_header_four_items_not_bulleted() {
    // Implicit header with only 4 items should NOT trigger (threshold is 5).
    let input =
        "I want to go ahead and test all of these tasks for the project we are working on today. \
                 Do a Unicode test on the frontend components. \
                 Do a transformer test on the backend services. \
                 Check the output format for correctness and readability. \
                 Run the full regression suite against staging.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "4 items after implicit header should not be bulleted: {result}"
    );
}

#[test]
fn signal_phrase_the_following() {
    // Needs 5+ items and 40+ words to trigger.
    let input = "For our release next week I need to do the following tasks and get them done. \
                 Update the database schema with the new migration files. \
                 Fix the integration tests that are currently broken. \
                 Deploy the staging environment to production servers. \
                 Notify the team about the upcoming downtime window. \
                 Run a final smoke test against the live environment.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Signal phrases without a count should stay prose: {result}"
    );
}

#[test]
fn implicit_list_terminates_at_conclusion() {
    // After a run of short list items, a significantly longer sentence
    // should NOT be bulleted — it's a conclusion / topic transition.
    // Items use varied phrasing to avoid triggering Pattern 4 (repeated prefix).
    let input = "Here are the tasks we need to complete for the project launch this quarter. \
                 Strip all bullet markers from the inputs. \
                 Remove heading markers from content blocks. \
                 Clean up inline bold from the text. \
                 Rejoin all lines into properly flowing text. \
                 Handle edge cases in the main parser. \
                 The formatting ability is fully preserved and still handles all the smart list detection properly including the termination heuristic and the implicit list detection logic.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Implicit lists should stay prose without an explicit count: {result}"
    );
    assert!(result.contains("The formatting ability is fully preserved"));
}

// ── Ordinal sentences (Pattern 3) — ordinals stripped ─────────

#[test]
fn ordinal_sentences_stripped() {
    // Needs 5+ consecutive ordinals and 40+ words to trigger.
    let input = "Here is the plan for the upcoming project release we need to deliver on time. \
                 First, set up the database with the new schema and run the migration scripts. \
                 Second, write the API endpoints and connect them to the new service layer. \
                 Third, build the frontend components and wire up the data fetching logic. \
                 Fourth, deploy to the staging environment and verify everything works correctly. \
                 Fifth, notify the team and update the documentation for the release.";
    let result = format_lists(input);
    // Ordinals should be stripped — no redundant "- First,"
    assert!(
        !result.contains("- "),
        "Ordinal-only runs should stay prose unless paired with an explicit count: {result}"
    );
    assert!(result.starts_with("Here is the plan"));
}

// ── Repeated starters (Pattern 4) ─────────────────────────────

#[test]
fn repeated_sentence_starters() {
    let input = "I want to do a full Unicode compatibility test on the frontend application. \
                 I want to do a transformer performance test on the backend service layer. \
                 I want to check the output format for correctness and readability. \
                 I want to verify the error handling works for all edge cases. \
                 I want to run the full integration suite against the staging server.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Repeated sentence starters without an explicit list cue should stay prose: {result}"
    );
}

#[test]
fn repeated_sentence_starters_after_header() {
    let input = "Here are the tasks I want to complete before the next project release. \
                 I want to do a full Unicode compatibility test on the frontend application. \
                 I want to do a transformer performance test on the backend service layer. \
                 I want to check the output format for correctness and readability. \
                 I want to verify the error handling works for all edge cases. \
                 I want to run the full integration suite against the staging server.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Repeated starters after an implicit header should stay prose: {result}"
    );
}

#[test]
fn repeated_starters_four_not_bulleted() {
    // Only 4 repeated starters should NOT trigger (threshold is 5).
    let input = "I want to do a full Unicode compatibility test on the application frontend. \
                 I want to do a transformer performance test on the backend service. \
                 I want to check the output format for correctness and readability. \
                 I want to verify the error handling works for all edge cases.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "4 repeated starters should not be bulleted: {result}"
    );
}

#[test]
fn common_prose_prefix_not_bulleted() {
    // "I was" / "it was" etc. are common prose — should NOT become a list.
    let input = "I was tired after work. I was thinking about dinner. I was ready to relax.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Common prose should not be bulleted: {result}"
    );
}

#[test]
fn the_meeting_prose_not_bulleted() {
    // "The meeting" is narrative prose, not a list.
    let input =
        "The meeting was productive. The meeting room was cold. The meeting notes are ready.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Narrative prose should not be bulleted: {result}"
    );
}

// ── Inline comma list (Pattern 5) ─────────────────────────────

#[test]
fn inline_comma_list_short_items_stay_inline() {
    let input = "I need milk, eggs, bread, and butter.";
    let result = format_lists(input);
    assert_eq!(result, input, "Short items should not be bulleted");
}

#[test]
fn inline_comma_list_four_items_not_bulleted() {
    // Only 4 items — needs 5+ to trigger inline list formatting.
    let input = "I need to update the database, fix the API tests, refactor the auth module, and deploy to production.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "4 inline items should not be bulleted: {result}"
    );
}

// ── Passthrough / guard tests ─────────────────────────────────

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
fn short_text_min_word_guard() {
    // Under MIN_WORDS_FOR_LIST — should never be formatted.
    let input = "Fix the bug. Run tests.";
    assert_eq!(format_lists(input), input);
}

// ── Mixed pattern tests ───────────────────────────────────────

#[test]
fn couple_of_things_with_few_items_not_bulleted() {
    // "things" removed from COLLECTION_NOUNS — casual speech should not trigger.
    // Also only 2 items after header — not enough to trigger (needs 5+).
    let input = "I really like where the design is going but there's a couple of things I want to change. \
                 First, we need to move the header down about 3 inches. \
                 Then we need to adjust the desert section and also we need to change where the lens comes in.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Too few items should not be bulleted: {result}"
    );
}

#[test]
fn first_then_also_short_not_bulleted() {
    // Under word threshold and only 3 items — should not be bulleted.
    let input = "First, we need to update the CSS. \
                 Then we need to fix the layout. \
                 Also we need to add the footer.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "Short text with few items should not be bulleted: {result}"
    );
}

#[test]
fn header_not_bulleted_couple_things() {
    // "things" removed from COLLECTION_NOUNS, and only 3 ordinals —
    // not enough for the 5+ threshold.
    let input = "There are a couple things to get done today. \
                 First, check how the LLM removes filters. \
                 Second, fix the punctuation issues. \
                 Thirdly, rewrite or shorten and add length.";
    let result = format_lists(input);
    assert!(
        !result.contains("- "),
        "3 ordinals should not be bulleted with raised threshold: {result}"
    );
}

// ── Pre-existing marker stripping integration ─────────────────

#[test]
fn strips_existing_dashes_before_formatting() {
    // Whisper output with existing dashes should not produce "- - item"
    let input = "Here are my tasks. - Update the code. - Fix the tests. - Deploy to staging.";
    let result = format_lists(input);
    assert_eq!(
        result,
        "Here are my tasks. Update the code. Fix the tests. Deploy to staging."
    );
    assert!(!result.contains("- - "), "Should not double-mark: {result}");
    assert!(
        !result.contains("- * "),
        "Should not have mixed markers: {result}"
    );
}

#[test]
fn short_numbered_markers_are_cleaned_before_guard() {
    let input = "1. First item\n2. Second item\n3. Third item";
    assert_eq!(format_lists(input), "First item Second item Third item");
}

#[test]
fn short_unicode_markers_are_cleaned_before_guard() {
    let input = "â€¢ First item\nâ€¢ Second item";
    assert_eq!(format_lists(input), "First item Second item");
}

#[test]
fn strips_markdown_bullets_before_formatting() {
    let input = "## My list\n* First thing to do\n* Second thing to do\n* Third thing to do";
    let result = format_lists(input);
    assert!(
        !result.contains("##"),
        "Heading markers should be stripped: {result}"
    );
    assert!(
        !result.contains("* "),
        "Asterisk bullets should be stripped: {result}"
    );
}

// ── Counted header: short dictations + numbered ordinals ──────

#[test]
fn short_counted_header_formats() {
    // A counted header is an explicit signal — honored below the word gate.
    let input = "I need these three things. Milk. Eggs. Bread.";
    let result = format_lists(input);
    assert_eq!(
        result,
        "I need these three things.\n- Milk.\n- Eggs.\n- Bread."
    );
}

#[test]
fn short_text_without_header_still_passes_through_exactly() {
    // Three sentences, no counted header — must round-trip byte-exact.
    let input = "I went to the store.  I bought milk. I came home.";
    assert_eq!(format_lists(input), input);
}

#[test]
fn counted_header_with_ordinal_items_becomes_numbered() {
    let input = "Here are the three steps for tonight before the launch window closes. \
                 First, set up the database with the new schema and migrations. \
                 Second, write the API endpoints for the new service layer. \
                 Third, deploy everything to the staging environment for review.";
    let result = format_lists(input);
    assert!(
        result.contains("1. Set up the database with the new schema and migrations."),
        "ordinal items should be numbered with ordinals stripped: {result}"
    );
    assert!(result.contains("2. Write the API endpoints for the new service layer."));
    assert!(result.contains("3. Deploy everything to the staging environment for review."));
    assert!(!result.contains("First,"), "spoken ordinal should be stripped: {result}");
}

#[test]
fn counted_header_without_ordinals_stays_bulleted() {
    let input = "I need these three items handled before the end of the day today. \
                 Update the settings panel copy so users can scan it quickly. \
                 Fix the formatter so lists only come from counted requests. \
                 Run the regression tests against the release branch.";
    let result = format_lists(input);
    assert!(result.contains("- Update the settings panel copy"));
    assert!(!result.contains("1. "), "non-ordinal items stay bullets: {result}");
}

#[test]
fn ordinal_only_run_still_stays_prose() {
    // No counted header — ordinals alone must never trigger a list.
    let input = "First, we look at the budget for the quarter and trim it down. \
                 Second, we meet with the design team about the new landing page. \
                 Third, we finalize the hiring plan for the platform group. \
                 Fourth, we review the incident postmortem from last week. \
                 Fifth, we close out the roadmap review with the stakeholders.";
    let result = format_lists(input);
    assert!(!result.contains("- "), "ordinal-only runs stay prose: {result}");
    assert!(!result.contains("1. "), "ordinal-only runs stay prose: {result}");
}
