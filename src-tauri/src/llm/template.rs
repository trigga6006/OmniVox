use crate::llm::schema::{EmailExtraction, NotesExtraction, SlotExtraction, Urgency};

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

/// Render an `EmailExtraction` into a ready-to-paste draft.
///
/// Deliberately markdown-light: `To:` / `Subject:` header lines when present,
/// then the body points as paragraphs, then the sign-off.  Empty slots render
/// nothing — a subject-less, recipient-less dictation is just the body.
pub fn render_email(e: &EmailExtraction) -> String {
    let mut out = String::new();

    if !e.recipient_hint.is_empty() {
        out.push_str("To: ");
        out.push_str(e.recipient_hint.trim());
        out.push('\n');
    }
    if !e.subject.is_empty() {
        out.push_str("Subject: ");
        out.push_str(e.subject.trim());
        out.push('\n');
    }

    for point in &e.body_points {
        let trimmed = point.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed);
        out.push('\n');
    }

    if !e.sign_off.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(e.sign_off.trim());
        out.push('\n');
    }

    out
}

/// Render a `NotesExtraction` into a clean markdown outline: `##` title,
/// `###` section headings, `-` bullet points.  Heading-less sections render
/// their bullets directly.
pub fn render_notes(n: &NotesExtraction) -> String {
    let mut out = String::new();

    if !n.title.is_empty() {
        out.push_str("## ");
        out.push_str(n.title.trim());
        out.push('\n');
    }

    for section in &n.sections {
        if section.points.is_empty() {
            continue;
        }
        if !section.heading.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str("### ");
            out.push_str(section.heading.trim());
            out.push('\n');
        } else if !out.is_empty() {
            out.push('\n');
        }
        for point in &section.points {
            let trimmed = point.trim();
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
