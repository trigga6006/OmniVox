// @vitest-environment jsdom

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const listeners: Record<string, (payload: any) => void> = {};

vi.mock("@/lib/tauri", () => {
  const subscribe = (name: string) => (callback: (payload: any) => void) => {
    listeners[name] = callback;
    return Promise.resolve(() => {});
  };
  return {
    listContextModes: () => Promise.resolve([]),
    getActiveContextMode: () => Promise.resolve(null),
    onContextModeChanged: subscribe("context-mode-changed"),
    onTranscriptionPreview: subscribe("transcription-preview"),
    onTranscriptionResult: subscribe("transcription-result"),
    onStructuredOutputReady: subscribe("structured-output-ready"),
    onStructuredModeDegraded: subscribe("structured-mode-degraded"),
    onWhisperGpuFallback: subscribe("whisper-gpu-fallback"),
    onLlmGpuFallback: subscribe("llm-gpu-fallback"),
    onGpuEnvironmentWarning: subscribe("gpu-environment-warning"),
    onLlmStatus: subscribe("llm-status"),
    onCommandStateChange: subscribe("command-state-change"),
    onCommandConfirm: subscribe("command-confirm"),
    onCommandResult: subscribe("command-result"),
    discardStructuredOutput: vi.fn(() => Promise.resolve(false)),
  };
});

import { useOverlayEvents } from "./useOverlayEvents";
import { useCommandStore } from "@/stores/commandStore";
import {
  discardStructuredOutput,
  type AppSettings,
  type StructuredOutputPayload,
} from "@/lib/tauri";

const payload = (markdown: string): StructuredOutputPayload => ({
  generation: 1,
  markdown,
  slots: {},
  raw_transcript: markdown,
  truncated_chars: 0,
  binding_id: `${markdown}-binding`,
});

