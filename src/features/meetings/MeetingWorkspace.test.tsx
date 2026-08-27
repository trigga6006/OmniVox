// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getMeeting: vi.fn(),
  setMeetingActionCompleted: vi.fn(),
  addMeetingActionItem: vi.fn(),
  updateMeetingActionItem: vi.fn(),
  setMeetingActionItemCompleted: vi.fn(),
  deleteMeetingActionItem: vi.fn(),
  updateMeetingSegment: vi.fn(),
  updateMeetingSegmentSpeaker: vi.fn(),
  retryMeetingTranscription: vi.fn(),
  estimateMeetingSummary: vi.fn(),
  openOpenRouterCredits: vi.fn(),
  askMeetingQuestion: vi.fn(),
  deleteMeetingQuestion: vi.fn(),
  getMeetingProviderSettings: vi.fn(),
  listOpenRouterModels: vi.fn(),
  updateMeetingAiOptions: vi.fn(),
  updateMeetingMetadata: vi.fn(),
  exportMeeting: vi.fn(),
  saveMeetingAsTemplate: vi.fn(),
  updateMeetingSummary: vi.fn(),
  listMeetingSummaryVersions: vi.fn(),
  restoreMeetingSummaryVersion: vi.fn(),
  summarizeMeeting: vi.fn(),
  onMeetingUpdated: vi.fn(),
  deleteMeeting: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  getMeeting: mocks.getMeeting,
  getMeetingState: vi.fn(() => Promise.resolve({ meeting_id: null, status: "idle", elapsed_ms: 0, mic_level: 0, system_level: 0, mic_signal_detected: false, system_signal_detected: false, capture_warning: null })),
  onMeetingState: vi.fn(() => Promise.resolve(() => {})),
  onMeetingUpdated: mocks.onMeetingUpdated,
  setMeetingActionCompleted: mocks.setMeetingActionCompleted,
  addMeetingActionItem: mocks.addMeetingActionItem,
  updateMeetingActionItem: mocks.updateMeetingActionItem,
  setMeetingActionItemCompleted: mocks.setMeetingActionItemCompleted,
  deleteMeetingActionItem: mocks.deleteMeetingActionItem,
  updateMeetingSegment: mocks.updateMeetingSegment,
  updateMeetingSegmentSpeaker: mocks.updateMeetingSegmentSpeaker,
  retryMeetingTranscription: mocks.retryMeetingTranscription,
  estimateMeetingSummary: mocks.estimateMeetingSummary,
  openOpenRouterCredits: mocks.openOpenRouterCredits,
  askMeetingQuestion: mocks.askMeetingQuestion,
  deleteMeetingQuestion: mocks.deleteMeetingQuestion,
  getMeetingProviderSettings: mocks.getMeetingProviderSettings,
  listOpenRouterModels: mocks.listOpenRouterModels,
  updateMeetingAiOptions: mocks.updateMeetingAiOptions,
  updateMeetingMetadata: mocks.updateMeetingMetadata,
  updateMeetingNotes: vi.fn(() => Promise.resolve()),
  updateMeetingSummary: mocks.updateMeetingSummary,
  listMeetingSummaryVersions: mocks.listMeetingSummaryVersions,
  restoreMeetingSummaryVersion: mocks.restoreMeetingSummaryVersion,
  updateMeetingTitle: vi.fn(() => Promise.resolve()),
  updateMeetingMarker: vi.fn(() => Promise.resolve()),
  deleteMeetingMarker: vi.fn(() => Promise.resolve()),
  addMeetingMarker: vi.fn(() => Promise.resolve()),
  deleteMeeting: mocks.deleteMeeting,
  exportMeeting: mocks.exportMeeting,
  saveMeetingAsTemplate: mocks.saveMeetingAsTemplate,
  revealMeetingExport: vi.fn(() => Promise.resolve()),
  setMeetingFavorite: vi.fn(() => Promise.resolve()),
  setMeetingTags: vi.fn(() => Promise.resolve([])),
  summarizeMeeting: mocks.summarizeMeeting,
  meetingPause: vi.fn(() => Promise.resolve()),
  meetingResume: vi.fn(() => Promise.resolve()),
  meetingStop: vi.fn(() => Promise.resolve()),
}));

import { localActionDueDate, MeetingWorkspace } from "./MeetingWorkspace";
import type { MeetingDetail } from "@/lib/tauri";

