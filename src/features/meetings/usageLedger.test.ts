import { describe, expect, it } from "vitest";
import type { MeetingAiUsageRecord } from "@/lib/tauri";
import { projectedMonthSpend, rollupUsageByMeeting, usageRecordsCsv } from "./usageLedger";

function record(overrides: Partial<MeetingAiUsageRecord> = {}): MeetingAiUsageRecord {
  return {
    id: "usage-1",
    meeting_id: "meeting-1",
    meeting_title: "Product sync",
    request_kind: "summary",
    provider: "OpenRouter",
    model: "test/value",
    cost: 0.01,
    prompt_tokens: 1_000,
    completion_tokens: 200,
    created_at: "2026-08-12T12:00:00Z",
    ...overrides,
  };
}

describe("meeting AI usage ledger", () => {
  it("groups requests by meeting and orders the largest spend first", () => {
    const rows = rollupUsageByMeeting([
      record(),
      record({ id: "usage-2", request_kind: "question", cost: 0.005, prompt_tokens: 500 }),
      record({ id: "usage-3", meeting_id: "meeting-2", meeting_title: "Sales call", cost: 0.02 }),
    ]);
    expect(rows.map((row) => row.meetingTitle)).toEqual(["Sales call", "Product sync"]);
    expect(rows[1]).toMatchObject({ cost: 0.015, requests: 2, promptTokens: 1_500, completionTokens: 400 });
  });

  it("keeps deleted usage auditable without merging unrelated records", () => {
    const rows = rollupUsageByMeeting([
      record({ id: "deleted-1", meeting_id: null, meeting_title: null }),
      record({ id: "deleted-2", meeting_id: null, meeting_title: null }),
    ]);
    expect(rows).toHaveLength(2);
    expect(rows.every((row) => row.meetingTitle === "Deleted meeting")).toBe(true);
  });

  it("projects a monthly run rate from elapsed time", () => {
    expect(projectedMonthSpend(1, new Date(2026, 7, 16, 0, 0))).toBeCloseTo(31 / 15);
    expect(projectedMonthSpend(0, new Date(2026, 7, 16))).toBe(0);
  });

  it("exports spreadsheet-safe CSV with quotes escaped", () => {
    const csv = usageRecordsCsv([record({ meeting_title: 'Planning, "phase 2"' })]);
    expect(csv).toContain('"Planning, ""phase 2"""');
    expect(csv).toContain('"0.01000000"');
  });
});
