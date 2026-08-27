import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: mocks.listen,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { onHistoryCleanupError, onMeetingError, onMeetingSummaryReady } from "./tauri";

describe("history cleanup event bridge", () => {
  beforeEach(() => {
    mocks.listen.mockReset();
  });

  it("subscribes to the backend error event and forwards its reason", async () => {
    const unlisten = vi.fn();
    mocks.listen.mockImplementationOnce(
      async (event: string, handler: (event: { payload: string }) => void) => {
        expect(event).toBe("history-cleanup-error");
        handler({ payload: "database is locked" });
        return unlisten;
      }
    );
    const callback = vi.fn();

    const registeredUnlisten = await onHistoryCleanupError(callback);

    expect(callback).toHaveBeenCalledOnce();
    expect(callback).toHaveBeenCalledWith("database is locked");
    expect(registeredUnlisten).toBe(unlisten);
  });

  it("degrades to a no-op listener in a plain browser preview", async () => {
    mocks.listen.mockRejectedValueOnce(new TypeError("Tauri bridge unavailable"));

    const unlisten = await onHistoryCleanupError(vi.fn());

    expect(() => unlisten()).not.toThrow();
  });

  it("forwards meeting completion and processing-error events", async () => {
    mocks.listen.mockImplementation(
      async (event: string, handler: (event: { payload: string }) => void) => {
        handler({ payload: event === "meeting-summary-ready" ? "meeting-42" : "model unavailable" });
        return vi.fn();
      },
    );
    const ready = vi.fn();
    const failed = vi.fn();

    await onMeetingSummaryReady(ready);
    await onMeetingError(failed);

    expect(ready).toHaveBeenCalledWith("meeting-42");
    expect(failed).toHaveBeenCalledWith("model unavailable");
    expect(mocks.listen.mock.calls.map(([event]) => event)).toEqual([
      "meeting-summary-ready",
      "meeting-error",
    ]);
  });
});