// jsdom has no scrollIntoView; the kit Select calls it to keep the active
// option in view. Stub it so opening a Select doesn't throw in tests.
Element.prototype.scrollIntoView ??= () => {};

const summary = {
  overview: "The launch remains on track.",
  topics: [{ title: "Launch", details: "Thursday is the target.", segment_refs: ["[00:00:01]"] }],
  decisions: [{ text: "Ship Thursday.", segment_refs: ["[00:00:01]"] }],
  action_items: [{ task: "Send launch brief", owner: "Alex", due: "Friday", segment_refs: ["[00:00:01]"] }],
  open_questions: ["Who owns support coverage?"],
  highlights: ["The beta met its target."],
};

const detail: MeetingDetail = {
  id: "meeting-1",
  title: "Product sync",
  status: "ready",
  source_app: "Google Meet",
  started_at: "2026-08-12T12:00:00Z",
  ended_at: "2026-08-12T12:30:00Z",
  user_notes: "Focus on launch risk.",
  summary_markdown: "# Meeting summary",
  summary_json: JSON.stringify(summary),
  summary_provider: "OpenRouter",
  summary_model: "deepseek/deepseek-v4-flash",
  summary_cost: 0.004,
  prompt_tokens: 1200,
  completion_tokens: 300,
  is_favorite: false,
  tags: ["launch"],
  completed_actions: [],
  summary_stale: false,
  error: null,
  deleted_at: null,
  created_at: "2026-08-12T12:00:00Z",
  updated_at: "2026-08-12T12:30:00Z",
  ai_model_override: null,
  summary_preset_override: null,
  summary_instructions: "",
  agenda: "Review launch readiness",
  participants: ["Alex", "Jordan"],
  previous_meeting_id: null,
  actions_materialized: true,
  markers: [],
  action_items: [{
    id: "action-1", meeting_id: "meeting-1", task: "Send launch brief", owner: "Alex", due: "Friday",
    segment_refs: ["[00:00:01]"], origin: "ai", completed: false,
    created_at: "2026-08-12T12:30:00Z", updated_at: "2026-08-12T12:30:00Z",
  }],
  questions: [],
  transcription: { total: 4, completed: 4, pending: 0, processing: 0, failed: 0 },
  segments: [{ id: "segment-1", meeting_id: "meeting-1", source: "system", start_ms: 1234, end_ms: 3234, text: "Ship on Tuesday.", speaker_label: null, created_at: "2026-08-12T12:00:02Z" }],
};

/** Meeting that has stopped but has no AI notes yet. */
function pending(status: MeetingDetail["status"], overrides: Partial<MeetingDetail> = {}): MeetingDetail {
  return { ...detail, status, summary_markdown: "", summary_json: null, ...overrides };
}

