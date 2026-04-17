import { useEffect, useState, useCallback, useRef } from "react";
import { Loader2, Eye, ShieldCheck, Layers, Rocket, Ghost, Send } from "lucide-react";
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
  getSettings,
  updateSettings,
  type AppSettings,
  type ContextMode,
} from "@/lib/tauri";
import { formatDuration, cn } from "@/lib/utils";
import { PillWaveform } from "./PillWaveform";
import { ModeSelector } from "./ModeSelector";

type PillState = RecordingStatus | "success";

// Map mode color names → CSS color values for waveform bars
const MODE_COLORS: Record<string, string> = {
  amber: "rgb(251,191,36)",
  blue: "rgb(96,165,250)",
  green: "rgb(52,211,153)",
  purple: "rgb(192,132,252)",
  red: "rgb(248,113,113)",
  cyan: "rgb(34,211,238)",
};

// Window sizes — button always fills the window 100%
const ACTIVE_W = 210;
const ACTIVE_H = 34;
const IDLE_W = 56;
const IDLE_H = 26;

export function FloatingPill() {
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const duration = useRecordingStore((s) => s.duration);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  const [pillState, setPillState] = useState<PillState>("idle");
  const [flashText, setFlashText] = useState<string | null>(null);
  const prevExpandedRef = useRef(false);
  const [showContent, setShowContent] = useState(false);

  // Live preview state
  const [previewText, setPreviewText] = useState<string | null>(null);
  const [livePreviewEnabled, setLivePreviewEnabled] = useState(false);
  const [noiseReduction, setNoiseReduction] = useState(true);
  const [autoSwitchModes, setAutoSwitchModes] = useState(true);
  const [shipMode, setShipMode] = useState(false);
  const [commandSend, setCommandSend] = useState(true);
  const [ghostMode, setGhostMode] = useState(false);
  const [showShipPopup, setShowShipPopup] = useState(false);

  // Mode selector state
  const [showModeSelector, setShowModeSelector] = useState(false);
  const [modes, setModes] = useState<ContextMode[]>([]);
  const [activeModId, setActiveModId] = useState<string | null>(null);
  const [activeColor, setActiveColor] = useState("amber");

  // In-memory mirror of the latest settings so toggles can read+write without
  // re-fetching from SQLite.  Updated on initial load AND whenever
  // onSettingsChanged fires (from any window).  The old code did
  // `const s = await getSettings(); await updateSettings({ ...s, key: next })`
  // for every toggle — two rapid toggles could race (A reads DB, B reads DB,
  // A writes { ...old, x:true }, B writes { ...old, y:true } overwriting x).
  // With a synchronously-updated ref, toggles never see stale data.
  const settingsRef = useRef<AppSettings | null>(null);

  useEffect(() => {
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
  }, [status, lastTranscription]);

  // Resize window on idle ↔ active transitions with content fade
  useEffect(() => {
    const expanded = pillState !== "idle";
    if (expanded !== prevExpandedRef.current) {
      prevExpandedRef.current = expanded;

      if (expanded) {
        // idle → active: expand window, then fade content in
        resizeOverlay(ACTIVE_W, ACTIVE_H);
        const t = setTimeout(() => setShowContent(true), 80);
        return () => clearTimeout(t);
      } else {
        // active → idle: fade content out, then shrink window
        setShowContent(false);
        const t = setTimeout(() => resizeOverlay(IDLE_W, IDLE_H), 200);
        return () => clearTimeout(t);
      }
    }
  }, [pillState]);

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
    resizeOverlay(IDLE_W, IDLE_H);
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
        settingsRef.current = s;
        setLivePreviewEnabled(s.live_preview);
        setNoiseReduction(s.noise_reduction);
        setAutoSwitchModes(s.auto_switch_modes);
        setShipMode(s.ship_mode);
        setCommandSend(s.command_send);
        setGhostMode(s.ghost_mode);
      })
      .catch(() => {});

    const unlistenPreview = onTranscriptionPreview((text) => {
      const trimmed = text.length > 30 ? "…" + text.slice(-29) : text;
      setPreviewText(trimmed);
    });

    // Stay in sync when settings change from the main window (or any window)
    const unlistenSettings = onSettingsChanged((s) => {
      settingsRef.current = s;
      setLivePreviewEnabled(s.live_preview);
      setNoiseReduction(s.noise_reduction);
      setAutoSwitchModes(s.auto_switch_modes);
      setShipMode(s.ship_mode);
      setCommandSend(s.command_send);
      setGhostMode(s.ghost_mode);
    });

    return () => {
      unlistenPreview.then((fn) => fn());
      unlistenSettings.then((fn) => fn());
    };
  }, []);

  // Apply a single-field change to the settings ref and push to the DB.
  // Synchronous ref update means back-to-back toggles never race.
  // Returns a Promise that rejects on DB failure so callers can revert local state.
  const applySettingPatch = useCallback(
    (patch: Partial<AppSettings>): Promise<void> => {
      const current = settingsRef.current;
      if (!current) return Promise.reject(new Error("settings not loaded"));
      const updated: AppSettings = { ...current, ...patch };
      settingsRef.current = updated;
      return updateSettings(updated).catch((e) => {
        // Revert the ref so a subsequent toggle sees consistent state.
        settingsRef.current = current;
        throw e;
      });
    },
    []
  );

  // Clear preview text when not recording
  useEffect(() => {
    if (status !== "recording") {
      setPreviewText(null);
    }
  }, [status]);

  // Manage overlay size when mode selector opens/closes.
  // Always pre-allocate width for the ship popup so toggling it is purely CSS
  // (no async window resize = no flash or clipping).
  // Layout math: toggle buttons at 50%+102px, 26px wide, popup 8px gap + 160px.
  // Popup right edge = 50% + 296px → window needs ≥ 600px so 300px ≥ 296px.
  useEffect(() => {
    if (showModeSelector) {
      const selectorH = Math.min(modes.length * 34 + 40 + 34, 240);
      resizeOverlay(600, ACTIVE_H + selectorH + 4);
    } else if (pillState === "idle") {
      resizeOverlay(IDLE_W, IDLE_H);
    }
  }, [showModeSelector, modes.length]);

  const handleToggleAutoSwitch = useCallback(async () => {
    const next = !autoSwitchModes;
    setAutoSwitchModes(next);
    try {
      await applySettingPatch({ auto_switch_modes: next });
    } catch {
      setAutoSwitchModes(!next);
    }
  }, [autoSwitchModes, applySettingPatch]);

  const handleToggleLivePreview = useCallback(async () => {
    const next = !livePreviewEnabled;
    setLivePreviewEnabled(next); // optimistic
    try {
      await applySettingPatch({ live_preview: next });
    } catch {
      setLivePreviewEnabled(!next); // revert on failure
    }
  }, [livePreviewEnabled, applySettingPatch]);

  const handleToggleNoiseReduction = useCallback(async () => {
    const next = !noiseReduction;
    setNoiseReduction(next); // optimistic
    try {
      await applySettingPatch({ noise_reduction: next });
    } catch {
      setNoiseReduction(!next); // revert on failure
    }
  }, [noiseReduction, applySettingPatch]);

  const handleToggleShipMode = useCallback(async () => {
    const next = !shipMode;
    setShipMode(next);
    try {
      await applySettingPatch({ ship_mode: next });
    } catch {
      setShipMode(!next);
    }
  }, [shipMode, applySettingPatch]);

  const handleToggleCommandSend = useCallback(async () => {
    const next = !commandSend;
    setCommandSend(next);
    try {
      await applySettingPatch({ command_send: next });
    } catch {
      setCommandSend(!next);
    }
  }, [commandSend, applySettingPatch]);

  const handleToggleGhostMode = useCallback(async () => {
    const next = !ghostMode;
    setGhostMode(next);
    // When activating ghost mode, close the menu so pill fades out
    if (next) {
      setShowModeSelector(false);
    }
    try {
      await applySettingPatch({ ghost_mode: next });
    } catch {
      setGhostMode(!next);
    }
  }, [ghostMode, applySettingPatch]);

  // Exit ghost mode — used when user clicks/right-clicks the invisible pill
  const exitGhostMode = useCallback(async () => {
    setGhostMode(false);
    try {
      await applySettingPatch({ ghost_mode: false });
    } catch {}
  }, [applySettingPatch]);

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
      if (pillState === "idle") {
        setShowModeSelector((prev) => {
          // Close ship popup when toggling mode selector
          if (!prev) setShowShipPopup(false);
          return !prev;
        });
      }
    },
    [pillState, ghostMode, exitGhostMode]
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
  const isSuccess = pillState === "success";
  const isError = pillState === "error";

  const modeColor = MODE_COLORS[activeColor] ?? MODE_COLORS.amber;

  return (
    <div className="w-screen h-screen flex flex-col justify-end items-center">
      {/* Mode selector dropdown — centered above the pill */}
      {showModeSelector && modes.length > 0 && (
        <div className="relative shrink-0 flex justify-center w-full">
          <ModeSelector
            modes={modes}
            activeId={activeModId}
            onSelect={handleModeSelect}
            onClose={() => {
              setShowModeSelector(false);
              setShowShipPopup(false);
            }}
          />
          {/* Quick-toggle circles — frosted glass, lower-right of menu */}
          {/* Uses mousedown for toggle action since the overlay is transparent
              and click events can be swallowed at window edges in WebView2 */}
          <div
            className="absolute flex flex-col gap-1.5"
            style={{
              left: "calc(50% + 96px + 6px)",
              bottom: "46px",
            }}
          >
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleAutoSwitch();
              }}
              title={autoSwitchModes ? "Auto context switch: on" : "Auto context switch: off"}
              className={cn(
                "w-[26px] h-[26px] rounded-full flex items-center justify-center",
                "backdrop-blur-md border transition-all duration-150",
                autoSwitchModes
                  ? "border-amber-500/20 shadow-[0_0_8px_rgba(0,0,0,0.15)]"
                  : "border-transparent opacity-50 hover:opacity-80"
              )}
              style={{ backgroundColor: autoSwitchModes ? "rgba(180,120,20,0.75)" : "rgba(180,120,20,0.25)" }}
            >
              <Layers size={12} strokeWidth={2} className="text-white drop-shadow-sm" />
            </button>
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleLivePreview();
              }}
              title={livePreviewEnabled ? "Live preview: on" : "Live preview: off"}
              className={cn(
                "w-[26px] h-[26px] rounded-full flex items-center justify-center",
                "backdrop-blur-md border transition-all duration-150",
                livePreviewEnabled
                  ? "border-amber-500/20 shadow-[0_0_8px_rgba(0,0,0,0.15)]"
                  : "border-transparent opacity-50 hover:opacity-80"
              )}
              style={{ backgroundColor: livePreviewEnabled ? "rgba(180,120,20,0.75)" : "rgba(180,120,20,0.25)" }}
            >
              <Eye size={12} strokeWidth={2} className="text-white drop-shadow-sm" />
            </button>
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleNoiseReduction();
              }}
              title={noiseReduction ? "Noise suppression: on" : "Noise suppression: off"}
              className={cn(
                "w-[26px] h-[26px] rounded-full flex items-center justify-center",
                "backdrop-blur-md border transition-all duration-150",
                noiseReduction
                  ? "border-amber-500/20 shadow-[0_0_8px_rgba(0,0,0,0.15)]"
                  : "border-transparent opacity-50 hover:opacity-80"
              )}
              style={{ backgroundColor: noiseReduction ? "rgba(180,120,20,0.75)" : "rgba(180,120,20,0.25)" }}
            >
              <ShieldCheck size={12} strokeWidth={2} className="text-white drop-shadow-sm" />
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
                  "w-[26px] h-[26px] rounded-full flex items-center justify-center",
                  "backdrop-blur-md border transition-all duration-150",
                  shipMode
                    ? "border-amber-500/20 shadow-[0_0_8px_rgba(0,0,0,0.15)]"
                    : "border-transparent opacity-50 hover:opacity-80"
                )}
                style={{ backgroundColor: shipMode ? "rgba(180,120,20,0.75)" : "rgba(180,120,20,0.25)" }}
              >
                <Rocket size={12} strokeWidth={2} className="text-white drop-shadow-sm" />
              </button>

              {/* ── Ship button right-click popup ── */}
              <div
                className="absolute z-50 backdrop-blur-xl border border-white/10 rounded-lg shadow-2xl pointer-events-none"
                style={{
                  left: "calc(100% + 8px)",
                  top: "50%",
                  transform: "translateY(-50%)",
                  backgroundColor: "rgba(28, 26, 24, 0.92)",
                  minWidth: 160,
                  padding: "8px 10px",
                  opacity: showShipPopup ? 1 : 0,
                  scale: showShipPopup ? "1" : "0.92",
                  pointerEvents: showShipPopup ? "auto" : "none",
                  transition: "opacity 0.15s ease-out, scale 0.15s ease-out",
                  transformOrigin: "left center",
                }}
                onMouseDown={(e) => e.stopPropagation()}
              >
                <div className="flex items-center gap-1.5 mb-2">
                  <Send size={10} strokeWidth={2} className="text-amber-400/80" />
                  <span className="text-[10px] font-semibold text-amber-400/90 uppercase tracking-wider">
                    Command Send
                  </span>
                </div>
                <p className="text-[9px] text-white/40 mb-2.5 leading-tight">
                  Say "send" to submit instead of auto-sending everything
                </p>
                <div className="flex items-center gap-2">
                  <button
                    onMouseDown={(e) => {
                      e.stopPropagation();
                      e.preventDefault();
                      handleToggleCommandSend();
                    }}
                    className={cn(
                      "relative inline-flex h-4 w-7 items-center rounded-full transition-colors",
                      commandSend ? "bg-amber-500" : "bg-white/15"
                    )}
                  >
                    <span
                      className={cn(
                        "inline-block h-2.5 w-2.5 rounded-full bg-white transition-transform",
                        commandSend ? "translate-x-[14px]" : "translate-x-0.5"
                      )}
                    />
                  </button>
                  <span className="text-[10px] text-white/60">
                    {commandSend ? "On" : "Off"}
                  </span>
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
              backgroundColor: "rgba(255,255,255,0.15)",
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
              title={ghostMode ? "Ghost mode: on — pill hidden" : "Ghost mode: off"}
              className={cn(
                "w-[26px] h-[26px] rounded-full flex items-center justify-center",
                "backdrop-blur-md border transition-all duration-150",
                ghostMode
                  ? "border-white/10 shadow-[0_0_8px_rgba(0,0,0,0.15)]"
                  : "border-transparent opacity-50 hover:opacity-80"
              )}
              style={{ backgroundColor: "var(--color-pill-bg)" }}
            >
              <Ghost size={12} strokeWidth={2} className="text-white/50 drop-shadow-sm" />
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
        transition: "opacity 0.25s ease",
      }}
      className={cn(
        // The pill — sized to match resizeOverlay dimensions
        isIdle && !showModeSelector ? "w-[56px] h-[26px]" : "w-[200px] h-[34px]",
        "relative flex items-center overflow-hidden shrink-0",
        isProcessing ? "cursor-default" : "cursor-pointer",

        // Idle
        isIdle && "bg-[var(--color-pill-bg)] rounded-full",

        // Recording
        isRecording && "bg-[var(--color-pill-bg)] border border-recording-500/30 rounded-full gap-2.5 px-3.5",

        // Processing
        isProcessing && "bg-[var(--color-pill-bg)] border border-amber-500/25 rounded-full gap-2.5 px-3.5",

        // Success
        isSuccess && "bg-[var(--color-pill-bg)] border border-success/30 rounded-full gap-2.5 px-3.5",

        // Error
        isError && "bg-[var(--color-pill-bg)] border border-recording-500/35 rounded-full gap-2.5 px-3.5",
      )}
    >
      {/* ── Idle: sleek ambient waveform ── */}
      {isIdle && <IdleWaveform color={modeColor} />}

      {/* ── Active states: full pill content with fade ── */}
      {!isIdle && (
        <div
          className="flex items-center w-full h-full gap-2.5"
          style={{
            opacity: showContent ? 1 : 0,
            transition: "opacity 0.2s ease",
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
                    "linear-gradient(90deg, transparent 0%, oklch(0.65 0.16 55 / 0.06) 50%, transparent 100%)",
                  animation: "shimmer 2s ease-in-out infinite",
                }}
              />
            </div>
          )}

          {/* Left: timer / spinner / icon */}
          <div className="shrink-0 flex items-center justify-center min-w-[34px]">
            {isRecording && (
              <span className="font-mono text-[11px] tabular-nums text-recording-300/80 tracking-wide">
                {formatDuration(duration)}
              </span>
            )}
            {isProcessing && (
              <Loader2
                size={12}
                className="text-amber-400/70 animate-spin"
                strokeWidth={2.5}
              />
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
              <span
                className="text-[10px] truncate font-normal tracking-tight"
                style={{ color: modeColor, opacity: 0.7 }}
              >
                {previewText}
              </span>
            )}
            {isRecording && !previewText && <PillWaveform active color={modeColor} />}
            {isProcessing && (
              <span className="text-[10px] font-medium text-amber-400/60 tracking-wide truncate">
                Transcribing…
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
          <div className="shrink-0 w-[20px] flex items-center justify-end">
            {isRecording && (
              <div className="relative flex items-center justify-center">
                <span
                  className="absolute h-3.5 w-3.5 rounded-full bg-recording-500/15"
                  style={{ animation: "recording-pulse 2s ease-in-out infinite" }}
                />
                <span className="relative h-1.5 w-1.5 rounded-full bg-recording-500 shadow-[0_0_6px_rgba(180,50,40,0.4)]" />
              </div>
            )}
            {isProcessing && (
              <div className="h-1.5 w-1.5 rounded-full bg-amber-500/40" />
            )}
            {isSuccess && (
              <div className="h-1.5 w-1.5 rounded-full bg-success/40" />
            )}
          </div>
        </div>
      )}

      <style>{`
        @keyframes shimmer {
          0% { transform: translateX(-100%); }
          100% { transform: translateX(200%); }
        }
      `}</style>
    </button>
    </div>
  );
}

/* ── Idle waveform: subtle ambient bars ── */
function IdleWaveform({ color }: { color: string }) {
  const BAR_COUNT = 5;

  return (
    <div className="flex items-center justify-center gap-[3px] w-full h-full">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          className="rounded-full"
          style={{
            width: 2,
            height: 10,
            backgroundColor: color,
            opacity: 0.25,
            willChange: "transform, opacity",
            animation: `idle-wave 2.4s ease-in-out ${i * 0.18}s infinite`,
          }}
        />
      ))}
      <style>{`
        @keyframes idle-wave {
          0%, 100% { transform: scaleY(0.4); opacity: 0.15; }
          50% { transform: scaleY(1); opacity: 0.35; }
        }
      `}</style>
    </div>
  );
}
