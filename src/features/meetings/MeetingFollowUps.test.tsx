// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Meeting, MeetingActionItem } from "@/lib/tauri";
import {
  extractMeetingFollowUps,
  followUpDueState,
  followUpsChecklist,
  MeetingFollowUps,
  sortFollowUpsForInbox,
} from "./MeetingFollowUps";

// jsdom has no scrollIntoView; the kit Select calls it to keep the active
// option in view. Stub it so opening a Select doesn't throw in tests.
Element.prototype.scrollIntoView ??= () => {};

const meeting = (overrides: Partial<Meeting> = {}): Meeting => ({
  id: "meeting-1",
  title: "Product sync",
  status: "ready",
  source_app: "Google Meet",
  started_at: "2026-08-12T12:00:00Z",
  ended_at: "2026-08-12T12:30:00Z",
  user_notes: "",
  summary_markdown: "# Notes",
  summary_json: JSON.stringify({ action_items: [
    { task: "Send launch brief", owner: "Alex", due: "Friday", segment_refs: ["[00:01:20]"] },
    { task: "Confirm support coverage", owner: null, due: null, segment_refs: [] },
  ] }),
  summary_provider: "OpenRouter",
  summary_model: "test/model",
  summary_cost: 0.001,
  prompt_tokens: 100,
  completion_tokens: 20,
  is_favorite: false,
  tags: [],
  completed_actions: ["Confirm support coverage"],
  summary_stale: false,
  error: null,
  deleted_at: null,
  created_at: "2026-08-12T12:00:00Z",
  updated_at: "2026-08-12T12:30:00Z",
  ai_model_override: null,
  summary_preset_override: null,
  summary_instructions: "",
  agenda: "",
  participants: [],
  previous_meeting_id: null,
  actions_materialized: false,
  ...overrides,
});

afterEach(cleanup);

describe("MeetingFollowUps", () => {
  it("extracts structured actions and ignores malformed summaries", () => {
    const items = extractMeetingFollowUps([
      meeting(),
      meeting({ id: "broken", summary_json: "not json" }),
    ]);
    expect(items).toHaveLength(2);
    expect(items[0]).toMatchObject({ task: "Send launch brief", owner: "Alex", due: "Friday", completed: false });
    expect(items[1]).toMatchObject({ task: "Confirm support coverage", completed: true });
  });

  it("prefers durable actions, includes manual work, and avoids summary duplicates", () => {
    const actions: MeetingActionItem[] = [
      { id: "stored-1", meeting_id: "meeting-1", task: "Send launch brief", owner: "Jordan", due: "Monday", segment_refs: [], origin: "manual", completed: true, created_at: "2026-08-12T12:31:00Z", updated_at: "2026-08-12T12:31:00Z" },
      { id: "stored-2", meeting_id: "meeting-1", task: "Share call notes", owner: null, due: null, segment_refs: [], origin: "manual", completed: false, created_at: "2026-08-12T12:32:00Z", updated_at: "2026-08-12T12:32:00Z" },
    ];
    const items = extractMeetingFollowUps([meeting()], actions);
    expect(items).toHaveLength(3);
    expect(items.filter((item) => item.task === "Send launch brief")).toHaveLength(1);
    expect(items.find((item) => item.id === "stored-1")).toMatchObject({ owner: "Jordan", completed: true });
    expect(items.some((item) => item.task === "Share call notes")).toBe(true);
  });

  it("shows open work by default and can reveal completed history", () => {
    render(<MeetingFollowUps meetings={[meeting()]} busyKey={null} onToggle={vi.fn()} onOpenMeeting={vi.fn()} />);
    expect(screen.getByText("Send launch brief")).toBeTruthy();
    expect(screen.queryByText("Confirm support coverage")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show 1 completed" }));
    expect(screen.getByText("Confirm support coverage")).toBeTruthy();
  });

  it("prioritizes absolute due dates and classifies urgency without guessing free-form dates", () => {
    const base = extractMeetingFollowUps([meeting()]).filter((item) => !item.completed)[0];
    const items = sortFollowUpsForInbox([
      { ...base, id: "unscheduled", task: "Free-form", due: "Friday" },
      { ...base, id: "future", task: "Future", due: "2026-08-14" },
      { ...base, id: "overdue", task: "Overdue", due: "2026-08-10" },
    ], new Date(2026, 7, 12, 12));
    expect(items.map((item) => item.task)).toEqual(["Overdue", "Future", "Free-form"]);
    expect(followUpDueState("2026-08-10", new Date(2026, 7, 12, 12))).toMatchObject({ kind: "overdue" });
    expect(followUpDueState("2026-08-12", new Date(2026, 7, 12, 12))).toMatchObject({ kind: "today", label: "Due today" });
    expect(followUpDueState("Friday", new Date(2026, 7, 12, 12))).toBeNull();
    expect(followUpDueState("2026-02-30", new Date(2026, 7, 12, 12))).toBeNull();
  });

  it("creates a portable Markdown checklist with task context", () => {
    const items = extractMeetingFollowUps([meeting()]);
    expect(followUpsChecklist(items)).toBe("- [ ] Send launch brief (Owner: Alex · Due: Friday · From: Product sync)");
  });

  it("copies the currently visible open work as a checklist", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(<MeetingFollowUps meetings={[meeting()]} busyKey={null} onToggle={vi.fn()} onOpenMeeting={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Copy open · 1" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("- [ ] Send launch brief (Owner: Alex · Due: Friday · From: Product sync)"));
    expect(await screen.findByRole("button", { name: "Copied" })).toBeTruthy();
  });

  it("searches and filters the inbox by owner", async () => {
    render(<MeetingFollowUps meetings={[meeting()]} busyKey={null} onToggle={vi.fn()} onOpenMeeting={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Search follow-ups"), { target: { value: "launch" } });
    await waitFor(() => expect(screen.getByText("Send launch brief")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Filter follow-ups by owner"));
    fireEvent.click(screen.getByRole("option", { name: "Unassigned" }));
    await waitFor(() => expect(screen.getByText("No matching follow-ups")).toBeTruthy());
    fireEvent.click(screen.getByLabelText("Clear follow-up search"));
    fireEvent.click(screen.getByRole("button", { name: "Show 1 completed" }));
    await waitFor(() => expect(screen.getByText("Confirm support coverage")).toBeTruthy());
  });

  it("completes an item and opens its source meeting", () => {
    const onToggle = vi.fn();
    const onOpenMeeting = vi.fn();
    render(<MeetingFollowUps meetings={[meeting()]} busyKey={null} onToggle={onToggle} onOpenMeeting={onOpenMeeting} />);
    fireEvent.click(screen.getByRole("button", { name: "Complete Send launch brief" }));
    expect(onToggle).toHaveBeenCalledWith(expect.objectContaining({ task: "Send launch brief" }), true);
    fireEvent.click(screen.getByRole("button", { name: "Open Product sync" }));
    expect(onOpenMeeting).toHaveBeenCalledWith("meeting-1");
    fireEvent.click(screen.getByRole("button", { name: "Open Product sync at 00:01:20" }));
    expect(onOpenMeeting).toHaveBeenCalledWith("meeting-1", "[00:01:20]");
  });
});