describe("useOverlayEvents backend event path", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.mocked(discardStructuredOutput).mockClear();
    useCommandStore.getState().reset();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows structured output unless panel dictation or ghost mode owns it", () => {
    const dictatingInPanelRef = { current: false };
    const settingsRef = {
      current: { ghost_mode: false } as AppSettings,
    };
    const closeModeSelector = vi.fn();
    const closeShipPopup = vi.fn();
    const closeLeyLinePopup = vi.fn();
    const { result } = renderHook(() =>
      useOverlayEvents({
        status: "idle",
        dictatingInPanelRef,
        settingsRef,
        setShowModeSelector: closeModeSelector,
        setShowShipPopup: closeShipPopup,
        setShowLeyLinePopup: closeLeyLinePopup,
      })
    );

    act(() => listeners["structured-output-ready"](payload("first")));
    expect(result.current.structuredPayload?.markdown).toBe("first");
    expect(closeModeSelector).toHaveBeenCalledWith(false);
    expect(closeShipPopup).toHaveBeenCalledWith(false);
    expect(closeLeyLinePopup).toHaveBeenCalledWith(false);

    dictatingInPanelRef.current = true;
    act(() => listeners["structured-output-ready"](payload("panel-owned")));
    expect(result.current.structuredPayload?.markdown).toBe("first");

    dictatingInPanelRef.current = false;
    settingsRef.current = { ghost_mode: true } as AppSettings;
    act(() => listeners["structured-output-ready"](payload("ghosted")));
    expect(result.current.structuredPayload).toBeNull();
    expect(discardStructuredOutput).toHaveBeenCalledWith("first-binding", 1);
    expect(discardStructuredOutput).toHaveBeenCalledWith("ghosted-binding", 1);
  });

  it("retires replacements and ignores a stale panel close", () => {
    const { result } = renderHook(() =>
      useOverlayEvents({
        status: "idle",
        dictatingInPanelRef: { current: false },
        settingsRef: { current: { ghost_mode: false } as AppSettings },
        setShowModeSelector: vi.fn(),
        setShowShipPopup: vi.fn(),
        setShowLeyLinePopup: vi.fn(),
      })
    );
    const first = payload("first");
    const second = { ...payload("second"), generation: 2 };

    act(() => listeners["structured-output-ready"](first));
    act(() => listeners["structured-output-ready"](second));
    expect(discardStructuredOutput).toHaveBeenCalledWith("first-binding", 1);
    expect(result.current.structuredPayload).toEqual(second);

    act(() => result.current.dismissStructuredPayload(first));
    expect(result.current.structuredPayload).toEqual(second);

    act(() => result.current.dismissStructuredPayload(second));
    expect(result.current.structuredPayload).toBeNull();
    expect(discardStructuredOutput).toHaveBeenCalledWith("second-binding", 2);
  });

  it("surfaces GPU fallback and clears the banner after its full dwell time", () => {
    const { result } = renderHook(() =>
      useOverlayEvents({
        status: "idle",
        dictatingInPanelRef: { current: false },
        settingsRef: { current: null },
        setShowModeSelector: vi.fn(),
        setShowShipPopup: vi.fn(),
        setShowLeyLinePopup: vi.fn(),
      })
    );

    act(() => listeners["whisper-gpu-fallback"]("Vulkan unavailable; using CPU"));
    expect(result.current.structuredDegraded).toBe(
      "Vulkan unavailable; using CPU"
    );

    act(() => vi.advanceTimersByTime(19_999));
    expect(result.current.structuredDegraded).not.toBeNull();
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.structuredDegraded).toBeNull();
  });

  it("surfaces an integrated-only GPU environment warning in the banner", () => {
    const { result } = renderHook(() =>
      useOverlayEvents({
        status: "idle",
        dictatingInPanelRef: { current: false },
        settingsRef: { current: null },
        setShowModeSelector: vi.fn(),
        setShowShipPopup: vi.fn(),
        setShowLeyLinePopup: vi.fn(),
      })
    );

    act(() =>
      listeners["gpu-environment-warning"](
        "GPU acceleration is running on the integrated GPU (AMD Radeon(TM) Graphics)"
      )
    );
    expect(result.current.structuredDegraded).toContain("integrated GPU");

    act(() => vi.advanceTimersByTime(20_000));
    expect(result.current.structuredDegraded).toBeNull();
  });

  it("surfaces an LLM CPU fallback instead of silently getting slower", () => {
    const { result } = renderHook(() =>
      useOverlayEvents({
        status: "idle",
        dictatingInPanelRef: { current: false },
        settingsRef: { current: null },
        setShowModeSelector: vi.fn(),
        setShowShipPopup: vi.fn(),
        setShowLeyLinePopup: vi.fn(),
      })
    );

    act(() => listeners["llm-gpu-fallback"]({
      model_id: "qwen",
      backend: { kind: "cpu_fallback" },
      duration_ms: 1000,
    }));
    expect(result.current.structuredDegraded).toContain("running on CPU");
  });

  it("moves Command Mode through confirm, completion, and timed reset", () => {
    renderHook(() =>
      useOverlayEvents({
        status: "idle",
        dictatingInPanelRef: { current: false },
        settingsRef: { current: null },
        setShowModeSelector: vi.fn(),
        setShowShipPopup: vi.fn(),
        setShowLeyLinePopup: vi.fn(),
      })
    );

    act(() => listeners["command-state-change"]("recognizing"));
    expect(useCommandStore.getState().state).toBe("recognizing");

    act(() =>
      listeners["command-confirm"]({
        generation: 3,
        id: 17,
        summary: "Send message?",
        editable_text: "hello",
      })
    );
    expect(useCommandStore.getState()).toMatchObject({
      state: "confirm",
      summary: "Send message?",
      editableText: "hello",
      confirmId: 17,
    });

    act(() =>
      listeners["command-result"]({
        generation: 3,
        status: "done",
        summary: "Message sent",
      })
    );
    expect(useCommandStore.getState().state).toBe("done");
    act(() => vi.advanceTimersByTime(2_600));
    expect(useCommandStore.getState().state).toBe("idle");
  });
});
