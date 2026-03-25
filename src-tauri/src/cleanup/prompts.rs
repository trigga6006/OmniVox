use crate::cleanup::types::{CleanupMode, RewriteStrength};

/// Build the system prompt for the cleanup model based on mode and strength.
pub fn build_system_prompt(mode: CleanupMode, strength: RewriteStrength) -> String {
    let base = base_instructions();
    let mode_instructions = mode_prompt(mode);
    let strength_instructions = strength_prompt(strength);

    format!(
        "{base}\n\n## Mode: {mode_label}\n{mode_instructions}\n\n## Rewrite Intensity: {strength_label}\n{strength_instructions}",
        mode_label = mode.label(),
        strength_label = strength.label(),
    )
}

/// Build the user prompt that wraps the raw transcript.
pub fn build_user_prompt(raw_text: &str) -> String {
    format!(
        "Rewrite the following dictated text according to your instructions. Output ONLY the rewritten text, nothing else.\n\n---\n{raw_text}\n---"
    )
}

fn base_instructions() -> &'static str {
    "You are a local text cleanup assistant for a dictation app. Your job is to \
take raw speech-to-text output and produce a cleaner version.

## Core Rules
- Output ONLY the rewritten text. No explanations, no markdown formatting, no quotes.
- PRESERVE the user's intent and meaning exactly.
- NEVER invent requirements, files, APIs, modules, or technical details not present in the input.
- NEVER silently change the scope or add tasks the user did not mention.
- If the user expressed uncertainty, PRESERVE that uncertainty.
- Remove speech filler words (um, uh, like, you know, basically, actually, kind of, sort of).
- Remove false starts and repeated phrases.
- Fix obvious grammar issues.
- Keep output as plain text — no bullet points or headers unless the input clearly intended a list."
}

fn mode_prompt(mode: CleanupMode) -> &'static str {
    match mode {
        CleanupMode::Clean => "\
- Remove filler and spoken-language clutter.
- Fix grammar lightly.
- Improve readability.
- Preserve the user's original wording as much as possible.
- Minimal restructuring — keep the same sentence order and structure.
- Do NOT aggressively paraphrase or rewrite.",

        CleanupMode::TechnicalRectify => "\
- Remove filler and improve structure.
- Correct obvious technical recognition errors ONLY when you are highly confident \
(e.g., \"post gress\" → \"PostgreSQL\", \"jason\" → \"JSON\", \"get hub\" → \"GitHub\").
- Normalize technical terminology to standard forms.
- Do NOT guess at technical terms you are unsure about — leave them as-is.
- Improve sentence structure for clarity.
- Preserve technical intent precisely.",

        CleanupMode::AgentOptimize => "\
- Rewrite into clearer, more actionable instructions suitable for an AI assistant.
- Preserve the exact scope — do not add or remove tasks.
- Improve ordering: put the most important instruction first.
- Be specific and explicit about what needs to be done.
- Remove ambiguity where possible, but preserve genuine uncertainty.
- Keep the output concise — remove redundancy without losing information.",

        CleanupMode::ClaudeCodeOptimize => "\
- Rewrite into clear, explicit instructions optimized for a coding AI agent (Claude Code).
- Emphasize: explicit task description, debugging order, constraints, minimal diffs, testing expectations.
- Structure as direct instructions: what to investigate, what to change, what to test.
- Preserve scope exactly — do not expand or invent requirements.
- If the user mentioned multiple tasks, order them logically (diagnose → fix → test).
- Keep instructions actionable and specific.
- Avoid vague language like \"make it better\" — instead specify what \"better\" means in context.",
    }
}

fn strength_prompt(strength: RewriteStrength) -> &'static str {
    match strength {
        RewriteStrength::Conservative => "\
- Apply very light cleanup only.
- Minimal paraphrasing — preserve the user's exact phrasing wherever possible.
- Only fix clear grammar errors and remove obvious filler words.
- Do NOT restructure sentences or change word choices.
- The output should read very close to the original, just cleaner.",

        RewriteStrength::Balanced => "\
- Apply standard cleanup with moderate restructuring.
- Improve clarity and flow while preserving meaning.
- Paraphrase where it genuinely improves readability.
- Condense wordy passages but keep all important details.
- This is the recommended default level.",

        RewriteStrength::Aggressive => "\
- Apply strong compression and restructuring.
- Condense heavily — remove all redundancy.
- Restructure freely for maximum clarity and conciseness.
- Still MUST preserve all user intent and never invent requirements.
- The output can read quite differently from the input, but the meaning must be identical.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_mode_strength_combinations_produce_non_empty_prompts() {
        let modes = [
            CleanupMode::Clean,
            CleanupMode::TechnicalRectify,
            CleanupMode::AgentOptimize,
            CleanupMode::ClaudeCodeOptimize,
        ];
        let strengths = [
            RewriteStrength::Conservative,
            RewriteStrength::Balanced,
            RewriteStrength::Aggressive,
        ];

        for mode in &modes {
            for strength in &strengths {
                let system = build_system_prompt(*mode, *strength);
                let user = build_user_prompt("test input");
                assert!(!system.is_empty(), "Empty system prompt for {mode:?}/{strength:?}");
                assert!(!user.is_empty(), "Empty user prompt");
                assert!(system.contains("Core Rules"), "Missing base instructions for {mode:?}");
                assert!(system.contains(mode.label()), "Missing mode label for {mode:?}");
                assert!(system.contains(strength.label()), "Missing strength label for {strength:?}");
            }
        }
    }

    #[test]
    fn user_prompt_contains_raw_text() {
        let prompt = build_user_prompt("hello world test");
        assert!(prompt.contains("hello world test"));
    }
}
