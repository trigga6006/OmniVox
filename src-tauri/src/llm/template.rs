use crate::llm::schema::{SlotExtraction, Urgency};

/// Render a `SlotExtraction` into the canonical target-agnostic Markdown.
///
/// Empty slots produce no section header at all (the plan deliberately avoids
/// "N/A" noise).  The resulting string is what the pipeline paste/copy path
/// ends up sending to the target editor.
pub fn render_markdown(s: &SlotExtraction) -> String {
    let mut out = String::new();

    // Goal — always present (grammar guarantees it).
    out.push_str("## Goal\n");
    out.push_str(s.goal.trim());
    out.push('\n');

    if !s.context.is_empty() {
        out.push_str("\n## Context\n");
        for item in &s.context {
            let trimmed = item.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str("- ");
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    if !s.constraints.is_empty() {
        out.push_str("\n## Constraints\n");
        for c in &s.constraints {
            let trimmed = c.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str("- ");
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    if !s.files.is_empty() {
        out.push_str("\n## Files / Components\n");
        for f in &s.files {
            let trimmed = f.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str("- `");
            out.push_str(trimmed);
            out.push_str("`\n");
        }
    }

    if let Some(u) = s.urgency {
        out.push_str("\n## Urgency\n");
        out.push_str(match u {
            Urgency::Low => "low",
            Urgency::Normal => "normal",
            Urgency::High => "high",
        });
        out.push('\n');
    }

    if !s.expected_behavior.is_empty() {
        out.push_str("\n## Expected Behavior\n");
        for t in &s.expected_behavior {
            let trimmed = t.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str("- ");
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    if !s.questions.is_empty() {
        out.push_str("\n## Open Questions\n");
        for q in &s.questions {
            let trimmed = q.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str("- ");
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    if !s.options.is_empty() {
        out.push_str("\n## Options\n");
        for o in &s.options {
            let trimmed = o.trim();
            if trimmed.is_empty() {
                continue;
            }
            out.push_str("- ");
            out.push_str(trimmed);
            out.push('\n');
        }
    }

    out
}
