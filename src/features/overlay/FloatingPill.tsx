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
  StickyNote,
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

// Window sizes — button always fills the window 100% (except in hover-idle,
// where the window widens rightward to reveal the scratchpad trigger circle
// while the pill stays anchored at screen-center via the outer flex layout).
//
// State ladder:
//   slim-idle    window 96 × 26   pill 60 × 10 at bottom-centre (rest is a
//                                 transparent hover hit-zone so the cursor
//                                 doesn't need pixel-perfect aim on the slit)
//   hover-idle   window 112 × 26  pill 60 × 26, waveform fades in, trigger right
//   menu         window 600 × ~240, pill 200 × 26
//   active       window 210 × 34, pill 200 × 34
//
// IDLE_W/IDLE_H are the slim-idle WINDOW dimensions, not the pill's visible
// size — the pill stays 60 × 10 in CSS and the surrounding ~18 px of width
// + 16 px of height above it is transparent.  Cursor anywhere in the
// window triggers hover via the outer div's onMouseEnter.  Trade-off: that
// transparent margin intercepts clicks meant for apps behind the overlay,
// but the region is tiny (96 × 26 px just above the taskbar) where no app
// typically has clickable UI.
const ACTIVE_W = 210;
const ACTIVE_H = 34;
const IDLE_W = 96;
const IDLE_H = 26;
// Hover-idle window math: pill 60 stays centred, gap 4 + trigger 22 to the
// right → pill right edge at +30, trigger right edge at +56.  Window must
// span ±56 around pill-centre → width 112.  The .scratchpad-trigger CSS
// pins the trigger at `left: calc(50% + 34px)` so these dimensions must
// stay in sync if you change one.
const HOVER_IDLE_W = 112;
const HOVER_IDLE_H = 26;
const MENU_PILL_H = 26;
const HOVER_DWELL_MS = 100;
const HOVER_LEAVE_MS = 120;
// Motion budget — single source of truth for the choreography.
//   PILL_MORPH_MS: pill width/height CSS transition, the "spine" of every
//     transition.  All other timings are calibrated against this.
//   SHRINK_RESIZE_DELAY_MS: how long the window stays at its old (larger)
//     size while the pill morphs smaller.  Must equal PILL_MORPH_MS so the
//     pill is never larger than the window it lives in (= no edge-clipping).
//   CONTENT_FADE_IN_DELAY_MS: how long active/menu/panel content waits
//     before fading in after a grow.  Past the WebView2 composition race.
const PILL_MORPH_MS = 240;
const SHRINK_RESIZE_DELAY_MS = 240;
const CONTENT_FADE_IN_DELAY_MS = 80;

