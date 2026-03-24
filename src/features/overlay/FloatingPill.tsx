import { useEffect, useState, useCallback, useRef } from "react";
import { Loader2, Eye, ShieldCheck, Layers } from "lucide-react";
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

  // Mode selector state
  const [showModeSelector, setShowModeSelector] = useState(false);
  const [modes, setModes] = useState<ContextMode[]>([]);
  const [activeModId, setActiveModId] = useState<string | null>(null);
  const [activeColor, setActiveColor] = useState("amber");

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
        setLivePreviewEnabled(s.live_preview);
        setNoiseReduction(s.noise_reduction);
        setAutoSwitchModes(s.auto_switch_modes);
      })
      .catch(() => {});

    const unlistenPreview = onTranscriptionPreview((text) => {
      const trimmed = text.length > 30 ? "…" + text.slice(-29) : text;
      setPreviewText(trimmed);
    });

    // Stay in sync when settings change from the main window (or any window)
    const unlistenSettings = onSettingsChanged((s) => {
      setLivePreviewEnabled(s.live_preview);
      setNoiseReduction(s.noise_reduction);
      setAutoSwitchModes(s.auto_switch_modes);
    });

    return () => {
      unlistenPreview.then((fn) => fn());
      unlistenSettings.then((fn) => fn());
    };
  }, []);

  // Clear preview text when not recording
  useEffect(() => {
    if (status !== "recording") {
      setPreviewText(null);
    }
  }, [status]);

  // Manage overlay size when mode selector opens/closes
  useEffect(() => {
    if (showModeSelector) {
      // Expand window to fit selector + pill + side toggle buttons
      // Menu w-48 (192px) centered + 6px gap + 26px buttons + padding = ~260px
      const selectorH = Math.min(modes.length * 34 + 40 + 34, 240);
      resizeOverlay(260, ACTIVE_H + selectorH + 4);
    } else if (pillState === "idle") {
      resizeOverlay(IDLE_W, IDLE_H);
    }
  }, [showModeSelector, modes.length]);

  const handleToggleAutoSwitch = useCallback(async () => {
    const next = !autoSwitchModes;
    setAutoSwitchModes(next);
    try {
      const s = await getSettings();
      await updateSettings({ ...s, auto_switch_modes: next });
    } catch {
      setAutoSwitchModes(!next);
    }
  }, [autoSwitchModes]);

  const handleToggleLivePreview = useCallback(async () => {
    const next = !livePreviewEnabled;
    setLivePreviewEnabled(next); // optimistic
    try {
      const s = await getSettings();
      await updateSettings({ ...s, live_preview: next });
    } catch {
      setLivePreviewEnabled(!next); // revert on failure
    }
  }, [livePreviewEnabled]);

  const handleToggleNoiseReduction = useCallback(async () => {
    const next = !noiseReduction;
    setNoiseReduction(next); // optimistic
    try {
      const s = await getSettings();
      await updateSettings({ ...s, noise_reduction: next });
    } catch {
      setNoiseReduction(!next); // revert on failure
    }
  }, [noiseReduction]);

  const handleClick = useCallback(async () => {
    if (showModeSelector) return; // Don't start recording while selector is open
    try {
      if (status === "idle") await startRecording();
      else if (status === "recording") await stopRecording();
    } catch (err) {
      console.error("Pill recording toggle failed:", err);
    }
  }, [status, showModeSelector]);

  const handleContextMenu = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      if (pillState === "idle") {
        setShowModeSelector((prev) => !prev);
      }
    },
    [pillState]
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
            onClose={() => setShowModeSelector(false)}
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
                  ? "bg-white/[0.14] border-white/20 shadow-[0_0_8px_rgba(255,255,255,0.06)]"
                  : "bg-white/[0.06] border-white/10 opacity-50 hover:opacity-80"
              )}
            >
              <Layers size={12} strokeWidth={2} className={autoSwitchModes ? "text-white/80" : "text-white/40"} />
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
                  ? "bg-white/[0.14] border-white/20 shadow-[0_0_8px_rgba(255,255,255,0.06)]"
                  : "bg-white/[0.06] border-white/10 opacity-50 hover:opacity-80"
              )}
            >
              <Eye size={12} strokeWidth={2} className={livePreviewEnabled ? "text-white/80" : "text-white/40"} />
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
                  ? "bg-white/[0.14] border-white/20 shadow-[0_0_8px_rgba(255,255,255,0.06)]"
                  : "bg-white/[0.06] border-white/10 opacity-50 hover:opacity-80"
              )}
            >
              <ShieldCheck size={12} strokeWidth={2} className={noiseReduction ? "text-white/80" : "text-white/40"} />
            </button>
          </div>
        </div>
      )}

    <button
      onClick={handleClick}
      onContextMenu={handleContextMenu}
      disabled={isProcessing}
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
            height: 6,
            backgroundColor: color,
            opacity: 0.25,
            animation: `idle-wave 2s ease-in-out ${i * 0.2}s infinite`,
          }}
        />
      ))}
      <style>{`
        @keyframes idle-wave {
          0%, 100% { height: 4px; opacity: 0.15; }
          50% { height: 10px; opacity: 0.35; }
        }
      `}</style>
    </div>
  );
}
