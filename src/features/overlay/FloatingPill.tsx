import { useEffect, useState, useCallback, useRef } from "react";
import {
  Loader2,
  Eye,
  ShieldCheck,
  Layers,
  Rocket,
  Ghost,
  Send,
  Sparkles,
  Mic,
} from "lucide-react";
import { useRecordingStore, type RecordingStatus } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import {
  startRecording,
  stopRecording,
  resizeOverlay,
  listContextModes,
  getActiveContextMode,
  setActiveContextMode,
  onContextModeChanged,
  onTranscriptionPreview,
  onSettingsChanged,
  onStructuredOutputReady,
  onStructuredModeDegraded,
  onWhisperGpuFallback,
  onCommandStateChange,
  onCommandConfirm,
  onCommandResult,
  getSettings,
  type ContextMode,
  type StructuredOutputPayload,
} from "@/lib/tauri";
import { formatDuration, cn } from "@/lib/utils";
import { useSettingsPatch } from "@/hooks/useSettingsPatch";
import { PillWaveform } from "./PillWaveform";
import { ModeSelector } from "./ModeSelector";
import { StructuredPanel } from "./StructuredPanel";
import { StructuredModeToggle } from "./StructuredModeToggle";
import { CommandPill } from "./CommandPill";
import { useCommandStore } from "@/stores/commandStore";
import { IDLE_WIN_H, IDLE_WIN_W, useOverlaySizing } from "./useOverlaySizing";
import "./FloatingPill.css";

type PillState = RecordingStatus | "success";

// Map mode color names → CSS color values for waveform bars.
// Graphite-system roles (keys kept for backward-compat with saved modes).
const MODE_COLORS: Record<string, string> = {
  amber: "rgb(245,158,11)",   // amber — primary / dictation
  blue: "rgb(96,165,250)",    // blue — command
  green: "rgb(74,222,128)",   // green — success
  purple: "rgb(167,139,250)", // violet — structured
  red: "rgb(239,68,68)",      // red — recording / error
  cyan: "rgb(45,212,191)",    // teal — spare
};

