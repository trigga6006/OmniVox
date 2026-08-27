// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Meeting } from "@/lib/tauri";

const mocks = vi.hoisted(() => ({
  listMeetings: vi.fn(),
  listMeetingActionItems: vi.fn(),
  listMeetingRecoveryItems: vi.fn(),
  retryMeetingTranscription: vi.fn(),
  meetingStart: vi.fn(),
  updateMeetingMetadata: vi.fn(),
  addMeetingActionItem: vi.fn(),
  updateMeetingAiOptions: vi.fn(),
  testMeetingAudio: vi.fn(),
  getAudioDevices: vi.fn(),
  getSelectedAudioDevice: vi.fn(),
  setAudioDevice: vi.fn(),
  listMeetingTemplates: vi.fn(),
  deleteMeetingTemplate: vi.fn(),
  setMeetingActionItemCompleted: vi.fn(),
  meetingWorkspaceProps: vi.fn(),
  listDeletedMeetings: vi.fn(),
  permanentlyDeleteMeeting: vi.fn(),
}));

vi.mock("@/lib/tauri", () => ({
  listMeetings: mocks.listMeetings,
  listMeetingActionItems: mocks.listMeetingActionItems,
  listMeetingRecoveryItems: mocks.listMeetingRecoveryItems,
  retryMeetingTranscription: mocks.retryMeetingTranscription,
  listMeetingTemplates: mocks.listMeetingTemplates,
  deleteMeetingTemplate: mocks.deleteMeetingTemplate,
  listDeletedMeetings: mocks.listDeletedMeetings,
  searchMeetings: vi.fn(() => Promise.resolve([])),
  searchDeletedMeetings: vi.fn(() => Promise.resolve([])),
  meetingStart: mocks.meetingStart,
  updateMeetingMetadata: mocks.updateMeetingMetadata,
  addMeetingActionItem: mocks.addMeetingActionItem,
  updateMeetingAiOptions: mocks.updateMeetingAiOptions,
  testMeetingAudio: mocks.testMeetingAudio,
  getAudioDevices: mocks.getAudioDevices,
  getSelectedAudioDevice: mocks.getSelectedAudioDevice,
  setAudioDevice: mocks.setAudioDevice,
  onMeetingUpdated: vi.fn(() => Promise.resolve(() => {})),
  openMeetingDrawer: vi.fn(() => Promise.resolve()),
  permanentlyDeleteMeeting: mocks.permanentlyDeleteMeeting,
  restoreMeeting: vi.fn(() => Promise.resolve()),
  setMeetingFavorite: vi.fn(() => Promise.resolve()),
  setMeetingActionCompleted: vi.fn(() => Promise.resolve([])),
  setMeetingActionItemCompleted: mocks.setMeetingActionItemCompleted,
}));

vi.mock("./useMeetingRuntime", () => ({
  useMeetingRuntime: () => ({ runtime: { meeting_id: null, status: "idle", elapsed_ms: 0 } }),
}));
vi.mock("./MeetingWorkspace", () => ({ MeetingWorkspace: (props: Record<string, unknown>) => { mocks.meetingWorkspaceProps(props); return <div>Meeting workspace</div>; } }));
vi.mock("./OpenRouterPanel", () => ({ OpenRouterPanel: () => <div>OpenRouter settings</div> }));

import { defaultMeetingTitle, MeetingsPage } from "./MeetingsPage";

// jsdom has no scrollIntoView; the kit Select calls it to keep the active
// option in view. Stub it so opening a Select doesn't throw in tests.
Element.prototype.scrollIntoView ??= () => {};

/** The kit Select is a listbox popover, not a native <select>: open, then pick. */
function pickOption(triggerLabel: string, optionName: string | RegExp) {
  fireEvent.click(screen.getByLabelText(triggerLabel));
  fireEvent.click(screen.getByRole("option", { name: optionName }));
}