describe("MeetingWorkspace", () => {
  let clipboard: { writeText: ReturnType<typeof vi.fn> };
  let updatedHandlers: Array<(meetingId: string) => void>;

  beforeEach(() => {
    updatedHandlers = [];
    clipboard = { writeText: vi.fn(() => Promise.resolve()) };
    Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true });
    mocks.summarizeMeeting.mockReset().mockResolvedValue(undefined);
    mocks.onMeetingUpdated.mockReset().mockImplementation((callback: (meetingId: string) => void) => {
      updatedHandlers.push(callback);
      return Promise.resolve(() => {});
    });
    mocks.getMeeting.mockReset().mockResolvedValue(detail);
    mocks.setMeetingActionCompleted.mockReset().mockResolvedValue(["Send launch brief"]);
    mocks.addMeetingActionItem.mockReset();
    mocks.updateMeetingActionItem.mockReset();
    mocks.setMeetingActionItemCompleted.mockReset();
    mocks.deleteMeetingActionItem.mockReset().mockResolvedValue(undefined);
    mocks.setMeetingActionItemCompleted.mockResolvedValue({ ...detail.action_items[0], completed: true });
    mocks.updateMeetingActionItem.mockResolvedValue({ ...detail.action_items[0], task: "Send the final launch brief", owner: "Jordan", due: "Monday", origin: "manual", completed: true });
    mocks.addMeetingActionItem.mockResolvedValue({ ...detail.action_items[0], id: "action-2", task: "Share meeting notes", owner: null, due: null, origin: "manual" });
    mocks.updateMeetingSegment.mockReset().mockResolvedValue(undefined);
    mocks.updateMeetingSegmentSpeaker.mockReset().mockResolvedValue(undefined);
    mocks.retryMeetingTranscription.mockReset().mockResolvedValue(1);
    mocks.estimateMeetingSummary.mockReset().mockResolvedValue({
      model: "deepseek/deepseek-v4-flash",
      model_name: "DeepSeek V4 Flash",
      estimated_cost: 0.0042,
      estimated_prompt_tokens: 3200,
      max_completion_tokens: 3000,
      required_context_tokens: 6200,
      context_length: 65536,
      prompt_price_million: 0.07,
      completion_price_million: 0.28,
      request_price: 0,
      month_spend: 0.03,
      meeting_spend: 0.004,
      monthly_budget: 2,
      per_meeting_budget: 0.05,
      allowed: true,
      blocking_reason: null,
    });
    mocks.openOpenRouterCredits.mockReset().mockResolvedValue(undefined);
    mocks.askMeetingQuestion.mockReset().mockResolvedValue({
      id: "question-1",
      meeting_id: "meeting-1",
      question: "What was decided?",
      answer: "The team decided to ship Thursday.",
      segment_refs: ["[00:00:01]"],
      provider: "OpenRouter",
      model: "deepseek/deepseek-v4-flash",
      cost: 0.001,
      prompt_tokens: 800,
      completion_tokens: 40,
      context_segments: 1,
      total_segments: 1,
      created_at: "2026-08-12T12:31:00Z",
    });
    mocks.deleteMeetingQuestion.mockReset().mockResolvedValue(undefined);
    mocks.getMeetingProviderSettings.mockReset().mockResolvedValue({
      provider: "openrouter", model: "global/model", monthly_budget: 2, per_meeting_budget: 0.05,
      max_prompt_price: 0.5, max_completion_price: 3, zdr_only: true, deny_data_collection: true,
      auto_suggest: true, auto_summarize: true, summary_preset: "general", custom_instructions: "",
      transcription_mode: "after_meeting", routing_preference: "price",
    });
    mocks.listOpenRouterModels.mockReset().mockResolvedValue([]);
    mocks.updateMeetingAiOptions.mockReset().mockResolvedValue(undefined);
    mocks.updateMeetingMetadata.mockReset().mockResolvedValue([]);
    mocks.exportMeeting.mockReset().mockResolvedValue({ path: "x", file_name: "product-sync-notes.md" });
    mocks.saveMeetingAsTemplate.mockReset().mockResolvedValue({
      id: "template-1", name: "Product sync", title: "Product sync", agenda: "Review launch readiness",
      participants: ["Alex", "Jordan"], ai_model_override: null, summary_preset_override: null,
      summary_instructions: "", created_at: "2026-08-12T12:00:00Z", updated_at: "2026-08-12T12:00:00Z",
    });
    mocks.updateMeetingSummary.mockReset().mockResolvedValue(undefined);
    mocks.listMeetingSummaryVersions.mockReset().mockResolvedValue([
      { id: "version-current", meeting_id: "meeting-1", markdown: "# Meeting summary", summary_json: JSON.stringify(summary), provider: "OpenRouter", model: "deepseek/deepseek-v4-flash", cost: 0.004, prompt_tokens: 1200, completion_tokens: 300, origin: "ai", created_at: "2026-08-12T12:30:00Z" },
      { id: "version-old", meeting_id: "meeting-1", markdown: "# Earlier notes\n\nOriginal direction.", summary_json: null, provider: "OpenRouter", model: "test/older", cost: 0.002, prompt_tokens: 800, completion_tokens: 150, origin: "manual", created_at: "2026-08-12T12:20:00Z" },
    ]);
    mocks.restoreMeetingSummaryVersion.mockReset().mockResolvedValue({ id: "version-restored", meeting_id: "meeting-1", markdown: "# Earlier notes\n\nOriginal direction.", summary_json: null, provider: "OpenRouter", model: "test/older", cost: 0.002, prompt_tokens: 800, completion_tokens: 150, origin: "restored", created_at: "2026-08-12T12:40:00Z" });
    mocks.deleteMeeting.mockReset().mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  it("creates stable local ISO due dates across month and year boundaries", () => {
    expect(localActionDueDate(1, new Date(2026, 11, 31, 23, 30))).toBe("2027-01-01");
    expect(localActionDueDate(7, new Date(2026, 7, 12, 8))).toBe("2026-08-19");
  });

  it("renders structured notes and persists completed action items", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);

    expect(await screen.findByText("The launch remains on track.")).toBeTruthy();
    const checkbox = screen.getByRole("checkbox", { name: "Complete Send launch brief" });
    fireEvent.click(checkbox);

    await waitFor(() => expect(mocks.setMeetingActionItemCompleted).toHaveBeenCalledWith("action-1", "meeting-1", true));
    expect(screen.getByText("Ship Thursday.")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Edit source" })).toBeTruthy();
  });

  it("adds, completes, edits, and removes durable follow-ups without regenerating notes", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: /Actions/ }));

    fireEvent.click(screen.getByRole("checkbox", { name: "Complete Send launch brief" }));
    await waitFor(() => expect(mocks.setMeetingActionItemCompleted).toHaveBeenCalledWith("action-1", "meeting-1", true));

    fireEvent.click(screen.getByRole("button", { name: "Edit Send launch brief" }));
    fireEvent.change(screen.getByLabelText("Action item task"), { target: { value: "Send the final launch brief" } });
    fireEvent.change(screen.getByLabelText("Action item owner"), { target: { value: "Jordan" } });
    fireEvent.change(screen.getByLabelText("Action item due date"), { target: { value: "Monday" } });
    fireEvent.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(mocks.updateMeetingActionItem).toHaveBeenCalledWith("action-1", "meeting-1", "Send the final launch brief", "Jordan", "Monday"));

    fireEvent.click(screen.getByRole("button", { name: "Delete Send the final launch brief" }));
    await waitFor(() => expect(mocks.deleteMeetingActionItem).toHaveBeenCalledWith("action-1", "meeting-1"));

    fireEvent.click(screen.getByRole("button", { name: "Add follow-up" }));
    fireEvent.change(screen.getByLabelText("Action item task"), { target: { value: "Share meeting notes" } });
    fireEvent.click(screen.getByRole("button", { name: "Create follow-up" }));
    await waitFor(() => expect(mocks.addMeetingActionItem).toHaveBeenCalledWith("meeting-1", "Share meeting notes", "", ""));
  });

  it("lets the user correct a transcript segment with the keyboard", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: /Transcript/ }));
    fireEvent.click(screen.getByRole("button", { name: "Correct transcript at 0:01" }));

    const editor = screen.getByDisplayValue("Ship on Tuesday.");
    fireEvent.change(editor, { target: { value: "Ship on Thursday." } });
    fireEvent.keyDown(editor, { key: "Enter", ctrlKey: true });

    await waitFor(() => expect(mocks.updateMeetingSegment).toHaveBeenCalledWith("segment-1", "meeting-1", "Ship on Thursday."));
  });

  it("labels one speaker without running diarization", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: /Transcript/ }));
    fireEvent.click(screen.getByRole("button", { name: "Rename speaker at 0:01" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Speaker name at 0:01" }), { target: { value: "Alex" } });
    fireEvent.click(screen.getByRole("button", { name: "This line" }));

    await waitFor(() => expect(mocks.updateMeetingSegmentSpeaker).toHaveBeenCalledWith("segment-1", "meeting-1", "Alex", false));
  });

  it("offers recovery when a captured audio chunk failed", async () => {
    mocks.getMeeting.mockResolvedValue({
      ...detail,
      status: "error",
      error: "1 audio chunk could not be transcribed",
      transcription: { total: 4, completed: 3, pending: 0, processing: 0, failed: 1 },
    });
    render(<MeetingWorkspace meetingId="meeting-1" />);

    const retry = await screen.findByRole("button", { name: "Retry 1 failed" });
    fireEvent.click(retry);

    await waitFor(() => expect(mocks.retryMeetingTranscription).toHaveBeenCalledWith("meeting-1"));
  });

  it("filters and highlights a long transcript locally", async () => {
    mocks.getMeeting.mockResolvedValue({
      ...detail,
      segments: [
        detail.segments[0],
        { ...detail.segments[0], id: "segment-2", text: "Budget review next week." },
      ],
    });
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: /Transcript/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Find in transcript" }), { target: { value: "Tuesday" } });

    expect(await screen.findByText("Tuesday", { selector: "mark" })).toBeTruthy();
    expect(screen.queryByText("Budget review next week.")).toBeNull();
    expect(screen.getByText("1 match")).toBeTruthy();
  });

  it("previews the conservative model cost before regeneration", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);

    expect(await screen.findByText("Up to $0.0042")).toBeTruthy();
    expect(mocks.estimateMeetingSummary).toHaveBeenCalledWith("meeting-1");
  });

  it("preserves live edits and restores an earlier local notes version", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: "Edit source" }));
    const editor = screen.getByDisplayValue("# Meeting summary");
    fireEvent.change(editor, { target: { value: "# Unsaved careful edit" } });
    fireEvent.click(screen.getByRole("button", { name: /Versions/ }));

    const earlier = await screen.findByText("Earlier notes");
    const article = earlier.closest("article");
    const preview = article?.querySelector<HTMLButtonElement>('button[aria-label^="Preview notes version"]');
    expect(preview).toBeTruthy();
    fireEvent.click(preview!);
    expect(screen.getByText(/Original direction/)).toBeTruthy();
    const restore = article?.querySelector<HTMLButtonElement>('button[aria-label^="Restore notes version"]');
    expect(restore).toBeTruthy();
    fireEvent.click(restore!);

    // Restoring is destructive-adjacent, so it routes through ConfirmDialog now.
    expect(await screen.findByText("Restore this notes version")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restore version" }));

    await waitFor(() => expect(mocks.updateMeetingSummary).toHaveBeenCalledWith("meeting-1", "# Unsaved careful edit"));
    await waitFor(() => expect(mocks.restoreMeetingSummaryVersion).toHaveBeenCalledWith("version-old", "meeting-1"));
    expect(await screen.findByText(/Restored an earlier notes version/)).toBeTruthy();
  });

  /** Destructive + previously untested: the dialog is the only thing standing
   *  between the menu item and the meeting leaving the list. */
  it("moves the meeting to Trash only after the confirm dialog is accepted", async () => {
    const onDeleted = vi.fn();
    render(<MeetingWorkspace meetingId="meeting-1" onDeleted={onDeleted} />);
    await screen.findByText("The launch remains on track.");

    fireEvent.click(screen.getByRole("button", { name: "Meeting actions" }));
    fireEvent.click(screen.getByText("Move to Trash"));

    // Opening the menu item alone must not delete anything.
    expect(await screen.findByText("Move this meeting to Trash")).toBeTruthy();
    expect(mocks.deleteMeeting).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Move to trash" }));
    await waitFor(() => expect(mocks.deleteMeeting).toHaveBeenCalledWith("meeting-1"));
    await waitFor(() => expect(onDeleted).toHaveBeenCalled());
  });

  it("saves AI overrides for only this meeting", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: "Meeting AI settings" }));
    const model = await screen.findByPlaceholderText("Global · global/model");
    fireEvent.change(model, { target: { value: "anthropic/claude-haiku-4.5" } });
    fireEvent.click(screen.getByRole("combobox", { name: "Summary style" }));
    fireEvent.click(screen.getByRole("option", { name: "Sales call" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Meeting-specific instructions" }), { target: { value: "Prioritize objections." } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mocks.updateMeetingAiOptions).toHaveBeenCalledWith(
      "meeting-1", "anthropic/claude-haiku-4.5", "sales", "Prioritize objections.",
    ));
  });

  it("keeps tags, agenda, and participants collapsed until the meta row is opened", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");

    // The three fields no longer occupy a permanent row each — the header shows
    // what is already set and opens the editors on demand.
    expect(screen.queryByRole("textbox", { name: "Meeting agenda" })).toBeNull();
    expect(screen.queryByRole("textbox", { name: "Meeting tags" })).toBeNull();
    expect(screen.getByText("#launch")).toBeTruthy();
    expect(screen.getByText("Review launch readiness")).toBeTruthy();
    expect(screen.getByText("Alex, Jordan")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Edit meeting details" }));
    expect(screen.getByRole("textbox", { name: "Meeting tags" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Done" }));
    expect(screen.queryByRole("textbox", { name: "Meeting tags" })).toBeNull();
  });

  it("keeps the collapsed meta row and flat notes toolbar in the drawer", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" compact />);
    await screen.findByText("The launch remains on track.");

    expect(screen.queryByRole("textbox", { name: "Meeting agenda" })).toBeNull();
    expect(screen.getByRole("button", { name: "Edit meeting details" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Copy" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Regenerate" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Meeting AI settings" })).toBeTruthy();
  });

  it("edits local agenda and participant context", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: "Edit meeting details" }));
    const agenda = screen.getByRole("textbox", { name: "Meeting agenda" });
    fireEvent.change(agenda, { target: { value: "Review pricing and launch readiness" } });
    fireEvent.blur(agenda);
    await waitFor(() => expect(mocks.updateMeetingMetadata).toHaveBeenCalledWith(
      "meeting-1", "Review pricing and launch readiness", null,
    ));

    const participants = screen.getByRole("textbox", { name: "Meeting participants" });
    fireEvent.change(participants, { target: { value: "Alex, Jordan, Casey" } });
    fireEvent.blur(participants);
    await waitFor(() => expect(mocks.updateMeetingMetadata).toHaveBeenCalledWith(
      "meeting-1", null, ["Alex", " Jordan", " Casey"],
    ));
  });

  it("opens the previous meeting in a recurring series", async () => {
    const onOpenMeeting = vi.fn();
    mocks.getMeeting.mockResolvedValue({ ...detail, previous_meeting_id: "meeting-previous" });
    render(<MeetingWorkspace meetingId="meeting-1" onOpenMeeting={onOpenMeeting} />);

    fireEvent.click(await screen.findByRole("button", { name: "Previous meeting" }));
    expect(onOpenMeeting).toHaveBeenCalledWith("meeting-previous");
  });

  it("opens directly at a referenced transcript line from the follow-up inbox", async () => {
    const consumed = vi.fn();
    const scrollIntoView = vi.fn();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", { configurable: true, value: scrollIntoView });
    render(<MeetingWorkspace meetingId="meeting-1" initialTranscriptReference="[00:00:01]" onInitialTranscriptReferenceConsumed={consumed} />);

    expect(await screen.findByRole("textbox", { name: "Find in transcript" })).toBeTruthy();
    await waitFor(() => expect(consumed).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(scrollIntoView).toHaveBeenCalled());
    expect(document.activeElement?.getAttribute("data-segment-ref")).toBe("[00:00:01]");
  });

  it("offers a direct credit recovery action for OpenRouter payment failures", async () => {
    mocks.getMeeting.mockResolvedValue({
      ...detail,
      status: "awaiting_summary",
      summary_markdown: "",
      summary_json: null,
      error: "OpenRouter has insufficient credits. Add credits, then try again.",
    });
    render(<MeetingWorkspace meetingId="meeting-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Add credits" }));
    await waitFor(() => expect(mocks.openOpenRouterCredits).toHaveBeenCalled());
  });

  it("separates shareable notes from private archive exports", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: "Meeting actions" }));

    expect(screen.getByText("No transcript, scratchpad, Q&A, or cost data")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /Export polished notes/ }));

    await waitFor(() => expect(mocks.exportMeeting).toHaveBeenCalledWith("meeting-1", "notes"));
  });

  it("saves the current meeting configuration as a reusable setup", async () => {
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("Weekly launch sync");
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: "Meeting actions" }));
    fireEvent.click(screen.getByRole("button", { name: "Save as reusable setup" }));

    await waitFor(() => expect(mocks.saveMeetingAsTemplate).toHaveBeenCalledWith("meeting-1", "Weekly launch sync"));
    expect(await screen.findByText(/Saved “Product sync” as a reusable meeting setup/)).toBeTruthy();
    prompt.mockRestore();
  });

  it("never substitutes the raw transcript for missing AI notes", async () => {
    mocks.getMeeting.mockResolvedValue({ ...detail, summary_markdown: "", summary_json: null });
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByDisplayValue("Product sync");
    fireEvent.click(screen.getByRole("button", { name: "Meeting actions" }));

    expect(screen.getByRole("button", { name: /Copy AI notes/ })).toHaveProperty("disabled", true);
  });

  it("opens a just-finished meeting on the AI notes call to action", async () => {
    mocks.getMeeting.mockResolvedValue(pending("awaiting_summary"));
    render(<MeetingWorkspace meetingId="meeting-1" />);

    const cta = await screen.findByRole("button", { name: "Generate AI notes" });
    expect(await screen.findByText(/Up to \$0.0042 · DeepSeek V4 Flash/)).toBeTruthy();
    fireEvent.click(cta);

    await waitFor(() => expect(mocks.summarizeMeeting).toHaveBeenCalledWith("meeting-1"));
  });

  it("queues the request while transcription runs and fires it when the transcript lands", async () => {
    mocks.getMeeting.mockResolvedValue(pending("transcribing", {
      transcription: { total: 4, completed: 2, pending: 2, processing: 0, failed: 0 },
    }));
    render(<MeetingWorkspace meetingId="meeting-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Generate AI notes" }));
    expect(await screen.findByText(/Will generate when transcription finishes/)).toBeTruthy();
    expect(mocks.summarizeMeeting).not.toHaveBeenCalled();

    mocks.getMeeting.mockResolvedValue(pending("awaiting_summary"));
    act(() => updatedHandlers.forEach((handler) => handler("meeting-1")));

    await waitFor(() => expect(mocks.summarizeMeeting).toHaveBeenCalledWith("meeting-1"));
  });

  it("treats an already-running generation as progress rather than an error", async () => {
    mocks.getMeeting.mockResolvedValue(pending("awaiting_summary"));
    mocks.summarizeMeeting.mockRejectedValue("AI notes are already being generated for this meeting");
    render(<MeetingWorkspace meetingId="meeting-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Generate AI notes" }));
    await waitFor(() => expect(mocks.summarizeMeeting).toHaveBeenCalledTimes(1));
    expect(screen.queryByText(/already being generated/)).toBeNull();
  });

  it("shows generation progress when a summary starts on its own", async () => {
    mocks.getMeeting.mockResolvedValue(pending("summarizing"));
    render(<MeetingWorkspace meetingId="meeting-1" />);

    expect(await screen.findByText("Writing your AI notes")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Generate AI notes" })).toBeNull();
  });

  it("blocks the call to action when the summary is outside the budget", async () => {
    mocks.getMeeting.mockResolvedValue(pending("awaiting_summary"));
    mocks.estimateMeetingSummary.mockResolvedValue({
      model: "deepseek/deepseek-v4-flash", model_name: "DeepSeek V4 Flash", estimated_cost: 4,
      estimated_prompt_tokens: 3200, max_completion_tokens: 3000, required_context_tokens: 6200,
      context_length: 65536, prompt_price_million: 0.07, completion_price_million: 0.28, request_price: 0,
      month_spend: 1.9, meeting_spend: 0, monthly_budget: 2, per_meeting_budget: 0.05,
      allowed: false, blocking_reason: "This summary would exceed the monthly budget.",
    });
    render(<MeetingWorkspace meetingId="meeting-1" />);

    expect(await screen.findByText("This summary would exceed the monthly budget.")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Generate AI notes" })).toBeNull();
  });

  it("copies the AI notes markdown and the rendered transcript", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");

    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    await waitFor(() => expect(clipboard.writeText).toHaveBeenCalledWith("# Meeting summary"));
    expect(await screen.findByRole("button", { name: "Copy" })).toHaveProperty("textContent", "Copied");

    fireEvent.click(screen.getByRole("button", { name: /Transcript/ }));
    fireEvent.click(screen.getByRole("button", { name: "Copy transcript" }));
    await waitFor(() => expect(clipboard.writeText).toHaveBeenCalledWith("[0:01] Meeting: Ship on Tuesday."));
  });

  it("asks a grounded follow-up question and keeps the answer in the meeting", async () => {
    render(<MeetingWorkspace meetingId="meeting-1" />);
    await screen.findByText("The launch remains on track.");
    fireEvent.click(screen.getByRole("button", { name: /^Ask/ }));
    fireEvent.change(screen.getByRole("textbox", { name: "Ask a question about this meeting" }), { target: { value: "What was decided?" } });
    fireEvent.click(screen.getByRole("button", { name: "Ask meeting" }));

    await waitFor(() => expect(mocks.askMeetingQuestion).toHaveBeenCalledWith("meeting-1", "What was decided?"));
    expect(await screen.findByText("The team decided to ship Thursday.")).toBeTruthy();
    expect(screen.getByText(/full transcript/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "00:00:01" }));
    expect(await screen.findByRole("textbox", { name: "Find in transcript" })).toBeTruthy();
  });
});