// Window sizes — button always fills the window 100%
export function FloatingPill() {
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const duration = useRecordingStore((s) => s.duration);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);
  const commandState = useCommandStore((s) => s.state);

  const [pillState, setPillState] = useState<PillState>("idle");
  const [flashText, setFlashText] = useState<string | null>(null);

  // Live preview state
  const [previewText, setPreviewText] = useState<string | null>(null);
  const [livePreviewEnabled, setLivePreviewEnabled] = useState(false);
  const [noiseReduction, setNoiseReduction] = useState(true);
  const [autoSwitchModes, setAutoSwitchModes] = useState(true);
  const [shipMode, setShipMode] = useState(false);
  const [commandSend, setCommandSend] = useState(true);
  const [ghostMode, setGhostMode] = useState(false);
  const [structuredMode, setStructuredMode] = useState(false);
  const [structuredVoiceCommand, setStructuredVoiceCommand] = useState(false);
  const [showShipPopup, setShowShipPopup] = useState(false);
  const [showLeyLinePopup, setShowLeyLinePopup] = useState(false);

  // Mode selector state
  const [showModeSelector, setShowModeSelector] = useState(false);
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
  const { settingsRef, replaceSettings, patchSettings } = useSettingsPatch();

  // True while the user is dictating *into the StructuredPanel's textarea*.
  // When set, (a) a fresh recording must NOT close the current panel, and
  // (b) any resulting `structured-output-ready` event must be ignored — the
  // dictation pass only exists to append raw text to the existing preview.
  //
  // The `false` flip is delayed by a grace period: pipeline.rs emits
  // `transcription-result` (line 800) and `structured-output-ready`
  // (line 802) back-to-back.  Without the delay, the panel's dictation
  // handler can flip isDictating→false after consuming the first event
  // and before the parent sees the second, letting structured-output-ready
  // clobber the in-progress panel.  600ms is comfortably longer than any
  // realistic gap between two adjacent Tauri event emits.
  const dictatingInPanelRef = useRef(false);
  // State mirror of the ref above.  The ref is read synchronously by the event
  // handlers (panel-close guard, structured-output-ready drop), but the
  // pill-state effect needs a reactive dependency to re-run and force the pill
  // back to idle when in-panel dictation toggles via the global-hotkey path.
  const [dictatingInPanel, setDictatingInPanel] = useState(false);
  const dictatingGraceTimerRef = useRef<number | null>(null);
  const degradedTimerRef = useRef<number | null>(null);
  const showContent = useOverlaySizing({
    pillState,
    hasStructuredPayload: Boolean(structuredPayload),
    structuredDegraded,
    showModeSelector,
    modeCount: modes.length,
    commandState,
  });
  const handleDictatingChange = useCallback((active: boolean) => {
    if (dictatingGraceTimerRef.current !== null) {
      window.clearTimeout(dictatingGraceTimerRef.current);
      dictatingGraceTimerRef.current = null;
    }
    if (active) {
      dictatingInPanelRef.current = true;
      setDictatingInPanel(true);
    } else {
      dictatingGraceTimerRef.current = window.setTimeout(() => {
        dictatingInPanelRef.current = false;
        setDictatingInPanel(false);
        dictatingGraceTimerRef.current = null;
      }, 600);
    }
  }, []);

  useEffect(() => {
    // While the user is dictating into the StructuredPanel's own textarea, the
    // panel renders its own recording/processing animation — keep the pill
    // itself idle so the active state doesn't double up.  Gated on
    // `structuredPayload` so a panel that was just dismissed can never suppress
    // a fresh, unrelated dictation during the 600ms grace tail.  The ref is set
    // synchronously before startRecording on the mic-button path (clean); the
    // `dictatingInPanel` mirror re-runs this effect for the global-hotkey path,
    // which may still show a 1–2 frame recording blip before it snaps idle.
    if (structuredPayload && (dictatingInPanel || dictatingInPanelRef.current)) {
      if (pillState !== "idle") {
        setPillState("idle");
        setFlashText(null);
      }
      return;
    }
    if (status === "idle" && lastTranscription && pillState === "processing") {
      setFlashText(
        lastTranscription.length > 30
          ? lastTranscription.slice(0, 30) + "…"
          : lastTranscription
      );
      setPillState("success");
      const timer = setTimeout(() => {
        setPillState("idle");
        setFlashText(null);
      }, 2500);
      return () => clearTimeout(timer);
    }
    if (status !== "idle" || pillState !== "success") {
      setPillState(status);
    }
  }, [status, lastTranscription, dictatingInPanel, structuredPayload]);

  useEffect(() => {
    return () => {
      if (dictatingGraceTimerRef.current !== null) {
        window.clearTimeout(dictatingGraceTimerRef.current);
        dictatingGraceTimerRef.current = null;
      }
      if (degradedTimerRef.current !== null) {
        window.clearTimeout(degradedTimerRef.current);
        degradedTimerRef.current = null;
      }
    };
  }, []);

  // Mount: transparent bg, force dark theme, shrink to idle
  useEffect(() => {
    document.documentElement.dataset.theme = "dark";
    document.documentElement.style.background = "transparent";
    document.documentElement.style.margin = "0";
    document.documentElement.style.padding = "0";
    document.documentElement.style.overflow = "hidden";
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    document.body.style.padding = "0";
    document.body.style.overflow = "hidden";
    document.body.classList.add("overlay-window");
    resizeOverlay(IDLE_WIN_W, IDLE_WIN_H);
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

  // Load settings, listen for changes from other windows, and preview events
  useEffect(() => {
    getSettings()
      .then((s) => {
        replaceSettings(s);
        setLivePreviewEnabled(s.live_preview);
        setNoiseReduction(s.noise_reduction);
        setAutoSwitchModes(s.auto_switch_modes);
        setShipMode(s.ship_mode);
        setCommandSend(s.command_send);
        setGhostMode(s.ghost_mode);
        setStructuredMode(s.structured_mode);
        setStructuredVoiceCommand(s.structured_voice_command);
      })
      .catch(() => {});

    const unlistenPreview = onTranscriptionPreview((text) => {
      // Keep a generous tail; the pill right-anchors the text and clips the
      // older words off the left, so the newest speech stays visible.
      const tail = text.length > 90 ? text.slice(-90) : text;
      setPreviewText(tail.replace(/^\s+/, ""));
    });

    // Stay in sync when settings change from the main window (or any window)
    const unlistenSettings = onSettingsChanged((s) => {
      replaceSettings(s);
      setLivePreviewEnabled(s.live_preview);
      setNoiseReduction(s.noise_reduction);
      setAutoSwitchModes(s.auto_switch_modes);
      setShipMode(s.ship_mode);
      setCommandSend(s.command_send);
      setGhostMode(s.ghost_mode);
      setStructuredMode(s.structured_mode);
      setStructuredVoiceCommand(s.structured_voice_command);
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

    return () => {
      unlistenPreview.then((fn) => fn());
      unlistenSettings.then((fn) => fn());
      unlistenStructured.then((fn) => fn());
      unlistenDegraded.then((fn) => fn());
      unlistenGpuFallback.then((fn) => fn());
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
      useCommandStore.getState().setState("confirm", p.summary);
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

  // Close the structured panel if the user starts a new recording — unless
  // the recording is the panel's own in-place dictation, in which case we
  // keep the panel mounted so the appended text can land in the textarea.
  useEffect(() => {
    if (
      status === "recording" &&
      structuredPayload &&
      !dictatingInPanelRef.current
    ) {
      setStructuredPayload(null);
    }
  }, [status, structuredPayload]);
  // Clear preview text when not recording
  useEffect(() => {
    if (status !== "recording") {
      setPreviewText(null);
    }
  }, [status]);

  // (Resize logic unified above — see the "Consolidated overlay sizing"
  // comment.  This block intentionally left blank after the merge.)

  const handleToggleAutoSwitch = useCallback(async () => {
    const next = !autoSwitchModes;
    setAutoSwitchModes(next);
    try {
      await patchSettings({ auto_switch_modes: next });
    } catch {
      setAutoSwitchModes(!next);
    }
  }, [autoSwitchModes, patchSettings]);

  const handleToggleLivePreview = useCallback(async () => {
    const next = !livePreviewEnabled;
    setLivePreviewEnabled(next); // optimistic
    try {
      await patchSettings({ live_preview: next });
    } catch {
      setLivePreviewEnabled(!next); // revert on failure
    }
  }, [livePreviewEnabled, patchSettings]);

  const handleToggleNoiseReduction = useCallback(async () => {
    const next = !noiseReduction;
    setNoiseReduction(next); // optimistic
    try {
      await patchSettings({ noise_reduction: next });
    } catch {
      setNoiseReduction(!next); // revert on failure
    }
  }, [noiseReduction, patchSettings]);

  const handleToggleShipMode = useCallback(async () => {
    const next = !shipMode;
    setShipMode(next);
    try {
      await patchSettings({ ship_mode: next });
    } catch {
      setShipMode(!next);
    }
  }, [shipMode, patchSettings]);

  const handleToggleStructuredMode = useCallback(async () => {
    const next = !structuredMode;
    setStructuredMode(next); // optimistic — UI transitions immediately
    try {
      await patchSettings({ structured_mode: next });
    } catch {
      setStructuredMode(!next);
    }
  }, [structuredMode, patchSettings]);

  const handleToggleStructuredVoiceCommand = useCallback(async () => {
    const next = !structuredVoiceCommand;
    setStructuredVoiceCommand(next);
    try {
      await patchSettings({ structured_voice_command: next });
    } catch {
      setStructuredVoiceCommand(!next);
    }
  }, [structuredVoiceCommand, patchSettings]);

  const handleToggleCommandSend = useCallback(async () => {
    const next = !commandSend;
    setCommandSend(next);
    try {
      await patchSettings({ command_send: next });
    } catch {
      setCommandSend(!next);
    }
  }, [commandSend, patchSettings]);

  const handleToggleGhostMode = useCallback(async () => {
    const next = !ghostMode;
    setGhostMode(next);
    // When activating ghost mode, close the menu so pill fades out
    if (next) {
      setShowModeSelector(false);
    }
    try {
      await patchSettings({ ghost_mode: next });
    } catch {
      setGhostMode(!next);
    }
  }, [ghostMode, patchSettings]);

  // Exit ghost mode — used when user clicks/right-clicks the invisible pill
  const exitGhostMode = useCallback(async () => {
    setGhostMode(false);
    try {
      await patchSettings({ ghost_mode: false });
    } catch {}
  }, [patchSettings]);

  const handleClick = useCallback(async () => {
    if (showModeSelector) return; // Don't start recording while selector is open
    // If ghost mode is active, just reveal the pill — don't trigger recording
    if (ghostMode) {
      exitGhostMode();
      return;
    }
    try {
      if (status === "idle") await startRecording();
      else if (status === "recording") await stopRecording();
    } catch (err) {
      console.error("Pill recording toggle failed:", err);
    }
  }, [status, showModeSelector, ghostMode, exitGhostMode]);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      // If ghost mode is active, reveal the pill and open the menu
      if (ghostMode) {
        exitGhostMode();
      }
      // The degraded banner clips the menu if left in place — the user's
      // intent when right-clicking is "show me the menu," so dismiss any
      // banner that's currently up so the menu has room to appear.
      if (structuredDegraded) {
        setStructuredDegraded(null);
      }
      // Allow the menu from any non-active pill state.  The `success` /
      // `error` states are transient tails of a completed recording
      // (2.5 s) — blocking the menu during them felt arbitrary to the
      // user, and the degraded banner commonly shows while pillState is
      // still `success`, so this is also part of the bug-2 fix.
      const canOpenMenu =
        pillState === "idle" ||
        pillState === "success" ||
        pillState === "error";
      if (canOpenMenu) {
        setShowModeSelector((prev) => {
          // Close nested popups when toggling mode selector
          if (!prev) {
            setShowShipPopup(false);
            setShowLeyLinePopup(false);
          }
          return !prev;
        });
      }
    },
    [pillState, ghostMode, exitGhostMode, structuredDegraded]
  );

  const handleModeSelect = useCallback(async (id: string) => {
    try {
      await setActiveContextMode(id);
      setActiveModId(id);
      const selected = modes.find((m) => m.id === id);
      if (selected?.color) setActiveColor(selected.color);
    } catch (e) {
      console.error("Failed to switch mode:", e);
    }
  }, [modes]);

  const isIdle = pillState === "idle";
  const isRecording = pillState === "recording";
  const isProcessing = pillState === "processing";
  const isStructuring = pillState === "structuring";
  const isSuccess = pillState === "success";
  const isError = pillState === "error";

  const modeColor = MODE_COLORS[activeColor] ?? MODE_COLORS.amber;

  // Command Mode takes over the overlay while active — it's mutually exclusive
  // with dictation (the backend's capture-mode guard guarantees only one runs).
  if (commandState !== "idle") {
    return (
      <div className="w-screen h-screen flex flex-col justify-end items-center">
        <CommandPill showContent={showContent} />
      </div>
    );
  }

  return (
    <div className="w-screen h-screen flex flex-col justify-end items-center">
      {/* Structured Mode panel — sits flush on top of the pill, forming a
          single unified surface.  Zero bottom margin is deliberate (the
          "reverse Dynamic Island" expansion effect): the panel's flat
          bottom merges visually into the pill's rounded top so they read
          as one connected shape instead of two floating bubbles.
          Gated on showContent so WebView2 finishes re-laying-out after
          the window resize before the panel mounts — otherwise a
          one-frame paint of the old layout in the new window bounds
          flashes the panel at the top-left of the expanded region. */}
      {showContent && structuredPayload && !ghostMode && (
        <div className="shrink-0">
          <StructuredPanel
            payload={structuredPayload}
            onClose={() => setStructuredPayload(null)}
            onDictatingChange={handleDictatingChange}
          />
        </div>
      )}

      {/* Transient degraded banner — LLM timed out / not loaded.
          Gated on showContent for the same anti-flicker reason. */}
      {showContent && structuredDegraded && !structuredPayload && !ghostMode && (
        <div
          className="mb-1.5 shrink-0 flex items-center gap-2 px-3 py-1.5 rounded-lg max-w-[380px] cursor-pointer group"
          onClick={() => setStructuredDegraded(null)}
          title="Click to dismiss"
          style={{
            background:
              "linear-gradient(180deg, rgba(26,26,30,0.96) 0%, rgba(16,15,18,0.96) 100%)",
            border: "1px solid rgba(245,158,11,0.30)",
            boxShadow: "0 10px 28px -14px rgba(0,0,0,0.8)",
            animation: "sp-in 220ms cubic-bezier(0.16,1,0.3,1) both",
          }}
        >
          <span
            aria-hidden="true"
            className="h-1.5 w-1.5 rounded-full shrink-0"
            style={{
              backgroundColor: "rgba(245,158,11,0.95)",
            }}
          />
          <span
            className="text-[9px] font-semibold uppercase tracking-[0.18em] shrink-0"
            style={{
              fontFamily: "var(--font-display)",
              color: "rgba(252,195,77,0.92)",
            }}
          >
            Structured
          </span>
          <span
            className="text-[10px] leading-snug truncate"
            style={{
              color: "rgba(244,244,245,0.9)",
              letterSpacing: "-0.005em",
            }}
          >
            {structuredDegraded}
          </span>
        </div>
      )}

      {/* Mode selector dropdown — centered above the pill.  Gated on
          showContent so the menu only mounts after the window has
          resized to 600×~200 + WebView2 has re-laid-out, preventing
          the one-frame flicker where the menu painted at the top-left
          of the old 56×26 bounds. */}
      {showContent && showModeSelector && modes.length > 0 && (
        <div className="relative shrink-0 flex justify-center w-full">
          <ModeSelector
            modes={modes}
            activeId={activeModId}
            onSelect={handleModeSelect}
            onClose={() => {
              setShowModeSelector(false);
              setShowShipPopup(false);
              setShowLeyLinePopup(false);
            }}
          />
          {/* Right-side controls — Ley Line on top (flagship) then the
              quick-toggle settings circles, all in one flex column so the
              same `gap-1.5` (6 px) spacing rule applies between every
              pair.  Pinned at `top: 0` so the Ley Line's top edge is
              always flush with the ModeSelector's top; the column's
              bottom floats based on content which keeps spacing uniform.
              `items-center` centres the 28 px Ley Line against the 26 px
              circles below it.  Uses mousedown for toggle action since
              the overlay is transparent and click events can be
              swallowed at window edges in WebView2. */}
          <div
            className="absolute flex flex-col items-center gap-1.5"
            style={{
              left: "calc(50% + 96px + 6px)",
              top: "0",
            }}
          >
            <div className="relative">
              <StructuredModeToggle
                active={structuredMode}
                onToggle={handleToggleStructuredMode}
                onContextMenu={() => {
                  setShowLeyLinePopup((prev) => !prev);
                  setShowShipPopup(false);
                }}
              />

              {/* ── Ley Line right-click popup: Voice Command gate ──
                  Mirrors the ship button's Command-Send popup, including
                  the same right-side position so it never overlays the
                  mode selector.  Width math (mirrors the comment on
                  ".ship-popup"): button ends at 50%+96+6+28 = 50%+130,
                  popup adds 8 gap + 160 = 50%+298 right edge, fits in
                  the 600 px window with 2 px margin. */}
              <div
                className="ley-line-popup"
                style={{
                  left: "calc(100% + 8px)",
                  // Align the popup's top with the button's top instead of
                  // centring on the button — the Ley Line is pinned to the
                  // menu's TOP edge, so a vertically-centred popup extended
                  // above the window and got clipped.  Top-aligned means the
                  // popup grows downward from the button into the menu's
                  // right margin, always fully visible.
                  top: "0",
                  transform: `scale(${showLeyLinePopup ? 1 : 0.92})`,
                  minWidth: 160,
                  opacity: showLeyLinePopup ? 1 : 0,
                  pointerEvents: showLeyLinePopup ? "auto" : "none",
                }}
                onMouseDown={(e) => e.stopPropagation()}
              >
                <div className="ley-line-popup-bloom" aria-hidden="true" />
                <div className="ley-line-popup-ring" aria-hidden="true" />
                <div className="ley-line-popup-content">
                  <div className="ley-line-popup-header">
                    <Mic size={10} strokeWidth={2} className="ley-line-popup-icon" />
                    <span className="ley-line-popup-kicker">Voice Command</span>
                  </div>
                  <p className="ley-line-popup-desc">
                    Say “Voxify” at the end to structure — otherwise paste plain
                  </p>
                  <div className="ley-line-popup-row">
                    <button
                      onMouseDown={(e) => {
                        e.stopPropagation();
                        e.preventDefault();
                        handleToggleStructuredVoiceCommand();
                      }}
                      className={cn(
                        "ley-line-popup-switch",
                        structuredVoiceCommand && "ley-line-popup-switch--on"
                      )}
                    >
                      <span className="ley-line-popup-knob" />
                    </button>
                    <span className="ley-line-popup-state">
                      {structuredVoiceCommand ? "On" : "Off"}
                    </span>
                  </div>
                </div>
              </div>
            </div>
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleAutoSwitch();
              }}
              data-tip="Auto-switch mode by active app"
              aria-label="Auto-switch mode by active app"
              className={cn(
                "quick-toggle",
                autoSwitchModes && "quick-toggle--on"
              )}
            >
              <Layers size={12} strokeWidth={2} className="quick-toggle-icon" />
            </button>
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleLivePreview();
              }}
              data-tip="Show words live as you speak"
              aria-label="Show words live as you speak"
              className={cn(
                "quick-toggle",
                livePreviewEnabled && "quick-toggle--on"
              )}
            >
              <Eye size={12} strokeWidth={2} className="quick-toggle-icon" />
            </button>
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleNoiseReduction();
              }}
              data-tip="Suppress background noise"
              aria-label="Suppress background noise"
              className={cn(
                "quick-toggle",
                noiseReduction && "quick-toggle--on"
              )}
            >
              <ShieldCheck size={12} strokeWidth={2} className="quick-toggle-icon" />
            </button>
            <div className="relative">
              <button
                onMouseDown={(e) => {
                  e.stopPropagation();
                  e.preventDefault();
                  // Only toggle ship mode on left-click (button 0)
                  if (e.button === 0) handleToggleShipMode();
                }}
                onContextMenu={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  setShowShipPopup((prev) => !prev);
                }}
                title={shipMode ? "Ship mode: on (right-click for options)" : "Ship mode: off (right-click for options)"}
                className={cn(
                  "quick-toggle",
                  shipMode && "quick-toggle--on"
                )}
              >
                <Rocket size={12} strokeWidth={2} className="quick-toggle-icon" />
              </button>

              {/* ── Ship button right-click popup ── */}
              <div
                className="ship-popup"
                style={{
                  left: "calc(100% + 8px)",
                  top: "50%",
                  transform: `translateY(-50%) scale(${showShipPopup ? 1 : 0.92})`,
                  minWidth: 168,
                  opacity: showShipPopup ? 1 : 0,
                  pointerEvents: showShipPopup ? "auto" : "none",
                }}
                onMouseDown={(e) => e.stopPropagation()}
              >
                <div className="ship-popup-bloom" aria-hidden="true" />
                <div className="ship-popup-ring" aria-hidden="true" />
                <div className="ship-popup-content">
                  <div className="ship-popup-header">
                    <Send size={10} strokeWidth={2} className="ship-popup-icon" />
                    <span className="ship-popup-kicker">Command Send</span>
                  </div>
                  <p className="ship-popup-desc">
                    Say "send" to submit instead of auto-sending everything
                  </p>
                  <div className="ship-popup-row">
                    <button
                      onMouseDown={(e) => {
                        e.stopPropagation();
                        e.preventDefault();
                        handleToggleCommandSend();
                      }}
                      className={cn(
                        "ship-popup-switch",
                        commandSend && "ship-popup-switch--on"
                      )}
                    >
                      <span className="ship-popup-knob" />
                    </button>
                    <span className="ship-popup-state">
                      {commandSend ? "On" : "Off"}
                    </span>
                  </div>
                </div>
              </div>
            </div>
          </div>
          {/* Divider between toggle buttons and ghost mode */}
          <div
            className="absolute"
            style={{
              left: "calc(50% + 96px + 8px)",
              bottom: "38px",
              width: 22,
              height: 1,
              background:
                "linear-gradient(90deg, rgba(255,235,200,0) 0%, rgba(255,235,200,0.18) 50%, rgba(255,235,200,0) 100%)",
              borderRadius: 1,
            }}
          />
          {/* Ghost mode — positioned parallel with "Open OmniVox" row */}
          <div
            className="absolute"
            style={{
              left: "calc(50% + 96px + 6px)",
              bottom: "8px",
            }}
          >
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleGhostMode();
              }}
              data-tip="Hide the pill until you summon it"
              aria-label="Hide the pill until you summon it"
              className={cn("quick-toggle quick-toggle--ghost", ghostMode && "quick-toggle--ghost-on")}
            >
              <Ghost size={12} strokeWidth={2} className="quick-toggle-icon" />
            </button>
          </div>
        </div>
      )}

    <button
      onClick={handleClick}
      onContextMenu={handleContextMenu}
      disabled={isProcessing}
      style={{
        // Ghost mode: fully transparent but still interactive
        opacity: ghostMode && !showModeSelector ? 0 : 1,
        // Idle locator — a super-faint amber glow so the dark slit stays
        // findable on a black UI behind it. It bleeds into the transparent
        // margin the idle window reserves around the pill (IDLE_GLOW_PAD); kept
        // off in every active state (the content itself marks the spot there).
        boxShadow:
          isIdle && !showModeSelector
            ? "0 0 3px rgba(251,191,36,0.34), 0 0 9px 1px rgba(251,191,36,0.20)"
            : undefined,
        transition: "opacity 0.25s ease, box-shadow 0.3s ease",
      }}
      className={cn(
        // The pill — sized to match resizeOverlay dimensions.  Every
        // state carries `border border-transparent` so the 1 px border
        // is always present; only its COLOR changes between states.
        // Without this, idle→active would transition border-width
        // from 0→1 px, which can't interpolate and snaps instead —
        // producing a visible one-frame jolt.  Colour + background
        // transitions below pick up those same class changes and
        // smooth them over 200 ms.
        // Idle = 42-px slit · menu-open = a thin base slit the exact width of the
        // mode-selector panel (196) · active = the snug recording pill.
        showModeSelector
          ? "w-[196px] h-[14px]"
          : isIdle
            ? "w-[42px] h-[14px] mb-[2px]"
            : "w-[148px] h-[34px]",
        "relative flex items-center overflow-hidden shrink-0 border border-transparent rounded-full",
        "transition-[border-color,background-color,box-shadow,width,height] duration-[280ms] ease-[cubic-bezier(0.32,0.72,0,1)]",
        isProcessing ? "cursor-default" : "cursor-pointer",

        // Idle — a small black slit with a faint hairline so it stays findable.
        isIdle && "bg-[var(--color-pill-bg)] border-white/10",

        // Recording
        isRecording && "bg-[var(--color-pill-bg)] border-recording-500/30 gap-2 px-3",

        // Processing
        isProcessing && "bg-[var(--color-pill-bg)] border-amber-500/25 gap-2 px-3",

        // Structuring (Structured Mode — LLM slot extraction in flight)
        isStructuring && "bg-[var(--color-pill-bg)] border-amber-500/30 gap-2 px-3",

        // Success
        isSuccess && "bg-[var(--color-pill-bg)] border-success/30 gap-2 px-3",

        // Error
        isError && "bg-[var(--color-pill-bg)] border-recording-500/35 gap-2 px-3",
      )}
    >
      {/* ── Active states: full pill content with fade. Idle is a totally black
          slit (no content); when the menu is open the pill is just a thin base
          slit, so suppress content there too. ── */}
      {!isIdle && !showModeSelector && (
        <div
          className="flex items-center w-full h-full gap-2"
          style={{
            opacity: showContent ? 1 : 0,
            // Asymmetric transition — key polish fix.
            // Before: `opacity 0.2s ease` applied in both directions,
            // which meant the 80 ms hide window (set before resize)
            // cut off the fade-out at ~60 % opacity, then React flipped
            // showContent back to true and the fade reversed.  User
            // perception: "content dims and brightens for no reason"
            // = the one-frame flicker.
            // Now: hide is instant (transition: "none" when going
            // false), so no partial fade is ever visible.  Show uses a
            // 40 ms delay to give WebView2 a margin beyond the 80 ms
            // resize window before the pixels arrive, then fades in
            // cleanly over 220 ms.
            transition: showContent
              ? "opacity 220ms cubic-bezier(0.4, 0, 0.2, 1) 40ms"
              : "none",
          }}
        >
          {isProcessing && (
            <div
              className="absolute inset-0 overflow-hidden pointer-events-none"
              aria-hidden="true"
            >
              <div
                className="absolute inset-0 -translate-x-full"
                style={{
                  background:
                    "linear-gradient(90deg, transparent 0%, rgba(245,158,11,0.06) 50%, transparent 100%)",
                  animation: "shimmer 2s ease-in-out infinite",
                }}
              />
            </div>
          )}

          {/* Left: timer / spinner / icon */}
          <div className="shrink-0 flex items-center justify-center min-w-[28px]">
            {isRecording && (
              <span className="font-mono text-[11px] tabular-nums text-recording-300/80 tracking-wide">
                {formatDuration(duration)}
              </span>
            )}
            {isStructuring && (
              <span className="relative flex items-center justify-center">
                <Sparkles
                  size={12}
                  className="relative text-amber-300"
                  strokeWidth={2.5}
                  style={{
                    animation: "structuring-spark 2.2s ease-in-out infinite",
                  }}
                />
              </span>
            )}
            {isSuccess && (
              <svg
                width="12" height="12" viewBox="0 0 16 16"
                className="text-success/80" fill="none" stroke="currentColor"
                strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"
              >
                <polyline points="3 8.5 6.5 12 13 4" />
              </svg>
            )}
            {isError && (
              <span className="text-recording-400/80 text-[11px] font-semibold">!</span>
            )}
          </div>

          {/* Center: waveform / preview text / status text */}
          <div className="flex-1 flex items-center justify-center overflow-hidden">
            {isRecording && previewText && (
              // Right-anchored teleprompter: newest words pinned to the right,
              // older ones clipped off the left as speech streams in.
              <div className="flex w-full justify-end overflow-hidden">
                <span
                  className="whitespace-nowrap text-[10px] font-normal tracking-tight"
                  style={{ color: modeColor, opacity: 0.7 }}
                >
                  {previewText}
                </span>
              </div>
            )}
            {isRecording && !previewText && <PillWaveform active color={modeColor} />}
            {isProcessing && (
              <Loader2 size={13} className="text-amber-400/70 animate-spin" strokeWidth={2.5} />
            )}
            {isStructuring && (
              <span
                className="text-[10px] font-medium tracking-[0.14em] uppercase truncate"
                style={{
                  fontFamily: "var(--font-display)",
                  background:
                    "linear-gradient(90deg, rgba(245,158,11,0.45) 0%, rgba(252,195,77,0.95) 50%, rgba(245,158,11,0.45) 100%)",
                  backgroundSize: "220% 100%",
                  WebkitBackgroundClip: "text",
                  WebkitTextFillColor: "transparent",
                  animation: "structuring-shimmer 2.4s linear infinite",
                }}
              >
                Structuring
                <span
                  aria-hidden="true"
                  style={{
                    display: "inline-block",
                    width: "1.2em",
                    textAlign: "left",
                    marginLeft: "1px",
                  }}
                >
                  <span
                    style={{
                      animation: "structuring-dot 1.4s ease-in-out infinite",
                      animationDelay: "0s",
                    }}
                  >
                    ·
                  </span>
                  <span
                    style={{
                      animation: "structuring-dot 1.4s ease-in-out infinite",
                      animationDelay: "0.2s",
                    }}
                  >
                    ·
                  </span>
                  <span
                    style={{
                      animation: "structuring-dot 1.4s ease-in-out infinite",
                      animationDelay: "0.4s",
                    }}
                  >
                    ·
                  </span>
                </span>
              </span>
            )}
            {isSuccess && flashText && (
              <span className="text-[10px] text-text-secondary/70 truncate">
                {flashText}
              </span>
            )}
            {isError && (
              <span className="text-[10px] text-recording-300/70 truncate">
                Error
              </span>
            )}
          </div>

          {/* Right: record dot */}
          <div className="shrink-0 w-[16px] flex items-center justify-end">
            {isRecording && (
              <div className="relative flex items-center justify-center">
                <span
                  className="absolute h-3.5 w-3.5 rounded-full bg-recording-500/15"
                  style={{ animation: "recording-pulse 2s ease-in-out infinite" }}
                />
                <span className="relative h-1.5 w-1.5 rounded-full bg-recording-500" />
              </div>
            )}
            {isStructuring && (
              <div className="relative flex items-center justify-center">
                <span
                  className="relative h-1.5 w-1.5 rounded-full"
                  style={{
                    backgroundColor: "rgb(245,158,11)",
                    animation: "structuring-pulse 2s ease-in-out infinite",
                  }}
                />
              </div>
            )}
            {isSuccess && (
              <div className="h-1.5 w-1.5 rounded-full bg-success/40" />
            )}
          </div>
        </div>
      )}
    </button>
    </div>
  );
}
