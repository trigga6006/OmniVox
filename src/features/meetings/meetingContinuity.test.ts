import { describe, expect, it } from "vitest";
import type { Meeting } from "@/lib/tauri";
import { findPreviousMeeting, normalizeMeetingTitle } from "./meetingContinuity";

function meeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    id: "meeting-1",
    title: "Weekly Product Sync",
    status: "ready",
    source_app: null,
    started_at: "2026-08-01T12:00:00Z",
    ended_at: "2026-08-01T13:00:00Z",
    user_notes: "",
    summary_markdown: "",
    summary_json: null,
    summary_provider: null,
    summary_model: null,
    summary_cost: null,
    prompt_tokens: null,
    completion_tokens: null,
    is_favorite: false,
    tags: [],
    completed_actions: [],
    summary_stale: false,
    error: null,
    deleted_at: null,
    created_at: "2026-08-01T12:00:00Z",
    updated_at: "2026-08-01T13:00:00Z",
    ai_model_override: null,
    summary_preset_override: null,
    summary_instructions: "",
    agenda: "Review delivery",
    participants: ["Alex"],
    previous_meeting_id: null,
    actions_materialized: true,
    ...overrides,
  };
}

describe("meeting continuity", () => {
  it("matches recurring titles despite harmless punctuation and casing", () => {
    expect(normalizeMeetingTitle("  Weekly: PRODUCT sync! ")).toBe("weekly product sync");
    const older = meeting();
    const newer = meeting({ id: "meeting-2", title: "weekly product sync", started_at: "2026-08-08T12:00:00Z" });
    expect(findPreviousMeeting([older, newer], "Weekly — Product Sync")?.id).toBe("meeting-2");
  });

  it("does not match an unrelated meeting or a deleted meeting", () => {
    expect(findPreviousMeeting([meeting()], "Customer review")).toBeNull();
    expect(findPreviousMeeting([meeting({ deleted_at: "2026-08-02T00:00:00Z" })], "Weekly Product Sync")).toBeNull();
  });

});
