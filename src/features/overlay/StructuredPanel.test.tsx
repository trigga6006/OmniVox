// @vitest-environment jsdom

import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const listeners: Record<string, (...args: any[]) => void> = {};
let recordingStatus = "idle";

vi.mock("@/lib/tauri", () => ({
  pasteStructuredOutput: vi.fn(() => Promise.resolve()),
  discardStructuredOutput: vi.fn(() => Promise.resolve()),
  setStructuredPanelActive: vi.fn(() => Promise.resolve()),
  startRecording: vi.fn(() => Promise.resolve()),
  stopRecording: vi.fn(() => Promise.resolve()),
  onRecordingLifecycle: (callback: (...args: any[]) => void) => {
    listeners.lifecycle = callback;
    return Promise.resolve(() => {});
  },
  onTranscriptionCompletion: (callback: (...args: any[]) => void) => {
    listeners.completion = callback;
    return Promise.resolve(() => {});
  },
}));

vi.mock("@/stores/recordingStore", () => ({
  useRecordingStore: (selector: (state: any) => unknown) =>
    selector({ status: recordingStatus, audioLevel: 0 }),
}));

import { StructuredPanel } from "./StructuredPanel";
import {
  discardStructuredOutput,
  setStructuredPanelActive,
  type StructuredOutputPayload,
} from "@/lib/tauri";

const payload: StructuredOutputPayload = {
  generation: 1,
  binding_id: "binding-1",
  markdown: "Existing text",
  slots: {},
  raw_transcript: "Existing text",
  truncated_chars: 0,
};

describe("StructuredPanel lifecycle", () => {
  beforeEach(() => {
    recordingStatus = "idle";
    vi.mocked(discardStructuredOutput).mockClear();
    vi.mocked(setStructuredPanelActive).mockClear();
  });

  afterEach(() => {
    cleanup();
  });

  it("does not discard a pending capability while temporarily hidden or unmounted", () => {
    const onClose = vi.fn();
    const { rerender, unmount } = render(
      <StructuredPanel payload={payload} onClose={onClose} active />
    );

    rerender(
      <StructuredPanel payload={payload} onClose={onClose} active={false} />
    );
    rerender(<StructuredPanel payload={payload} onClose={onClose} active />);
    unmount();

    expect(discardStructuredOutput).not.toHaveBeenCalled();
    expect(setStructuredPanelActive).toHaveBeenCalledWith(true);
    expect(setStructuredPanelActive).toHaveBeenCalledWith(false);
  });

  it("returns to idle when a silent panel dictation has no transcription", () => {
    const { rerender } = render(
      <StructuredPanel payload={payload} onClose={vi.fn()} />
    );

    recordingStatus = "recording";
    act(() => listeners.lifecycle("recording", 7));
    rerender(<StructuredPanel payload={payload} onClose={vi.fn()} />);
    expect(screen.getByLabelText("Stop dictation")).toBeTruthy();

    recordingStatus = "processing";
    act(() => listeners.lifecycle("processing", 7));
    rerender(<StructuredPanel payload={payload} onClose={vi.fn()} />);
    expect(
      screen
        .getByRole("button", { name: "Dictate into preview" })
        .hasAttribute("disabled")
    ).toBe(true);

    recordingStatus = "idle";
    act(() => listeners.lifecycle("idle", 7));
    rerender(<StructuredPanel payload={payload} onClose={vi.fn()} />);
    expect(
      screen
        .getByRole("button", { name: "Dictate into preview" })
        .hasAttribute("disabled")
    ).toBe(false);
  });

  it("appends every overlapping generation even when completions are out of order", () => {
    render(<StructuredPanel payload={payload} onClose={vi.fn()} />);

    act(() => {
      listeners.lifecycle("recording", 7);
      listeners.lifecycle("recording", 8);
      listeners.completion("newer", 8);
    });
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe(
      "Existing text\nnewer"
    );

    act(() => listeners.completion("older", 7));
    expect((screen.getByRole("textbox") as HTMLTextAreaElement).value).toBe(
      "Existing text\nnewer\nolder"
    );
  });
});
