import {
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import {
  listContextModes,
  getActiveContextMode,
  onContextModeChanged,
  onTranscriptionPreview,
  onTranscriptionResult,
  onStructuredOutputReady,
  onStructuredModeDegraded,
  onWhisperGpuFallback,
  onLlmStatus,
  onCommandStateChange,
  onCommandConfirm,
  onCommandResult,
  type AppSettings,
  type ContextMode,
  type StructuredOutputPayload,
} from "@/lib/tauri";
import type { RecordingStatus } from "@/stores/recordingStore";
import { useCommandStore } from "@/stores/commandStore";

interface OverlayEventsOptions {
  status: RecordingStatus;
  // See the comment on `dictatingInPanelRef` in FloatingPill.tsx — read
  // synchronously by the structured-output-ready and panel-close guards.
  dictatingInPanelRef: MutableRefObject<boolean>;
  settingsRef: MutableRefObject<AppSettings | null>;
  setShowModeSelector: Dispatch<SetStateAction<boolean>>;
  setShowShipPopup: Dispatch<SetStateAction<boolean>>;
  setShowLeyLinePopup: Dispatch<SetStateAction<boolean>>;
}

// Tauri event wiring for the overlay pill: transcription preview, structured
// output / degraded banners, Command Mode state, and context-mode loading.
// Owns the state those events drive; FloatingPill stays the orchestrator.
export function useOverlayEvents({
  status,
  dictatingInPanelRef,
  settingsRef,
  setShowModeSelector,
  setShowShipPopup,
  setShowLeyLinePopup,
}: OverlayEventsOptions) {
  // Live preview state
  const [previewText, setPreviewText] = useState<string | null>(null);

  // Overlay-local last transcription — the main window's store
  // `lastTranscription` is never populated in this isolated runtime, so the
  // pill's success flash is driven straight from `transcription-result`. (M7)
  const [lastTranscript, setLastTranscript] = useState<string | null>(null);

  // Mode selector state
  const [modes, setModes] = useState<ContextMode[]>([]);
  const [activeModId, setActiveModId] = useState<string | null>(null);
  const [activeColor, setActiveColor] = useState("amber");

  // Structured Mode panel state — populated when the pipeline emits
  // `structured-output-ready`.  Cleared on dismiss / paste / new recording.
  const [structuredPayload, setStructuredPayload] =
    useState<StructuredOutputPayload | null>(null);
  const [structuredDegraded, setStructuredDegraded] = useState<string | null>(
    null
  );
  const degradedTimerRef = useRef<number | null>(null);

  // Structured-Mode LLM lifecycle ("loading" | "ready" | "error: …") — lets
  // the structuring pill say "Loading model" during a lazy load instead of
  // appearing hung.  Load failures also emit structured-mode-degraded, so
  // the banner covers the error case.
  const [llmStatus, setLlmStatus] = useState<string | null>(null);

  useEffect(() => {
    return () => {
      if (degradedTimerRef.current !== null) {
        window.clearTimeout(degradedTimerRef.current);
        degradedTimerRef.current = null;
      }
    };
  }, []);

  // Load modes on mount and listen for changes
  useEffect(() => {
    const loadModes = async () => {
      try {
        const [m, active] = await Promise.all([
          listContextModes(),
          getActiveContextMode(),
        ]);
        setModes(m);
        setActiveModId(active?.id ?? null);
        if (active?.color) setActiveColor(active.color);
      } catch {}
    };
    loadModes();

    const unlisten = onContextModeChanged((payload) => {
      setActiveModId(payload.id);
      if (payload.color) setActiveColor(payload.color);
      // Refresh modes list in case names changed
      listContextModes().then(setModes).catch(() => {});
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Preview + structured-output events
  useEffect(() => {
    const unlistenPreview = onTranscriptionPreview((text) => {
      // Keep a generous tail; the pill right-anchors the text and clips the
      // older words off the left, so the newest speech stays visible.
      const tail = text.length > 90 ? text.slice(-90) : text;
      setPreviewText(tail.replace(/^\s+/, ""));
    });

    const unlistenResult = onTranscriptionResult((text) => {
      setLastTranscript(text);
    });

    const unlistenStructured = onStructuredOutputReady((payload) => {
      // If the user is dictating into the existing panel's textarea, this
      // event is the by-product of that dictation run — drop it so we don't
      // clobber their in-progress edits.  History still records it.
      if (dictatingInPanelRef.current) {
        return;
      }
      // Close any other floating UI — the panel takes priority.
      setShowModeSelector(false);
      setShowShipPopup(false);
      setShowLeyLinePopup(false);
      setStructuredDegraded(null);
      // Respect ghost mode: if the user has hidden the pill, they explicitly
      // don't want UI popping up.  History still records the structured
      // output; they can review it later.
      if (settingsRef.current?.ghost_mode) {
        return;
      }
      setStructuredPayload(payload);
    });

    const unlistenDegraded = onStructuredModeDegraded((reason) => {
      console.warn("[structured-mode] degraded:", reason);
      setStructuredDegraded(reason);
      // Keep the banner visible long enough to actually be read.
      if (degradedTimerRef.current !== null) {
        window.clearTimeout(degradedTimerRef.current);
      }
      degradedTimerRef.current = window.setTimeout(() => {
        setStructuredDegraded(null);
        degradedTimerRef.current = null;
      }, 15000);
    });

    // GPU→CPU fallback at model load: same banner — the user otherwise has
    // no way to tell why transcription is suddenly several times slower.
    const unlistenGpuFallback = onWhisperGpuFallback((message) => {
      console.warn("[whisper] gpu fallback:", message);
      setStructuredDegraded(message);
      if (degradedTimerRef.current !== null) {
        window.clearTimeout(degradedTimerRef.current);
      }
      degradedTimerRef.current = window.setTimeout(() => {
        setStructuredDegraded(null);
        degradedTimerRef.current = null;
      }, 20000);
    });

    const unlistenLlmStatus = onLlmStatus((status) => {
      setLlmStatus(status);
    });

    return () => {
      unlistenPreview.then((fn) => fn());
      unlistenResult.then((fn) => fn());
      unlistenStructured.then((fn) => fn());
      unlistenDegraded.then((fn) => fn());
      unlistenGpuFallback.then((fn) => fn());
      unlistenLlmStatus.then((fn) => fn());
    };
  }, []);

  // Command Mode events → drive the command pill (a separate, mutually-
  // exclusive surface from dictation).  done/error are transient: they linger
  // briefly then the pill collapses back to idle.
  useEffect(() => {
    let clearTimer: number | null = null;
    const scheduleClear = () => {
      if (clearTimer !== null) window.clearTimeout(clearTimer);
      clearTimer = window.setTimeout(() => {
        useCommandStore.getState().reset();
        clearTimer = null;
      }, 2600);
    };

    const unState = onCommandStateChange((s) => {
      if (clearTimer !== null) {
        window.clearTimeout(clearTimer);
        clearTimer = null;
      }
      if (s === "listening") useCommandStore.getState().setState("listening");
      else if (s === "recognizing") useCommandStore.getState().setState("recognizing");
      else useCommandStore.getState().reset();
    });
    const unConfirm = onCommandConfirm((p) => {
      if (clearTimer !== null) {
        window.clearTimeout(clearTimer);
        clearTimer = null;
      }
      useCommandStore
        .getState()
        .setState("confirm", p.summary, p.editable_text ?? null, p.id);
    });
    const unResult = onCommandResult((p) => {
      useCommandStore
        .getState()
        .setState(p.status === "done" ? "done" : "error", p.summary);
      scheduleClear();
    });

    return () => {
      if (clearTimer !== null) window.clearTimeout(clearTimer);
      unState.then((fn) => fn());
      unConfirm.then((fn) => fn());
      unResult.then((fn) => fn());
    };
  }, []);

  // A recording started while the structured panel is open always routes INTO
  // the panel: the StructuredPanel claims any recording begun while it's
  // mounted as in-place dictation and appends the result to its textarea.  We
  // deliberately do NOT close the panel here — the old close-on-recording
  // effect raced the `dictatingInPanelRef` flip on the global-hotkey path and
  // orphaned the recording (H8).

  // Clear per-recording UI on status change.  Not recording → drop the live
  // preview.  Entering a fresh recording → drop the previous transcript so an
  // empty dictation (which emits idle with no `transcription-result`) can't
  // re-flash stale text on the pill. (M7)
  useEffect(() => {
    if (status === "recording") {
      setLastTranscript(null);
    } else {
      setPreviewText(null);
    }
  }, [status]);

  return {
    previewText,
    lastTranscript,
    modes,
    activeModId,
    setActiveModId,
    activeColor,
    setActiveColor,
    structuredPayload,
    setStructuredPayload,
    structuredDegraded,
    setStructuredDegraded,
    llmStatus,
  };
}