function meeting(overrides: Partial<Meeting> = {}): Meeting {
  return {
    id: "previous-1",
    title: "Weekly Product Sync",
    status: "ready",
    source_app: "Google Meet",
    started_at: "2026-08-05T12:00:00Z",
    ended_at: "2026-08-05T13:00:00Z",
    user_notes: "",
    summary_markdown: "# Notes",
    summary_json: JSON.stringify({ action_items: [
      { task: "Send launch brief", owner: "Alex", due: "Friday", segment_refs: [] },
      { task: "Completed task", owner: null, due: null, segment_refs: [] },
    ] }),
    summary_provider: "OpenRouter",
    summary_model: "test/value",
    summary_cost: 0.002,
    prompt_tokens: 1_000,
    completion_tokens: 200,
    is_favorite: false,
    tags: [],
    completed_actions: ["Completed task"],
    summary_stale: false,
    error: null,
    deleted_at: null,
    created_at: "2026-08-05T12:00:00Z",
    updated_at: "2026-08-05T13:00:00Z",
    ai_model_override: "test/value",
    summary_preset_override: "standup",
    summary_instructions: "Put blockers first.",
    agenda: "Review launch readiness",
    participants: ["Alex", "Jordan"],
    previous_meeting_id: null,
    actions_materialized: false,
    ...overrides,
  };
}

