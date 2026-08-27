// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  dismissMeetingWidget: vi.fn(() => Promise.resolve()),
  openMeetingDrawer: vi.fn(() => Promise.resolve()),
}));

vi.mock("@/lib/tauri", () => ({
  dismissMeetingWidget: mocks.dismissMeetingWidget,
  meetingPause: vi.fn(() => Promise.resolve()),
  meetingResume: vi.fn(() => Promise.resolve()),
  meetingStop: vi.fn(() => Promise.resolve()),
  openMeetingDrawer: mocks.openMeetingDrawer,
}));

vi.mock("./useMeetingRuntime", () => ({
  formatMeetingDuration: () => "2:14",
  useMeetingRuntime: () => ({
    runtime: {
      meeting_id: null,
      status: "idle",
      elapsed_ms: 134_000,
      mic_level: 0,
      system_level: 0,
      mic_signal_detected: false,
      system_signal_detected: false,
      capture_warning: null,
    },
  }),
}));

import { MeetingWidget } from "./MeetingWidget";

afterEach(() => {
  cleanup();
  mocks.dismissMeetingWidget.mockClear();
  mocks.openMeetingDrawer.mockClear();
});

describe("MeetingWidget", () => {
  it("can always be dismissed even when no meeting remains", () => {
    render(<MeetingWidget />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss meeting widget" }));
    expect(mocks.dismissMeetingWidget).toHaveBeenCalledOnce();
  });

  it("dismisses an orphan instead of opening an empty notes drawer", () => {
    render(<MeetingWidget />);
    fireEvent.click(screen.getByRole("button", { name: "Dismiss inactive meeting widget" }));
    expect(mocks.dismissMeetingWidget).toHaveBeenCalledOnce();
    expect(mocks.openMeetingDrawer).not.toHaveBeenCalled();
  });
});
