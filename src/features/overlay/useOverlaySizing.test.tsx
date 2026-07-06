import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { renderHook, act } from "@testing-library/react";

const resizeOverlay = vi.fn();
vi.mock("@/lib/tauri", () => ({
  resizeOverlay: (w: number, h: number) => resizeOverlay(w, h),
}));

import {
  useOverlaySizing,
  ACTIVE_W,
  ACTIVE_H,
  IDLE_WIN_W,
  IDLE_WIN_H,
} from "./useOverlaySizing";
import type { RecordingStatus } from "@/stores/recordingStore";
import type { CommandUiState } from "@/stores/commandStore";

type Props = {
  pillState: RecordingStatus | "success";
  hasStructuredPayload: boolean;
  structuredDegraded: string | null;
  showModeSelector: boolean;
  modeCount: number;
  commandState: CommandUiState;
  commandEditableConfirm: boolean;
};

const idleProps: Props = {
  pillState: "idle",
  hasStructuredPayload: false,
  structuredDegraded: null,
  showModeSelector: false,
  modeCount: 0,
  commandState: "idle",
  commandEditableConfirm: false,
};

beforeEach(() => {
  vi.useFakeTimers();
  resizeOverlay.mockClear();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("useOverlaySizing", () => {
  it("does not resize on mount while idle (window is created at idle size)", () => {
    renderHook((p: Props) => useOverlaySizing(p), { initialProps: idleProps });
    expect(resizeOverlay).not.toHaveBeenCalled();
  });

  it("grows directly to the active size and reveals content after the relayout delay", () => {
    const { result, rerender } = renderHook((p: Props) => useOverlaySizing(p), {
      initialProps: idleProps,
    });

    rerender({ ...idleProps, pillState: "recording" });

    // Active fully contains idle, so a single resize with no settle pass.
    expect(resizeOverlay).toHaveBeenCalledTimes(1);
    expect(resizeOverlay).toHaveBeenCalledWith(ACTIVE_W, ACTIVE_H);

    // Content is hidden during the resize, shown after the 80ms relayout delay.
    expect(result.current).toBe(false);
    act(() => vi.advanceTimersByTime(80));
    expect(result.current).toBe(true);
  });

  it("collapsing resizes to the union first, then settles to the target", () => {
    const { rerender } = renderHook((p: Props) => useOverlaySizing(p), {
      initialProps: idleProps,
    });

    rerender({ ...idleProps, pillState: "recording" });
    resizeOverlay.mockClear();

    rerender(idleProps);

    // Union of active(156x34) and idle(62x26) is the active size — the
    // window holds it during the pill's shrink animation…
    expect(resizeOverlay).toHaveBeenCalledWith(ACTIVE_W, ACTIVE_H);

    // …then settles to the exact idle target after the transition.
    act(() => vi.advanceTimersByTime(320));
    expect(resizeOverlay).toHaveBeenLastCalledWith(IDLE_WIN_W, IDLE_WIN_H);
    expect(resizeOverlay).toHaveBeenCalledTimes(2);
  });

  it("sizes the command confirm state for the Yes/No pill", () => {
    const { rerender } = renderHook((p: Props) => useOverlaySizing(p), {
      initialProps: idleProps,
    });

    rerender({ ...idleProps, commandState: "confirm" });
    expect(resizeOverlay).toHaveBeenCalledWith(300, 46);
  });

  it("sizes the editable review-message confirm taller", () => {
    const { rerender } = renderHook((p: Props) => useOverlaySizing(p), {
      initialProps: idleProps,
    });

    rerender({ ...idleProps, commandState: "confirm", commandEditableConfirm: true });
    expect(resizeOverlay).toHaveBeenCalledWith(340, 96);
  });

  it("does not resize again when the target is unchanged", () => {
    const { rerender } = renderHook((p: Props) => useOverlaySizing(p), {
      initialProps: idleProps,
    });

    rerender({ ...idleProps, pillState: "recording" });
    act(() => vi.advanceTimersByTime(500));
    resizeOverlay.mockClear();

    // recording → processing keeps the same active size target.
    rerender({ ...idleProps, pillState: "processing" });
    expect(resizeOverlay).not.toHaveBeenCalled();
  });

  it("prioritizes the structured payload panel over the pill state", () => {
    const { rerender } = renderHook((p: Props) => useOverlaySizing(p), {
      initialProps: idleProps,
    });

    rerender({ ...idleProps, pillState: "processing", hasStructuredPayload: true });
    expect(resizeOverlay).toHaveBeenCalledWith(440, 480);
  });
});
