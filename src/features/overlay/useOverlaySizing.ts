import { useEffect, useRef, useState } from "react";
import { resizeOverlay } from "@/lib/tauri";
import type { RecordingStatus } from "@/stores/recordingStore";
import type { CommandUiState } from "@/stores/commandStore";

type PillState = RecordingStatus | "success";

// ─── Pill geometry: single source of truth (TS side) ───
// The Rust side mirrors the idle *window* size for window creation and
// recovery — keep OVERLAY_IDLE_WIN_W/H in src-tauri/src/lib.rs in sync
// with IDLE_WIN_W/H below.
export const ACTIVE_W = 156; // active window width; pill is ACTIVE_PILL_W inside
export const ACTIVE_H = 34;
export const ACTIVE_PILL_W = 148; // pill CSS width — 8px slack inside ACTIVE_W
export const MENU_PILL_W = 196; // thin base slit under the mode-selector menu
export const IDLE_W = 42;
export const IDLE_H = 14;
// The idle pill is a tiny dark slit; on a black UI behind it you can't tell
// where it is. We reserve a small transparent margin around it (kept small so
// the window doesn't eat clicks meant for apps behind it) for a faint amber
// locator glow to bleed into. Asymmetric on purpose: full pad on the sides +
// top, but the pill sits nearly flush at the bottom (just a 2px sliver) so it
// stays low above the taskbar — centering it floated the slit visibly too high.
export const IDLE_GLOW_PAD = 10;
export const IDLE_WIN_W = IDLE_W + IDLE_GLOW_PAD * 2; // 62 (10px each side)
export const IDLE_WIN_H = IDLE_H + IDLE_GLOW_PAD + 2; // 26 (10px top, 2px bottom)

interface OverlaySizingOptions {
  pillState: PillState;
  hasStructuredPayload: boolean;
  structuredDegraded: string | null;
  showModeSelector: boolean;
  modeCount: number;
  commandState: CommandUiState;
  /** Confirm pill carries an editable message textarea — needs a taller window. */
  commandEditableConfirm: boolean;
}

export function useOverlaySizing({
  pillState,
  hasStructuredPayload,
  structuredDegraded,
  showModeSelector,
  modeCount,
  commandState,
  commandEditableConfirm,
}: OverlaySizingOptions) {
  const prevTargetRef = useRef<{ w: number; h: number }>({ w: IDLE_WIN_W, h: IDLE_WIN_H });
  const showContentTimerRef = useRef<number | null>(null);
  const settleTimerRef = useRef<number | null>(null);
  const [showContent, setShowContent] = useState(false);

  // Consolidated overlay sizing.
  //
  // The window is resized to contain BOTH the old and new pill size (the union)
  // for the duration of the pill's CSS width/height transition, then settled to
  // the exact target. This lets the pill animate its size smoothly in either
  // direction without the window bounds clipping it mid-grow / mid-shrink.
  //
  // `showContent` is reset to false on every size change, then flipped back
  // after a short delay so WebView2 has time to re-layout before painting.
  useEffect(() => {
    let targetW: number;
    let targetH: number;
    if (commandState === "confirm" && commandEditableConfirm) {
      // Review-message confirm: header row + a 2-row textarea.
      targetW = 340;
      targetH = 96;
    } else if (commandState === "confirm") {
      // Room for "Open X?" plus the Yes/No buttons.
      targetW = 300;
      targetH = 46;
    } else if (commandState !== "idle") {
      // Listening / recognizing / done / error — a slightly wider pill.
      targetW = 260;
      targetH = ACTIVE_H;
    } else if (hasStructuredPayload) {
      targetW = 440;
      targetH = 480;
    } else if (structuredDegraded) {
      targetW = 420;
      targetH = ACTIVE_H + 80;
    } else if (showModeSelector) {
      const selectorH = Math.min(modeCount * 34 + 40 + 34, 240);
      // 640 (was 600) so the right-click Ship / Voice-Command popups, which open
      // to the right of the toggle column, aren't clipped by the window edge.
      targetW = 640;
      // The pill collapses to a thin base slit under the menu, so reserve only
      // the idle height below the menu (not the full active height).
      targetH = IDLE_H + selectorH + 4;
    } else if (pillState !== "idle") {
      targetW = ACTIVE_W;
      targetH = ACTIVE_H;
    } else {
      // Idle window includes the glow margin around the pill.
      targetW = IDLE_WIN_W;
      targetH = IDLE_WIN_H;
    }

    const prev = prevTargetRef.current;
    const sizeChanged = prev.w !== targetW || prev.h !== targetH;
    if (!sizeChanged) return;
    prevTargetRef.current = { w: targetW, h: targetH };

    if (showContentTimerRef.current !== null) {
      window.clearTimeout(showContentTimerRef.current);
      showContentTimerRef.current = null;
    }
    if (settleTimerRef.current !== null) {
      window.clearTimeout(settleTimerRef.current);
      settleTimerRef.current = null;
    }

    // Union of old + new, so the animating pill is never clipped.
    const interW = Math.max(prev.w, targetW);
    const interH = Math.max(prev.h, targetH);
    const needsSettle = interW !== targetW || interH !== targetH;

    setShowContent(false);
    resizeOverlay(interW, interH);

    showContentTimerRef.current = window.setTimeout(() => {
      setShowContent(true);
      showContentTimerRef.current = null;
    }, 80);

    // After the pill's size transition completes, shrink the (transparent)
    // window back to the exact target. Only needed when collapsing.
    if (needsSettle) {
      settleTimerRef.current = window.setTimeout(() => {
        resizeOverlay(targetW, targetH);
        settleTimerRef.current = null;
      }, 320);
    }
  }, [pillState, hasStructuredPayload, structuredDegraded, showModeSelector, modeCount, commandState, commandEditableConfirm]);

  useEffect(() => {
    return () => {
      if (showContentTimerRef.current !== null) {
        window.clearTimeout(showContentTimerRef.current);
        showContentTimerRef.current = null;
      }
      if (settleTimerRef.current !== null) {
        window.clearTimeout(settleTimerRef.current);
        settleTimerRef.current = null;
      }
    };
  }, []);

  return showContent;
}
