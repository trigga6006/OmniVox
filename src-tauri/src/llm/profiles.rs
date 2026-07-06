//! Compile-time registry of Structured Mode profiles.
//!
//! A profile is a (system prompt, GBNF grammar, template) triple: what the
//! LLM is asked to do, the exact JSON shape it is physically allowed to emit,
//! and how that JSON renders into the panel's Markdown.  Keeping all three
//! together per profile preserves the anti-hallucination design of the
//! original single-profile pipeline.
//!
//! KV-cache contract: the engine keeps ONE session warmed on the ACTIVE
//! profile's system prompt (byte-identical every call — see plan §3.4).
//! Switching profiles re-warms in the background via
//! `LlmRunner::set_profile`; there is never more than one KV session.

use crate::error::{AppError, AppResult};
use crate::llm::grammar::{
    EMAIL_DRAFT_ROOT, EMAIL_DRAFT_V1, NOTES_OUTLINE_ROOT, NOTES_OUTLINE_V1, SLOT_EXTRACTION_ROOT,
    SLOT_EXTRACTION_V1,
};
use crate::llm::prompt::{EMAIL_SYSTEM_PROMPT, NOTES_SYSTEM_PROMPT, SYSTEM_PROMPT};
use crate::llm::schema::{EmailExtraction, NotesExtraction, SlotExtraction};
use crate::llm::template::{render_email, render_markdown, render_notes};

/// Result of one profile-aware extraction: the rendered Markdown plus the
/// profile-specific slot object (serialized for the panel payload — slot
/// shapes differ per profile, so the wire format is a plain JSON value).
#[derive(Debug, Clone)]
pub struct ProfileOutput {
    pub markdown: String,
    pub slots: serde_json::Value,
}

/// One Structured Mode profile.  All fields are `'static` — the registry is
/// compile-time only; there are no user-defined profiles.
pub struct Profile {
    pub id: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub system_prompt: &'static str,
    pub grammar: &'static str,
    pub grammar_root: &'static str,
    /// Whether the screen-context token block is meaningful for this profile.
    /// Only the agent prompt documents SCREEN CONTEXT; feeding tokens to the
    /// other profiles would invite copying on-screen text into an email/note.
    pub uses_screen_context: bool,
    /// Parse the raw grammar-constrained JSON, apply the anti-fabrication
    /// grounding pass against the dictation, and render the Markdown.
    pub postprocess: fn(raw_json: &str, raw_input: &str) -> AppResult<ProfileOutput>,
}

pub const DEFAULT_PROFILE_ID: &str = "agent-prompt";

/// All profiles.  The first entry is the default (`agent-prompt`, the
/// original Structured Mode behavior).
pub static PROFILES: &[Profile] = &[
    Profile {
        id: DEFAULT_PROFILE_ID,
        display_name: "Agent Prompt",
        description: "Slot-filled prompt for an AI coding agent (goal, context, constraints, …)",
        system_prompt: SYSTEM_PROMPT,
        grammar: SLOT_EXTRACTION_V1,
        grammar_root: SLOT_EXTRACTION_ROOT,
        uses_screen_context: true,
        postprocess: postprocess_agent,
    },
    Profile {
        id: "email",
        display_name: "Email",
        description: "Ready-to-paste email draft (recipient, subject, body, sign-off)",
        system_prompt: EMAIL_SYSTEM_PROMPT,
        grammar: EMAIL_DRAFT_V1,
        grammar_root: EMAIL_DRAFT_ROOT,
        uses_screen_context: false,
        postprocess: postprocess_email,
    },
    Profile {
        id: "notes-outline",
        display_name: "Notes",
        description: "Hierarchical markdown outline (title, sections, bullet points)",
        system_prompt: NOTES_SYSTEM_PROMPT,
        grammar: NOTES_OUTLINE_V1,
        grammar_root: NOTES_OUTLINE_ROOT,
        uses_screen_context: false,
        postprocess: postprocess_notes,
    },
];

/// Look up a profile by id.  Empty or unknown ids resolve to the default
/// `agent-prompt` profile — a mode with no explicit choice keeps the original
/// behavior, and a stale id from an old install degrades safely.
pub fn get(id: &str) -> &'static Profile {
    PROFILES
        .iter()
        .find(|p| p.id == id)
        .unwrap_or(&PROFILES[0])
}

fn slots_value<T: serde::Serialize>(slots: &T) -> AppResult<serde_json::Value> {
    serde_json::to_value(slots).map_err(|e| AppError::Llm(format!("serialize slots: {e}")))
}

fn postprocess_agent(raw_json: &str, raw_input: &str) -> AppResult<ProfileOutput> {
    let slots: SlotExtraction = serde_json::from_str(raw_json.trim())
        .map_err(|e| AppError::Llm(format!("parse LLM JSON failed: {e}")))?;
    let slots = slots.normalize_with_raw(raw_input);
    Ok(ProfileOutput {
        markdown: render_markdown(&slots),
        slots: slots_value(&slots)?,
    })
}

fn postprocess_email(raw_json: &str, raw_input: &str) -> AppResult<ProfileOutput> {
    let slots: EmailExtraction = serde_json::from_str(raw_json.trim())
        .map_err(|e| AppError::Llm(format!("parse LLM JSON failed: {e}")))?;
    let slots = slots.normalize_with_raw(raw_input);
    Ok(ProfileOutput {
        markdown: render_email(&slots),
        slots: slots_value(&slots)?,
    })
}

fn postprocess_notes(raw_json: &str, raw_input: &str) -> AppResult<ProfileOutput> {
    let slots: NotesExtraction = serde_json::from_str(raw_json.trim())
        .map_err(|e| AppError::Llm(format!("parse LLM JSON failed: {e}")))?;
    let slots = slots.normalize_with_raw(raw_input);
    Ok(ProfileOutput {
        markdown: render_notes(&slots),
        slots: slots_value(&slots)?,
    })
}