export function FloatingPill() {
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const duration = useRecordingStore((s) => s.duration);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  const [pillState, setPillState] = useState<PillState>("idle");
  const [flashText, setFlashText] = useState<string | null>(null);
  const prevExpandedRef = useRef(false);
  const [showContent, setShowContent] = useState(false);

  // Hover-idle: cursor over the overlay window while in idle state.  Activates
  // after HOVER_DWELL_MS continuous dwell (filters fly-by cursor passes),
  // collapses after HOVER_LEAVE_MS off-window (filters brief gaps between
  // pill and trigger circle).  Drives both the pill's height bump
  // (22→26) and the scratchpad trigger circle's appearance.
  const [isHovering, setIsHovering] = useState(false);
  const hoverEnterTimerRef = useRef<number | null>(null);
  const hoverLeaveTimerRef = useRef<number | null>(null);

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
  const degradedTimerRef = useRef<number | null>(null);
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

  // Consolidated overlay sizing.
  //
  // Prior version had TWO effects that both reacted to the same state
  // transitions and called resizeOverlay independently.  When the user
  // right-clicked to open the mode selector, effect #1 issued
  // resizeOverlay(ACTIVE_W, ACTIVE_H) (210×34) and effect #2 issued
  // resizeOverlay(600, ACTIVE_H+selectorH+4) in the same tick.  Both
  // go through Tauri IPC; under load the first one could finish AFTER
  // the second, leaving the overlay at 210×34 with the mode selector
  // rendered but clipped to a one-line-tall window — "pill expands
  // horizontally but no menu".  Collapsing into a single effect
  // guarantees exactly one resize per state transition.
  //
  // `showContent` is reset to false on EVERY size change (not just
  // idle↔expanded), then flipped back to true 80 ms later.  This
  // masks a one-frame WebView2 composition race: SetWindowPos resizes
  // the window atomically on the Windows thread, but WebView2 can
  // paint the pre-resize React layout into the new window bounds for
  // a single frame before re-laying-out — showing the pill/menu at
  // the top-left of the expanded region.  Hiding content for 80 ms
  // (which also gates ModeSelector / StructuredPanel / degraded
  // banner mounts below) skips past that race.  200 ms out for
  // expanded→idle so the fade-out completes before the window shrinks.
  const prevTargetRef = useRef<{ w: number; h: number }>({ w: IDLE_W, h: IDLE_H });
  // Pending showContent timer lives in a ref rather than being released by
  // the effect's cleanup.  Reason: the effect's deps include `pillState`,
  // which changes AFTER `structuredPayload` is set (the pipeline emits
  // `structured-output-ready` and then `recording-state-change: idle`
  // back-to-back).  The second re-run sees `sizeChanged === false` and
  // returns early — but React has already invoked the first run's cleanup,
  // which would `clearTimeout` the pending `setShowContent(true)` call.
  // The panel then stays unmounted forever because `showContent` is stuck
  // at false.  Owning the timer manually means incidental re-runs no
  // longer nuke an in-flight show.
  const showContentTimerRef = useRef<number | null>(null);
  // Companion to showContentTimerRef.  Holds the pending shrink-resize call
  // when transitioning to a smaller size: we delay resizeOverlay() until the
  // pill's CSS morph has had time to finish, so the pill is never bigger
  // than the window for even one frame.  Cancelled (alongside showContent's
  // timer) on every effect re-run so the latest state wins.
  const shrinkResizeTimerRef = useRef<number | null>(null);
  useEffect(() => {
    let targetW: number;
    let targetH: number;
    if (structuredPayload) {
      // Panel dimensions: 420 wide, up to ~450 tall (preview + raw + actions).
      targetW = 440;
      targetH = 480;
    } else if (structuredDegraded) {
      // Banner needs a wider + taller window than idle, otherwise it's
      // clipped by the 56×26 overlay and the user never sees the reason.
      targetW = 420;
      targetH = ACTIVE_H + 80;
    } else if (showModeSelector) {
      // Popup width math: toggle buttons at 50%+102px, popup 8px+160px →
      // right edge at 50%+296px, so 600 window gives 4px margin.
      const selectorH = Math.min(modes.length * 34 + 40 + 34, 240);
      targetW = 600;
      targetH = MENU_PILL_H + selectorH + 4;
    } else if (pillState !== "idle") {
      targetW = ACTIVE_W;
      targetH = ACTIVE_H;
    } else if (isHovering) {
      // Hover-idle: window widens to expose the scratchpad trigger circle on
      // the right.  Pill stays at window-center via flex layout, so the user
      // perceives the trigger as appearing without the pill moving.
      targetW = HOVER_IDLE_W;
      targetH = HOVER_IDLE_H;
    } else {
      targetW = IDLE_W;
      targetH = IDLE_H;
    }

    const expanded = targetW > IDLE_W || targetH > IDLE_H;
    const wasExpanded = prevExpandedRef.current;
    const prev = prevTargetRef.current;
    const sizeChanged = prev.w !== targetW || prev.h !== targetH;

    if (!sizeChanged) return;

    prevExpandedRef.current = expanded;
    prevTargetRef.current = { w: targetW, h: targetH };

    // Starting a fresh hide→resize→show cycle, so cancel any prior
    // show-timer ourselves.  (See the ref-declaration comment above for
    // why this isn't delegated to the effect cleanup.)
    if (showContentTimerRef.current !== null) {
      window.clearTimeout(showContentTimerRef.current);
      showContentTimerRef.current = null;
    }

    if (!expanded && wasExpanded) {
      // expanded → idle.
      //
      // The old branch delayed the resize 200 ms "to let the content
      // fade out."  That was load-bearing back when the opacity fade
      // ran in both directions (200 ms out / 200 ms in).  Since the
      // hide side is now instant (see the opacity style on the pill's
      // active-content wrapper), the 200 ms wait was pure dead space
      // — it left a tiny idle-sized pill sitting inside a still-
      // expanded transparent window for a fifth of a second after the
      // menu / panel closed.  Resize immediately instead.  Content is
      // either already unmounted (idle path) or gated to opacity 0 on
      // the same tick, so nothing flashes.
      setShowContent(false);
      resizeOverlay(targetW, targetH);
      return;
    }
    // idle → expanded OR expanded → expanded (new dims): hide content,
    // resize, then fade content back in.  Same 80 ms delay in both
    // paths so WebView2 has time to re-layout before content paints.
    setShowContent(false);
    resizeOverlay(targetW, targetH);
    showContentTimerRef.current = window.setTimeout(() => {
      setShowContent(true);
      showContentTimerRef.current = null;
    }, 80);
  }, [pillState, structuredPayload, structuredDegraded, showModeSelector, modes.length, isHovering]);

  // Clean up the pending showContent timer on unmount so it doesn't
  // fire against a torn-down component.
  useEffect(() => {
    return () => {
      if (showContentTimerRef.current !== null) {
        window.clearTimeout(showContentTimerRef.current);
        showContentTimerRef.current = null;
      }
      if (dictatingGraceTimerRef.current !== null) {
        window.clearTimeout(dictatingGraceTimerRef.current);
        dictatingGraceTimerRef.current = null;
      }
      if (degradedTimerRef.current !== null) {
        window.clearTimeout(degradedTimerRef.current);
        degradedTimerRef.current = null;
      }
      if (hoverEnterTimerRef.current !== null) {
        window.clearTimeout(hoverEnterTimerRef.current);
        hoverEnterTimerRef.current = null;
      }
      if (hoverLeaveTimerRef.current !== null) {
        window.clearTimeout(hoverLeaveTimerRef.current);
        hoverLeaveTimerRef.current = null;
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
        setStructuredVoiceCommand(s.structured_voice_command);
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

  // (Resize logic unified above — see the "Consolidated overlay sizing"
  // comment.  This block intentionally left blank after the merge.)

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

  const handleToggleStructuredVoiceCommand = useCallback(async () => {
    const next = !structuredVoiceCommand;
    setStructuredVoiceCommand(next);
    try {
      await applySettingPatch({ structured_voice_command: next });
    } catch {
      setStructuredVoiceCommand(!next);
    }
  }, [structuredVoiceCommand, applySettingPatch]);

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

  // Hover handlers — attached to the outer w-screen div so the cursor can
  // travel between the pill and the trigger circle (with 8 px of transparent
  // gap between them) without breaking the hover state.  The enter timer is
  // a dwell filter: a cursor flying past the pill on its way to the system
  // tray shouldn't pop the trigger open.
  const handleOverlayMouseEnter = useCallback(() => {
    if (hoverLeaveTimerRef.current !== null) {
      window.clearTimeout(hoverLeaveTimerRef.current);
      hoverLeaveTimerRef.current = null;
    }
    if (hoverEnterTimerRef.current !== null) return;
    hoverEnterTimerRef.current = window.setTimeout(() => {
      hoverEnterTimerRef.current = null;
      setIsHovering(true);
    }, HOVER_DWELL_MS);
  }, []);

  const handleOverlayMouseLeave = useCallback(() => {
    if (hoverEnterTimerRef.current !== null) {
      window.clearTimeout(hoverEnterTimerRef.current);
      hoverEnterTimerRef.current = null;
    }
    if (hoverLeaveTimerRef.current !== null) return;
    hoverLeaveTimerRef.current = window.setTimeout(() => {
      hoverLeaveTimerRef.current = null;
      setIsHovering(false);
    }, HOVER_LEAVE_MS);
  }, []);

  // PR 2 will wire this to a toggle_scratchpad Tauri command.  Today it just
  // logs so the click is visible in the WebView2 console during PR 1 review.
  const handleScratchpadClick = useCallback((e: React.MouseEvent) => {
    e.stopPropagation();
    e.preventDefault();
    console.log("[scratchpad] trigger clicked (PR 2 will wire this)");
  }, []);

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

  // Hover-idle is the only state that shows the scratchpad trigger.  Suppress
  // it when any other UI is on screen (mode menu, structured panel, degraded
  // banner) or while the pill itself is invisible (ghost mode).
  const showHoverIdle =
    isHovering &&
    isIdle &&
    !showModeSelector &&
    !structuredPayload &&
    !structuredDegraded &&
    !ghostMode;

  const modeColor = MODE_COLORS[activeColor] ?? MODE_COLORS.amber;

  return (
    <div
      className="w-screen h-screen flex flex-col justify-end items-center relative"
      onMouseEnter={handleOverlayMouseEnter}
      onMouseLeave={handleOverlayMouseLeave}
    >
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
        // The pill — sized to match resizeOverlay dimensions.  Every
        // state carries `border border-transparent` so the 1 px border
        // is always present; only its COLOR changes between states.
        // Without this, idle→active would transition border-width
        // from 0→1 px, which can't interpolate and snaps instead —
        // producing a visible one-frame jolt.  Colour + background
        // transitions below pick up those same class changes and
        // smooth them over 200 ms.
        // Size ladder: slim-idle is a tiny 28×18 slit; hover-idle pops to
        // 56×26 (revealing the waveform and the trigger circle); menu
        // widens to 200×26 (slim like hover-idle, not 34); active pops
        // to 200×34.  Width/height snap instantly to stay locked to the
        // Tauri window resize — transitioning them produces a clipped
        // pill while the window catches up.  Smoothness lives in the
        // child opacity transitions (waveform fade, trigger fade) instead.
        showModeSelector
          ? "w-[200px] h-[26px]"
          : !isIdle
          ? "w-[200px] h-[34px]"
          : isHovering
          ? "w-[64px] h-[26px]"
          : "w-[64px] h-[10px]",
        "relative flex items-center overflow-hidden shrink-0 border border-transparent rounded-full",
        "transition-[border-color,background-color,box-shadow] duration-200 ease-out",
        isProcessing ? "cursor-default" : "cursor-pointer",

        // Idle (just the base background; border inherits transparent).
        isIdle && "bg-[var(--color-pill-bg)]",

        // Recording
        isRecording && "bg-[var(--color-pill-bg)] border-recording-500/30 gap-2.5 px-3.5",

        // Processing
        isProcessing && "bg-[var(--color-pill-bg)] border-amber-500/25 gap-2.5 px-3.5",

        // Structuring (Structured Mode — LLM slot extraction in flight)
        isStructuring && "bg-[var(--color-pill-bg)] border-violet-400/30 gap-2.5 px-3.5",

        // Success
        isSuccess && "bg-[var(--color-pill-bg)] border-success/30 gap-2.5 px-3.5",

        // Error
        isError && "bg-[var(--color-pill-bg)] border-recording-500/35 gap-2.5 px-3.5",
      )}
    >
      {/* ── Idle ──
          Slim-idle (default) renders no content — the pill is just a small
          dark slit, no animation, deliberately calm.  The ambient waveform
          fades in only once the user hovers and the window has finished
          expanding (showContent gate), so the waves appear as the pill
          settles into hover-idle rather than racing the resize. */}
      {isIdle && (
        <div
          aria-hidden="true"
          className="absolute inset-0 flex items-center justify-center pointer-events-none"
          style={{
            opacity: isHovering && showContent ? 1 : 0,
            transition: "opacity 220ms ease",
          }}
        >
          <IdleWaveform color={modeColor} maxBar={10} />
        </div>
      )}

      {/* ── Active states: full pill content with fade ── */}
      {!isIdle && (
        <div
          className="flex items-center w-full h-full gap-2.5"
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

        /* ── Ley Line popup (Structured Mode voice-command gate) ──
           Violet-themed mirror of the ship popup, anchored to the LEFT
           of the Ley Line button since the button is already at the
           window's right edge.  Same surface language (backdrop blur +
           bloom + top rim-light), swapped palette. */
        .ley-line-popup {
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
            0 16px 32px -12px rgba(0,0,0,0.8),
            0 0 22px -10px rgba(188,150,236,0.38);
          backdrop-filter: blur(14px);
          overflow: hidden;
          /* Popup opens to the RIGHT of the Ley Line button AND is
             top-aligned (inline style top:0).  Origin at left-top so the
             scale-in emerges from the corner adjacent to the button and
             flows right + down into place. */
          transform-origin: left top;
          transition:
            opacity 160ms ease-out,
            transform 180ms cubic-bezier(0.16, 1, 0.3, 1);
        }
        .ley-line-popup-bloom {
          position: absolute;
          inset: -60% -20% auto -20%;
          height: 80px;
          background: radial-gradient(ellipse at 50% 0%,
            rgba(186,148,234,0.16) 0%,
            rgba(160,115,220,0.06) 30%,
            transparent 65%);
          pointer-events: none;
          z-index: 0;
        }
        .ley-line-popup-ring {
          position: absolute;
          top: 0; left: 0; right: 0;
          height: 1px;
          background: linear-gradient(90deg,
            transparent 0%,
            rgba(210,178,246,0.35) 50%,
            transparent 100%);
          pointer-events: none;
          z-index: 2;
        }
        .ley-line-popup-content {
          position: relative;
          z-index: 3;
          padding: 9px 11px 10px;
        }
        .ley-line-popup-header {
          display: flex;
          align-items: center;
          gap: 6px;
          margin-bottom: 6px;
        }
        .ley-line-popup-icon {
          color: rgba(210,178,246,0.9);
          filter: drop-shadow(0 0 2px rgba(186,148,234,0.4));
        }
        .ley-line-popup-kicker {
          font-family: var(--font-display);
          font-size: 9px;
          font-weight: 700;
          text-transform: uppercase;
          letter-spacing: 0.2em;
          color: rgba(228,206,248,0.95);
        }
        .ley-line-popup-desc {
          margin: 0 0 9px;
          font-family: var(--font-sans);
          font-size: 9.5px;
          line-height: 1.4;
          color: rgba(255,255,255,0.5);
          letter-spacing: -0.005em;
        }
        .ley-line-popup-row {
          display: flex;
          align-items: center;
          gap: 8px;
        }
        .ley-line-popup-switch {
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
        .ley-line-popup-switch--on {
          background: linear-gradient(180deg,
            rgba(168,124,226,0.95) 0%,
            rgba(138,98,200,0.9) 100%);
          border-color: rgba(210,178,246,0.3);
          box-shadow:
            inset 0 1px 0 rgba(255,245,255,0.22),
            0 0 8px -2px rgba(188,150,236,0.55);
        }
        .ley-line-popup-knob {
          display: inline-block;
          position: absolute;
          top: 50%;
          left: 2px;
          width: 10px;
          height: 10px;
          border-radius: 50%;
          background: rgba(255,255,255,0.92);
          box-shadow: 0 1px 2px rgba(0,0,0,0.3);
          transform: translateY(-50%) translateX(0);
          transition: transform 180ms cubic-bezier(0.4, 0, 0.2, 1);
        }
        .ley-line-popup-switch--on .ley-line-popup-knob {
          transform: translateY(-50%) translateX(12px);
        }
        .ley-line-popup-state {
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

        /* ── Scratchpad trigger ──
           Sits 4 px to the right of the pill (centered at 50%).  Pill is
           64 wide so right edge = 50% + 32, gap = 4, trigger left = 50% + 36.
           Vertically anchored via top calc(50% - 11px) (half the 22 px
           trigger), not bottom, so it stays inside the outer div even when
           the window shrinks mid-fade-out.  Background uses var(--color-pill-bg)
           so the trigger reads as a sibling of the pill rather than an
           amber accent — same shade, just round and small. */
        .scratchpad-trigger {
          position: absolute;
          left: calc(50% + 36px);
          top: calc(50% - 11px);
          width: 22px;
          height: 22px;
          border-radius: 999px;
          display: flex;
          align-items: center;
          justify-content: center;
          background: var(--color-pill-bg);
          border: 1px solid transparent;
          cursor: pointer;
          color: rgba(255,255,255,0.7);
          opacity: 0;
          pointer-events: none;
          transition:
            opacity 220ms ease,
            border-color 180ms ease,
            transform 140ms ease,
            color 160ms ease;
        }
        .scratchpad-trigger--visible {
          opacity: 1;
          pointer-events: auto;
        }
        .scratchpad-trigger--visible:hover {
          border-color: rgba(255,255,255,0.12);
          color: rgba(255,255,255,0.95);
        }
        .scratchpad-trigger--visible:active { transform: scale(0.92); }
        .scratchpad-trigger-icon {
          filter: drop-shadow(0 0 2px rgba(0,0,0,0.45));
        }
      `}</style>
    </button>

    {/* ── Scratchpad trigger circle ──
        Absolute-positioned sibling of the pill so the pill itself stays
        anchored at window-centre via the outer flex layout — only the
        trigger moves as the window expands rightward into hover-idle.
        PR 1 wires it to a console.log placeholder; PR 2 will replace
        handleScratchpadClick with toggle_scratchpad. */}
    <button
      type="button"
      className={cn(
        "scratchpad-trigger",
        showHoverIdle && showContent && "scratchpad-trigger--visible"
      )}
      onClick={handleScratchpadClick}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
      }}
      tabIndex={showHoverIdle && showContent ? 0 : -1}
      aria-hidden={!(showHoverIdle && showContent)}
      title="Scratchpad"
    >
      <StickyNote size={12} strokeWidth={2} className="scratchpad-trigger-icon" />
    </button>
    </div>
  );
}

/* ── Idle waveform: subtle ambient bars ──
   Rendered only inside hover-idle now (the slim-idle slit is intentionally
   empty), so `maxBar` is a single value — 10 px peak inside the 26 px pill.
   Keyframes scale between 0.4× and 1× of that for a 4–10 px breathing range. */
function IdleWaveform({ color, maxBar = 10 }: { color: string; maxBar?: number }) {
  const BAR_COUNT = 5;

  return (
    <div className="flex items-center justify-center gap-[3px]">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          className="rounded-full"
          style={{
            width: 2,
            height: maxBar,
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
