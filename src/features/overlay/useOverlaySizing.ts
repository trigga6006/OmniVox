import { useEffect, useRef, useState } from "react";
import { resizeOverlay } from "@/lib/tauri";
import type { RecordingStatus } from "@/stores/recordingStore";

type PillState = RecordingStatus | "success";

export const ACTIVE_W = 210;
export const ACTIVE_H = 34;
export const IDLE_W = 56;
export const IDLE_H = 26;

interface OverlaySizingOptions {
  pillState: PillState;
  hasStructuredPayload: boolean;
  structuredDegraded: string | null;
  showModeSelector: boolean;
  modeCount: number;
}

export function useOverlaySizing({
  pillState,
  hasStructuredPayload,
  structuredDegraded,
  showModeSelector,
  modeCount,
}: OverlaySizingOptions) {
  const prevExpandedRef = useRef(false);
  const prevTargetRef = useRef<{ w: number; h: number }>({ w: IDLE_W, h: IDLE_H });
  const showContentTimerRef = useRef<number | null>(null);
  const [showContent, setShowContent] = useState(false);

  // Consolidated overlay sizing.
  //
  // Prior version had TWO effects that both reacted to the same state
  // transitions and called resizeOverlay independently. When the user
  // right-clicked to open the mode selector, effect #1 issued
  // resizeOverlay(ACTIVE_W, ACTIVE_H) and effect #2 issued
  // resizeOverlay(600, ACTIVE_H+selectorH+4) in the same tick. Both
  // go through Tauri IPC; under load the first one could finish AFTER
  // the second, leaving the overlay clipped to a one-line-tall window.
  //
  // `showContent` is reset to false on every size change, then flipped
  // back after resize so WebView2 has time to re-layout before painting.
  useEffect(() => {
    let targetW: number;
    let targetH: number;
    if (hasStructuredPayload) {
      targetW = 440;
      targetH = 480;
    } else if (structuredDegraded) {
      targetW = 420;
      targetH = ACTIVE_H + 80;
    } else if (showModeSelector) {
      const selectorH = Math.min(modeCount * 34 + 40 + 34, 240);
      targetW = 600;
      targetH = ACTIVE_H + selectorH + 4;
    } else if (pillState !== "idle") {
      targetW = ACTIVE_W;
      targetH = ACTIVE_H;
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

    if (showContentTimerRef.current !== null) {
      window.clearTimeout(showContentTimerRef.current);
      showContentTimerRef.current = null;
    }

    if (!expanded && wasExpanded) {
      setShowContent(false);
      resizeOverlay(targetW, targetH);
      return;
    }

    setShowContent(false);
    resizeOverlay(targetW, targetH);
    showContentTimerRef.current = window.setTimeout(() => {
      setShowContent(true);
      showContentTimerRef.current = null;
    }, 80);
  }, [pillState, hasStructuredPayload, structuredDegraded, showModeSelector, modeCount]);

  useEffect(() => {
    return () => {
      if (showContentTimerRef.current !== null) {
        window.clearTimeout(showContentTimerRef.current);
        showContentTimerRef.current = null;
      }
    };
  }, []);

  return showContent;
}
