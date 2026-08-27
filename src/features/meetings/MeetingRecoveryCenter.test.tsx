// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { MeetingRecoveryItem } from "@/lib/tauri";
import { MeetingRecoveryCenter, recoveryPresentation } from "./MeetingRecoveryCenter";

const recoveryItem = (overrides: Partial<MeetingRecoveryItem> = {}): MeetingRecoveryItem => ({
  meeting_id: "meeting-1",
  title: "Product sync",
  status: "interrupted",
  pending_chunks: 0,
  processing_chunks: 0,
  failed_chunks: 0,
  recoverable_failed_chunks: 0,
  completed_chunks: 2,
  has_summary: false,
  error: null,
  updated_at: "2026-08-12T12:30:00Z",
  ...overrides,
});

afterEach(cleanup);

describe("MeetingRecoveryCenter", () => {
  it("distinguishes automatic local recovery from explicit retry work", () => {
    expect(recoveryPresentation(recoveryItem({ status: "transcribing", pending_chunks: 2 }))).toMatchObject({
      kind: "working",
      title: "Recovering transcript automatically",
    });
    expect(recoveryPresentation(recoveryItem({ status: "error", failed_chunks: 1, recoverable_failed_chunks: 1 }))).toMatchObject({
      kind: "retry",
      title: "Captured audio needs another transcription attempt",
    });
  });

  it("does not offer an automatic paid-summary retry after interruption", () => {
    const presentation = recoveryPresentation(recoveryItem({ has_summary: true }));
    expect(presentation.kind).toBe("review");
    expect(presentation.detail).toContain("regenerate and spend again");

    render(<MeetingRecoveryCenter items={[recoveryItem({ has_summary: true })]} busyId={null} onRetry={vi.fn()} onRetryAll={vi.fn()} onOpenMeeting={vi.fn()} />);
    expect(screen.queryByRole("button", { name: /retry/i })).toBeNull();
    expect(screen.getByText(/Paid AI work always waits for your explicit retry/i)).toBeTruthy();
  });

  it("retries preserved audio and opens the affected meeting", () => {
    const item = recoveryItem({ status: "error", failed_chunks: 1, recoverable_failed_chunks: 1 });
    const onRetry = vi.fn();
    const onOpenMeeting = vi.fn();
    render(<MeetingRecoveryCenter items={[item]} busyId={null} onRetry={onRetry} onRetryAll={vi.fn()} onOpenMeeting={onOpenMeeting} />);

    fireEvent.click(screen.getByRole("button", { name: "Retry 1" }));
    expect(onRetry).toHaveBeenCalledWith(item);
    fireEvent.click(screen.getByRole("button", { name: "Review meeting" }));
    expect(onOpenMeeting).toHaveBeenCalledWith("meeting-1");
  });

  it("offers a batch retry only when multiple meetings have recoverable audio", () => {
    const onRetryAll = vi.fn();
    render(<MeetingRecoveryCenter items={[
      recoveryItem({ meeting_id: "one", status: "error", failed_chunks: 1, recoverable_failed_chunks: 1 }),
      recoveryItem({ meeting_id: "two", title: "Customer call", status: "failed", failed_chunks: 2, recoverable_failed_chunks: 2 }),
    ]} busyId={null} onRetry={vi.fn()} onRetryAll={onRetryAll} onOpenMeeting={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "Retry all local audio" }));
    expect(onRetryAll).toHaveBeenCalledOnce();
  });
});
