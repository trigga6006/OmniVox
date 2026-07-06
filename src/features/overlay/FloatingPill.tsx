import { useEffect, useState, useCallback, useRef } from "react";
import { useRecordingStore, type RecordingStatus } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import {
  startRecording,
  stopRecording,
  resizeOverlay,
  setActiveContextMode,
  type AppSettings,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useSettingsPatch } from "@/hooks/useSettingsPatch";
import { useSettingsSync } from "@/hooks/useSettingsSync";
import { ModeSelector } from "./ModeSelector";
import { StructuredPanel } from "./StructuredPanel";
import { CommandPill } from "./CommandPill";
import { PillContent } from "./PillContent";
import { QuickToggles } from "./QuickToggles";
import { useOverlayEvents } from "./useOverlayEvents";
import { useCommandStore } from "@/stores/commandStore";
import {
  ACTIVE_H,
  ACTIVE_PILL_W,
  IDLE_H,
  IDLE_W,
  IDLE_WIN_H,
  IDLE_WIN_W,
  MENU_PILL_W,
  useOverlaySizing,
} from "./useOverlaySizing";
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
  const [livePreviewEnabled, setLivePreviewEnabled] = useState(false);
  // Default mirrors the Rust AppSettings default (false) so the quick-toggle
  // doesn't show the wrong state before settings load.
  const [noiseReduction, setNoiseReduction] = useState(false);
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

  const {
    previewText,
    modes,
    activeModId,
    setActiveModId,
    activeColor,
    setActiveColor,
    structuredPayload,
    setStructuredPayload,
    structuredDegraded,
    setStructuredDegraded,
  } = useOverlayEvents({
    status,
    dictatingInPanelRef,
    settingsRef,
    setShowModeSelector,
    setShowShipPopup,
  });

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

  // Load settings and stay in sync with changes from any window — one apply
  // callback wired through useSettingsSync instead of duplicated fetch +
  // listener bodies.
  const applySettings = useCallback(
    (s: AppSettings) => {
      replaceSettings(s);
      setLivePreviewEnabled(s.live_preview);
      setNoiseReduction(s.noise_reduction);
      setAutoSwitchModes(s.auto_switch_modes);
      setShipMode(s.ship_mode);
      setCommandSend(s.command_send);
      setGhostMode(s.ghost_mode);
      setStructuredMode(s.structured_mode);
      setStructuredVoiceCommand(s.structured_voice_command);
    },
    [replaceSettings]
  );
  useSettingsSync(applySettings);

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
          <QuickToggles
            autoSwitchModes={autoSwitchModes}
            livePreviewEnabled={livePreviewEnabled}
            noiseReduction={noiseReduction}
            shipMode={shipMode}
            commandSend={commandSend}
            ghostMode={ghostMode}
            structuredMode={structuredMode}
            structuredVoiceCommand={structuredVoiceCommand}
            showShipPopup={showShipPopup}
            showLeyLinePopup={showLeyLinePopup}
            setShowShipPopup={setShowShipPopup}
            setShowLeyLinePopup={setShowLeyLinePopup}
            onToggleAutoSwitch={handleToggleAutoSwitch}
            onToggleLivePreview={handleToggleLivePreview}
            onToggleNoiseReduction={handleToggleNoiseReduction}
            onToggleShipMode={handleToggleShipMode}
            onToggleCommandSend={handleToggleCommandSend}
            onToggleGhostMode={handleToggleGhostMode}
            onToggleStructuredMode={handleToggleStructuredMode}
            onToggleStructuredVoiceCommand={handleToggleStructuredVoiceCommand}
          />
        </div>
      )}

    <button
      onClick={handleClick}
      onContextMenu={handleContextMenu}
      disabled={isProcessing}
      style={{
        // Pill size comes from the shared geometry constants in
        // useOverlaySizing.ts so the window resize targets and the pill CSS
        // can never drift apart.  Idle = 42-px slit · menu-open = a thin
        // base slit the exact width of the mode-selector panel · active =
        // the snug recording pill (8px slack inside the ACTIVE_W window).
        width: showModeSelector ? MENU_PILL_W : isIdle ? IDLE_W : ACTIVE_PILL_W,
        height: showModeSelector || isIdle ? IDLE_H : ACTIVE_H,
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
        !showModeSelector && isIdle && "mb-[2px]",
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
        <PillContent
          showContent={showContent}
          isRecording={isRecording}
          isProcessing={isProcessing}
          isStructuring={isStructuring}
          isSuccess={isSuccess}
          isError={isError}
          duration={duration}
          previewText={previewText}
          flashText={flashText}
          modeColor={modeColor}
        />
      )}
    </button>
    </div>
  );
}