describe("MeetingsPage recurring meeting continuity", () => {
  beforeEach(() => {
    const previous = meeting();
    mocks.listMeetings.mockReset().mockResolvedValue([previous]);
    mocks.listMeetingActionItems.mockReset().mockResolvedValue([]);
    mocks.listMeetingRecoveryItems.mockReset().mockResolvedValue([]);
    mocks.retryMeetingTranscription.mockReset().mockResolvedValue(undefined);
    mocks.meetingStart.mockReset().mockResolvedValue(meeting({
      id: "current-1",
      started_at: "2026-08-12T12:00:00Z",
      ended_at: null,
      status: "recording",
      previous_meeting_id: previous.id,
    }));
    mocks.updateMeetingMetadata.mockReset().mockResolvedValue(["Alex", "Jordan"]);
    mocks.addMeetingActionItem.mockReset().mockResolvedValue({});
    mocks.updateMeetingAiOptions.mockReset().mockResolvedValue(undefined);
    mocks.testMeetingAudio.mockReset().mockResolvedValue({
      microphone: { available: true, signal_detected: true, peak_level: 0.42, error: null },
      system_audio: { available: true, signal_detected: false, peak_level: 0, error: null },
    });
    mocks.getAudioDevices.mockReset().mockResolvedValue([
      { id: "mic-default", name: "Laptop microphone", is_default: true, sample_rate: 48_000, channels: 2 },
      { id: "mic-headset", name: "USB headset", is_default: false, sample_rate: 48_000, channels: 1 },
    ]);
    mocks.getSelectedAudioDevice.mockReset().mockResolvedValue("mic-headset");
    mocks.setAudioDevice.mockReset().mockResolvedValue(undefined);
    mocks.listMeetingTemplates.mockReset().mockResolvedValue([]);
    mocks.deleteMeetingTemplate.mockReset().mockResolvedValue(undefined);
    mocks.setMeetingActionItemCompleted.mockReset();
    mocks.meetingWorkspaceProps.mockReset();
    mocks.listDeletedMeetings.mockReset().mockResolvedValue([]);
    mocks.permanentlyDeleteMeeting.mockReset().mockResolvedValue(undefined);
  });

  afterEach(cleanup);

  it("renders the workspace full-bleed instead of nesting it in a page card", async () => {
    const { container } = render(<MeetingsPage />);
    await screen.findByText("Meeting workspace");

    // The hero header is gone: a slim bar with a compact title, no subtitle.
    expect(screen.getByRole("heading", { level: 1, name: "Meetings" })).toBeTruthy();
    expect(screen.queryByText(/Local capture and transcription/)).toBeNull();
    // And the master-detail split is structural — no rounded wrapper card, and
    // no separate "Your meetings" card header above the list.
    expect(container.querySelector(".rounded-2xl")).toBeNull();
    expect(screen.queryByText("Your meetings")).toBeNull();
    expect(screen.getByLabelText("Search meetings")).toBeTruthy();
  });

  it("starts a meeting in one tap without opening the options dialog", async () => {
    const started = meeting({ id: "current-1", status: "recording", ended_at: null, started_at: "2026-08-12T12:00:00Z" });
    mocks.listMeetings.mockResolvedValue([started, meeting()]);
    mocks.meetingStart.mockResolvedValue(started);
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start meeting" }));

    await waitFor(() => expect(mocks.meetingStart).toHaveBeenCalledWith(defaultMeetingTitle()));
    expect(screen.queryByPlaceholderText("Weekly product sync")).toBeNull();
    expect(mocks.updateMeetingMetadata).not.toHaveBeenCalled();
    await waitFor(() => expect(mocks.meetingWorkspaceProps.mock.calls.some(([props]) => props.meetingId === "current-1")).toBe(true));
  });

  it("recognizes a recurring title and carries selected local context into the new meeting", async () => {
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start with options" }));
    const title = await screen.findByPlaceholderText("Weekly product sync");
    fireEvent.change(title, { target: { value: "weekly — product sync" } });

    const previous = await screen.findByLabelText("Previous meeting");
    await waitFor(() => expect(previous.textContent).toContain("Weekly Product Sync"));
    expect(screen.getByText("Carry 1 unfinished follow-up as editable tasks")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));

    await waitFor(() => expect(mocks.meetingStart).toHaveBeenCalledWith(
      "weekly — product sync",
      undefined,
      "previous-1",
    ));
    expect(mocks.updateMeetingMetadata).toHaveBeenCalledWith(
      "current-1",
      "Review launch readiness",
      ["Alex", "Jordan"],
    );
    expect(mocks.addMeetingActionItem).toHaveBeenCalledWith(
      "current-1",
      "Send launch brief",
      "Alex",
      "Friday",
    );
    expect(mocks.updateMeetingAiOptions).toHaveBeenCalledWith(
      "current-1",
      "test/value",
      "standup",
      "Put blockers first.",
    );
  });

  it("uses durable action IDs in the cross-meeting follow-up queue", async () => {
    const action = {
      id: "action-1", meeting_id: "previous-1", task: "Share meeting notes", owner: "Alex", due: "Friday",
      segment_refs: [], origin: "manual", completed: false,
      created_at: "2026-08-05T13:00:00Z", updated_at: "2026-08-05T13:00:00Z",
    };
    mocks.listMeetings.mockResolvedValue([meeting({ summary_json: null, completed_actions: [] })]);
    mocks.listMeetingActionItems.mockResolvedValue([action]);
    mocks.setMeetingActionItemCompleted.mockResolvedValue({ ...action, completed: true });
    render(<MeetingsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Follow-ups · 1" }));
    fireEvent.click(await screen.findByRole("button", { name: "Complete Share meeting notes" }));
    await waitFor(() => expect(mocks.setMeetingActionItemCompleted).toHaveBeenCalledWith("action-1", "previous-1", true));
  });

  it("carries a follow-up timestamp through to the source meeting workspace", async () => {
    const action = {
      id: "action-1", meeting_id: "previous-1", task: "Review launch decision", owner: "Alex", due: "2026-08-15",
      segment_refs: ["[00:12:34]"], origin: "ai", completed: false,
      created_at: "2026-08-05T13:00:00Z", updated_at: "2026-08-05T13:00:00Z",
    };
    mocks.listMeetings.mockResolvedValue([meeting({ summary_json: null, completed_actions: [] })]);
    mocks.listMeetingActionItems.mockResolvedValue([action]);
    render(<MeetingsPage />);

    fireEvent.click(await screen.findByRole("button", { name: /Follow-ups/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Open Weekly Product Sync at 00:12:34" }));
    await waitFor(() => expect(mocks.meetingWorkspaceProps.mock.calls.some(([props]) =>
      props.meetingId === "previous-1" && props.initialTranscriptReference === "[00:12:34]",
    )).toBe(true));
  });

  it("surfaces recoverable meeting audio and retries it from inside the app", async () => {
    mocks.listMeetingRecoveryItems.mockResolvedValue([{
      meeting_id: "previous-1",
      title: "Weekly Product Sync",
      status: "error",
      pending_chunks: 0,
      processing_chunks: 0,
      failed_chunks: 1,
      recoverable_failed_chunks: 1,
      completed_chunks: 2,
      has_summary: false,
      error: "temporary model failure",
      updated_at: "2026-08-12T12:30:00Z",
    }]);
    render(<MeetingsPage />);

    fireEvent.click(await screen.findByRole("button", { name: "Recovery · 1" }));
    fireEvent.click(await screen.findByRole("button", { name: "Retry 1" }));
    await waitFor(() => expect(mocks.retryMeetingTranscription).toHaveBeenCalledWith("previous-1"));
  });

  it("tests microphone and system-loopback readiness before recording", async () => {
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start with options" }));
    fireEvent.click(await screen.findByRole("button", { name: "Test meeting audio" }));

    expect(await screen.findByText("Signal detected")).toBeTruthy();
    // Twice: once as the microphone Select's trigger label, once as the readiness card title.
    expect(screen.getAllByText("USB headset").length).toBe(2);
    expect(screen.getByText("Windows default output")).toBeTruthy();
    expect(screen.getByText("Available · no signal yet")).toBeTruthy();
    expect(screen.getByText("Capture path is working.")).toBeTruthy();
    expect(screen.getByText("Play any sound to verify loopback.")).toBeTruthy();
    expect(mocks.testMeetingAudio).toHaveBeenCalledTimes(1);
  });

  it("selects the meeting microphone without leaving the meeting dialog", async () => {
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start with options" }));
    const microphone = await screen.findByLabelText("Meeting microphone");
    await waitFor(() => expect(microphone.textContent).toContain("USB headset"));

    pickOption("Meeting microphone", /Laptop microphone/);
    await waitFor(() => expect(mocks.setAudioDevice).toHaveBeenCalledWith("mic-default"));
  });

  it("starts from a reusable setup and applies its meeting-specific AI options", async () => {
    mocks.listMeetingTemplates.mockResolvedValue([{
      id: "template-1",
      name: "Customer discovery",
      title: "Customer call",
      agenda: "Learn renewal blockers",
      participants: ["Morgan", "Alex"],
      ai_model_override: "anthropic/claude-haiku-4.5",
      summary_preset_override: "sales",
      summary_instructions: "Lead with objections.",
      created_at: "2026-08-12T12:00:00Z",
      updated_at: "2026-08-12T12:00:00Z",
    }]);
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start with options" }));
    await screen.findByLabelText("Meeting template");
    pickOption("Meeting template", "Customer discovery");

    expect((screen.getByPlaceholderText("Weekly product sync") as HTMLInputElement).value).toBe("Customer call");
    expect((screen.getByPlaceholderText("Review launch readiness and unblock pricing") as HTMLInputElement).value).toBe("Learn renewal blockers");
    expect((screen.getByPlaceholderText("Alex, Jordan, Casey") as HTMLInputElement).value).toBe("Morgan, Alex");
    fireEvent.click(screen.getByRole("button", { name: "Start recording" }));

    await waitFor(() => expect(mocks.meetingStart).toHaveBeenCalledWith("Customer call", undefined, undefined));
    expect(mocks.updateMeetingMetadata).toHaveBeenCalledWith("current-1", "Learn renewal blockers", ["Morgan", "Alex"]);
    expect(mocks.updateMeetingAiOptions).toHaveBeenCalledWith(
      "current-1",
      "anthropic/claude-haiku-4.5",
      "sales",
      "Lead with objections.",
    );
  });

  /** Escape used to travel from the open dropdown all the way to the Modal's
   *  window listener, closing both and discarding everything typed into the
   *  dialog. It must close the dropdown only — and still close the dialog once
   *  the dropdown is shut. */
  it("closes only the open dropdown on Escape, keeping the start dialog and its input", async () => {
    mocks.listMeetingTemplates.mockResolvedValue([{
      id: "template-1",
      name: "Customer discovery",
      title: "Customer call",
      agenda: "Learn renewal blockers",
      participants: ["Morgan"],
      ai_model_override: null,
      summary_preset_override: null,
      summary_instructions: "",
      created_at: "2026-08-12T12:00:00Z",
      updated_at: "2026-08-12T12:00:00Z",
    }]);
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start with options" }));
    fireEvent.change(await screen.findByPlaceholderText("Weekly product sync"), {
      target: { value: "Quarterly review" },
    });

    const trigger = await screen.findByLabelText("Meeting template");
    fireEvent.click(trigger);
    expect(screen.getByRole("listbox")).toBeTruthy();

    fireEvent.keyDown(trigger, { key: "Escape" });
    expect(screen.queryByRole("listbox")).toBeNull();
    expect((screen.getByPlaceholderText("Weekly product sync") as HTMLInputElement).value)
      .toBe("Quarterly review");

    // Dropdown closed — the next Escape belongs to the dialog again.
    fireEvent.keyDown(trigger, { key: "Escape" });
    await waitFor(() => expect(screen.queryByPlaceholderText("Weekly product sync")).toBeNull());
  });

  /** Destructive + previously untested: the trash button sits next to the
   *  template Select, so the dialog is the only thing between a mis-click and
   *  a deleted reusable setup. It is also the ConfirmDialog that stacks on top
   *  of the "Start with options" Modal. */
  it("deletes a reusable setup only after the confirm dialog is accepted", async () => {
    mocks.listMeetingTemplates.mockResolvedValue([{
      id: "template-1",
      name: "Customer discovery",
      title: "Customer call",
      agenda: "Learn renewal blockers",
      participants: ["Morgan", "Alex"],
      ai_model_override: null,
      summary_preset_override: null,
      summary_instructions: "",
      created_at: "2026-08-12T12:00:00Z",
      updated_at: "2026-08-12T12:00:00Z",
    }]);
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Start with options" }));
    await screen.findByLabelText("Meeting template");
    pickOption("Meeting template", "Customer discovery");

    fireEvent.click(screen.getByRole("button", { name: "Delete selected meeting template" }));
    expect(await screen.findByText("Delete meeting template")).toBeTruthy();
    expect(mocks.deleteMeetingTemplate).not.toHaveBeenCalled();

    // Escape dismisses the STACKED confirm only. It used to reach the modal
    // underneath as well and close the start dialog with it.
    fireEvent.keyDown(document.body, { key: "Escape" });
    await waitFor(() => expect(screen.queryByText("Delete meeting template")).toBeNull());
    expect(screen.getByPlaceholderText("Weekly product sync")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Delete selected meeting template" }));
    await screen.findByText("Delete meeting template");
    fireEvent.click(screen.getByRole("button", { name: "Delete template" }));
    await waitFor(() => expect(mocks.deleteMeetingTemplate).toHaveBeenCalledWith("template-1"));
    // The outer "Start with options" dialog survives — the confirm was stacked
    // on top of it, not instead of it.
    expect(screen.getByPlaceholderText("Weekly product sync")).toBeTruthy();
  });

  /** Destructive + previously untested: this is the one delete with no undo. */
  it("permanently deletes a trashed meeting only after the confirm dialog is accepted", async () => {
    mocks.listDeletedMeetings.mockResolvedValue([
      meeting({ id: "trashed-1", title: "Old sync", deleted_at: "2026-08-11T09:00:00Z" }),
    ]);
    render(<MeetingsPage />);
    fireEvent.click(await screen.findByRole("button", { name: "Trash" }));

    fireEvent.click(await screen.findByRole("button", { name: "Delete forever" }));
    expect(await screen.findByText("Delete this meeting permanently")).toBeTruthy();
    expect(mocks.permanentlyDeleteMeeting).not.toHaveBeenCalled();

    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    await waitFor(() => expect(mocks.permanentlyDeleteMeeting).toHaveBeenCalledWith("trashed-1"));
  });
});
