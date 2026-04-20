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
  getSettings,
  updateSettings,
  type AppSettings,
  type ContextMode,
  type StructuredOutputPayload,
} from "@/lib/tauri";
import { formatDuration, cn } from "@/lib/utils";
import { PillWaveform } from "./PillWaveform";
import { ModeSelector } from "./ModeSelector";
import { StructuredPanel } from "./StructuredPanel";
import { StructuredModeToggle } from "./StructuredModeToggle";

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
  const [structuredMode, setStructuredMode] = useState(false);
  const [showShipPopup, setShowShipPopup] = useState(false);

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
  const dictatingGraceTimerRef = useRef<number | null>(null);
  const handleDictatingChange = useCallback((active: boolean) => {
    if (dictatingGraceTimerRef.current !== null) {
      window.clearTimeout(dictatingGraceTimerRef.current);
      dictatingGraceTimerRef.current = null;
    }
    if (active) {
      dictatingInPanelRef.current = true;
    } else {
      dictatingGraceTimerRef.current = window.setTimeout(() => {
        dictatingInPanelRef.current = false;
        dictatingGraceTimerRef.current = null;
      }, 600);
    }
  }, []);

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
    const expanded =
      pillState !== "idle" ||
      !!structuredPayload ||
      !!structuredDegraded ||
      showModeSelector;
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
  }, [pillState, structuredPayload, structuredDegraded, showModeSelector]);

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
        setStructuredMode(s.structured_mode);
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
      setStructuredMode(s.structured_mode);
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
      window.setTimeout(() => setStructuredDegraded(null), 15000);
    });

    return () => {
      unlistenPreview.then((fn) => fn());
      unlistenSettings.then((fn) => fn());
      unlistenStructured.then((fn) => fn());
      unlistenDegraded.then((fn) => fn());
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
    if (structuredPayload) {
      // Panel dimensions: 420 wide, up to ~450 tall (preview + raw + actions).
      // Leave generous headroom — panel itself caps its own scroll area.
      resizeOverlay(440, 480);
    } else if (structuredDegraded) {
      // Banner needs a wider + taller window than idle, otherwise it's clipped
      // by the 56×26 overlay and the user never sees the failure reason.
      resizeOverlay(420, ACTIVE_H + 80);
    } else if (showModeSelector) {
      const selectorH = Math.min(modes.length * 34 + 40 + 34, 240);
      resizeOverlay(600, ACTIVE_H + selectorH + 4);
    } else if (pillState === "idle") {
      resizeOverlay(IDLE_W, IDLE_H);
    }
  }, [showModeSelector, modes.length, structuredPayload, structuredDegraded, pillState]);

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

  const handleToggleStructuredMode = useCallback(async () => {
    const next = !structuredMode;
    setStructuredMode(next); // optimistic — UI transitions immediately
    try {
      await applySettingPatch({ structured_mode: next });
    } catch {
      setStructuredMode(!next);
    }
  }, [structuredMode, applySettingPatch]);

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
  const isStructuring = pillState === "structuring";
  const isSuccess = pillState === "success";
  const isError = pillState === "error";

  const modeColor = MODE_COLORS[activeColor] ?? MODE_COLORS.amber;

  return (
    <div className="w-screen h-screen flex flex-col justify-end items-center">
      {/* Structured Mode panel — sits flush on top of the pill, forming a
          single unified surface.  Zero bottom margin is deliberate (the
          "reverse Dynamic Island" expansion effect): the panel's flat
          bottom merges visually into the pill's rounded top so they read
          as one connected shape instead of two floating bubbles. */}
      {structuredPayload && !ghostMode && (
        <div className="shrink-0">
          <StructuredPanel
            payload={structuredPayload}
            onClose={() => setStructuredPayload(null)}
            onDictatingChange={handleDictatingChange}
          />
        </div>
      )}

      {/* Transient degraded banner — LLM timed out / not loaded */}
      {structuredDegraded && !structuredPayload && !ghostMode && (
        <div
          className="mb-1.5 shrink-0 flex items-center gap-2 px-3 py-1.5 rounded-lg max-w-[380px] cursor-pointer group"
          onClick={() => setStructuredDegraded(null)}
          title="Click to dismiss"
          style={{
            background:
              "linear-gradient(180deg, rgba(60,42,22,0.92) 0%, rgba(42,30,18,0.92) 100%)",
            border: "1px solid rgba(232,180,95,0.28)",
            boxShadow:
              "inset 0 1px 0 rgba(255,225,175,0.08), 0 6px 18px -6px rgba(0,0,0,0.7), 0 0 20px -8px rgba(232,180,95,0.3)",
            animation: "sp-in 220ms cubic-bezier(0.16,1,0.3,1) both",
          }}
        >
          <span
            aria-hidden="true"
            className="h-1.5 w-1.5 rounded-full shrink-0"
            style={{
              backgroundColor: "rgba(244,190,110,0.95)",
              boxShadow: "0 0 6px rgba(244,190,110,0.7)",
            }}
          />
          <span
            className="text-[9px] font-semibold uppercase tracking-[0.18em] shrink-0"
            style={{
              fontFamily: "var(--font-display)",
              color: "rgba(244,200,130,0.9)",
            }}
          >
            Structured
          </span>
          <span
            className="text-[10px] leading-snug truncate"
            style={{
              color: "rgba(248,215,170,0.88)",
              letterSpacing: "-0.005em",
            }}
          >
            {structuredDegraded}
          </span>
        </div>
      )}

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
            <StructuredModeToggle
              active={structuredMode}
              onToggle={handleToggleStructuredMode}
            />
            <button
              onMouseDown={(e) => {
                e.stopPropagation();
                e.preventDefault();
                handleToggleAutoSwitch();
              }}
              title={autoSwitchModes ? "Auto context switch: on" : "Auto context switch: off"}
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
              title={livePreviewEnabled ? "Live preview: on" : "Live preview: off"}
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
              title={noiseReduction ? "Noise suppression: on" : "Noise suppression: off"}
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
              title={ghostMode ? "Ghost mode: on — pill hidden" : "Ghost mode: off"}
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

        // Structuring (Structured Mode — LLM slot extraction in flight)
        isStructuring && "bg-[var(--color-pill-bg)] border border-violet-400/30 rounded-full gap-2.5 px-3.5",

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
            {isStructuring && (
              <span className="relative flex items-center justify-center">
                <span
                  aria-hidden="true"
                  className="absolute h-[18px] w-[18px] rounded-full"
                  style={{
                    background:
                      "radial-gradient(circle, rgba(186,148,234,0.35) 0%, rgba(186,148,234,0) 70%)",
                    animation: "structuring-halo 1.8s ease-in-out infinite",
                  }}
                />
                <Sparkles
                  size={12}
                  className="relative text-violet-300"
                  strokeWidth={2.5}
                  style={{
                    animation: "structuring-spark 2.2s ease-in-out infinite",
                    filter:
                      "drop-shadow(0 0 3px rgba(186,148,234,0.5))",
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
            {isStructuring && (
              <span
                className="text-[10px] font-medium tracking-[0.14em] uppercase truncate"
                style={{
                  fontFamily: "var(--font-display)",
                  background:
                    "linear-gradient(90deg, rgba(186,148,234,0.35) 0%, rgba(218,195,244,0.95) 50%, rgba(186,148,234,0.35) 100%)",
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
            {isStructuring && (
              <div className="relative flex items-center justify-center">
                <span
                  className="absolute h-3 w-3 rounded-full"
                  style={{
                    background:
                      "radial-gradient(circle, rgba(186,148,234,0.35) 0%, rgba(186,148,234,0) 70%)",
                    animation: "structuring-halo 1.8s ease-in-out infinite",
                  }}
                />
                <span
                  className="relative h-1.5 w-1.5 rounded-full"
                  style={{
                    backgroundColor: "rgb(186,148,234)",
                    boxShadow: "0 0 6px rgba(186,148,234,0.6)",
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

      <style>{`
        /* ── Quick-toggle circles ── */
        .quick-toggle {
          width: 26px;
          height: 26px;
          border-radius: 999px;
          display: flex;
          align-items: center;
          justify-content: center;
          backdrop-filter: blur(10px);
          background: linear-gradient(180deg,
            rgba(148,98,18,0.28) 0%,
            rgba(130,84,14,0.22) 100%);
          border: 1px solid transparent;
          cursor: pointer;
          opacity: 0.55;
          transition:
            background 180ms ease,
            border-color 180ms ease,
            box-shadow 220ms ease,
            opacity 160ms ease,
            transform 140ms ease;
        }
        .quick-toggle:hover {
          opacity: 0.88;
          background: linear-gradient(180deg,
            rgba(164,112,26,0.36) 0%,
            rgba(144,94,18,0.28) 100%);
        }
        .quick-toggle:active { transform: scale(0.92); }
        .quick-toggle--on {
          opacity: 1;
          background: linear-gradient(180deg,
            rgba(200,142,36,0.86) 0%,
            rgba(168,112,22,0.78) 100%);
          border-color: rgba(255,220,160,0.22);
          box-shadow:
            inset 0 1px 0 rgba(255,230,190,0.18),
            inset 0 -1px 0 rgba(0,0,0,0.12),
            0 0 10px -2px rgba(232,180,95,0.5),
            0 2px 8px -4px rgba(0,0,0,0.55);
        }
        .quick-toggle--on:hover {
          background: linear-gradient(180deg,
            rgba(214,152,44,0.94) 0%,
            rgba(180,120,26,0.84) 100%);
          box-shadow:
            inset 0 1px 0 rgba(255,230,190,0.22),
            inset 0 -1px 0 rgba(0,0,0,0.12),
            0 0 14px -2px rgba(232,180,95,0.65),
            0 2px 10px -4px rgba(0,0,0,0.6);
        }
        .quick-toggle-icon {
          color: rgba(255,255,255,0.9);
          filter: drop-shadow(0 0 2px rgba(0,0,0,0.35));
        }
        .quick-toggle--ghost {
          background: linear-gradient(180deg,
            rgba(40,38,36,0.92) 0%,
            rgba(28,26,24,0.92) 100%);
          border: 1px solid rgba(255,255,255,0.04);
        }
        .quick-toggle--ghost:hover {
          background: linear-gradient(180deg,
            rgba(54,50,46,0.95) 0%,
            rgba(38,34,32,0.95) 100%);
          border-color: rgba(255,255,255,0.08);
        }
        .quick-toggle--ghost-on {
          opacity: 1;
          border-color: rgba(255,255,255,0.14);
          box-shadow:
            inset 0 1px 0 rgba(255,255,255,0.05),
            0 2px 8px -3px rgba(0,0,0,0.6);
        }
        .quick-toggle--ghost .quick-toggle-icon {
          color: rgba(255,255,255,0.5);
        }

        /* ── Ship popup ── */
        .ship-popup {
          position: absolute;
          z-index: 50;
          border-radius: 10px;
          padding: 0;
          background: linear-gradient(180deg,
            rgba(28,26,24,0.96) 0%,
            rgba(22,20,18,0.96) 100%);
          border: 1px solid rgba(255,255,255,0.06);
          box-shadow:
            inset 0 1px 0 rgba(255,255,255,0.05),
            0 1px 2px rgba(0,0,0,0.5),
            0 8px 18px -4px rgba(0,0,0,0.7),
            0 16px 32px -12px rgba(0,0,0,0.8);
          backdrop-filter: blur(14px);
          overflow: hidden;
          transform-origin: left center;
          transition:
            opacity 160ms ease-out,
            transform 180ms cubic-bezier(0.16, 1, 0.3, 1);
        }
        .ship-popup-bloom {
          position: absolute;
          inset: -60% -20% auto -20%;
          height: 80px;
          background: radial-gradient(ellipse at 50% 0%,
            rgba(255,230,180,0.06) 0%,
            rgba(255,220,160,0.02) 30%,
            transparent 65%);
          pointer-events: none;
          z-index: 0;
        }
        .ship-popup-ring {
          position: absolute;
          top: 0; left: 0; right: 0;
          height: 1px;
          background: linear-gradient(90deg,
            transparent 0%,
            rgba(255,230,190,0.18) 50%,
            transparent 100%);
          pointer-events: none;
          z-index: 2;
        }
        .ship-popup-content {
          position: relative;
          z-index: 3;
          padding: 9px 11px 10px;
        }
        .ship-popup-header {
          display: flex;
          align-items: center;
          gap: 6px;
          margin-bottom: 6px;
        }
        .ship-popup-icon {
          color: rgba(244,190,110,0.9);
          filter: drop-shadow(0 0 2px rgba(244,190,110,0.35));
        }
        .ship-popup-kicker {
          font-family: var(--font-display);
          font-size: 9px;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.2em;
          color: rgba(240,218,182,0.92);
        }
        .ship-popup-desc {
          margin: 0 0 9px;
          font-family: var(--font-sans);
          font-size: 9.5px;
          line-height: 1.4;
          color: rgba(255,255,255,0.42);
          letter-spacing: -0.005em;
        }
        .ship-popup-row {
          display: flex;
          align-items: center;
          gap: 8px;
        }
        .ship-popup-switch {
          position: relative;
          display: inline-flex;
          align-items: center;
          width: 26px;
          height: 14px;
          border-radius: 999px;
          background: rgba(255,255,255,0.12);
          border: 1px solid rgba(255,255,255,0.04);
          cursor: pointer;
          transition: background 160ms ease, border-color 160ms ease;
          padding: 0;
        }
        .ship-popup-switch--on {
          background: linear-gradient(180deg,
            rgba(208,148,40,0.95) 0%,
            rgba(176,118,22,0.9) 100%);
          border-color: rgba(255,220,160,0.28);
          box-shadow:
            inset 0 1px 0 rgba(255,230,190,0.2),
            0 0 8px -2px rgba(232,180,95,0.55);
        }
        .ship-popup-knob {
          display: inline-block;
          position: absolute;
          top: 50%;
          left: 2px;
          width: 10px;
          height: 10px;
          border-radius: 50%;
          background: rgba(255,255,255,0.92);
          box-shadow:
            0 1px 2px rgba(0,0,0,0.3);
          transform: translateY(-50%) translateX(0);
          transition: transform 180ms cubic-bezier(0.4, 0, 0.2, 1);
        }
        .ship-popup-switch--on .ship-popup-knob {
          transform: translateY(-50%) translateX(12px);
        }
        .ship-popup-state {
          font-family: var(--font-sans);
          font-size: 10px;
          font-weight: 500;
          color: rgba(255,255,255,0.62);
          letter-spacing: -0.005em;
        }

        @keyframes shimmer {
          0% { transform: translateX(-100%); }
          100% { transform: translateX(200%); }
        }
        @keyframes structuring-halo {
          0%, 100% { transform: scale(0.85); opacity: 0.5; }
          50% { transform: scale(1.15); opacity: 1; }
        }
        @keyframes structuring-pulse {
          0%, 100% {
            transform: scale(0.92);
            box-shadow: 0 0 4px rgba(186,148,234,0.45);
          }
          50% {
            transform: scale(1.08);
            box-shadow: 0 0 10px rgba(186,148,234,0.8);
          }
        }
        @keyframes structuring-spark {
          0%, 100% { transform: scale(0.94) rotate(-6deg); opacity: 0.85; }
          50% { transform: scale(1.08) rotate(6deg); opacity: 1; }
        }
        @keyframes structuring-shimmer {
          0% { background-position: 220% 0; }
          100% { background-position: -220% 0; }
        }
        @keyframes structuring-dot {
          0%, 100% { opacity: 0.25; }
          50% { opacity: 1; }
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
