// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const listeners: Record<string, (payload: unknown) => void> = {};
const unlisten = vi.fn();

vi.mock("@/lib/tauri", () => {
  const subscribe = (name: string) => (callback: (payload: unknown) => void) => {
    listeners[name] = callback;
    return Promise.resolve(unlisten);
  };
  return {
    onRecordingStateChange: subscribe("recording-state-change"),
    onAudioLevel: subscribe("audio-level"),
  };
});

import { useRecordingState } from "./useRecordingState";
import { useRecordingStore } from "@/stores/recordingStore";

describe("useRecordingState event path", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    unlisten.mockClear();
    useRecordingStore.getState().reset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("maps backend state and audio events into the recording store", () => {
    renderHook(() => useRecordingState());

    act(() => listeners["recording-state-change"]("recording"));
    act(() => listeners["audio-level"](0.42));

    expect(useRecordingStore.getState()).toMatchObject({
      status: "recording",
      duration: 0,
      audioLevel: 0.42,
    });

    act(() => vi.advanceTimersByTime(300));
    expect(useRecordingStore.getState().duration).toBe(300);

    act(() => listeners["recording-state-change"]("processing"));
    act(() => vi.advanceTimersByTime(300));
    expect(useRecordingStore.getState().duration).toBe(300);

    act(() => listeners["recording-state-change"]("idle"));
    expect(useRecordingStore.getState().duration).toBe(0);
  });

  it("unsubscribes from both Tauri streams on unmount", async () => {
    const { unmount } = renderHook(() => useRecordingState());
    await act(() => Promise.resolve());

    unmount();
    expect(unlisten).toHaveBeenCalledTimes(2);
  });
});
