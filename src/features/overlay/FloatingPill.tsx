import { useEffect, useState, useCallback, useRef } from "react";
import { Loader2 } from "lucide-react";
import { useRecordingStore, type RecordingStatus } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import { startRecording, stopRecording, resizeOverlay } from "@/lib/tauri";
import { formatDuration, cn } from "@/lib/utils";
import { PillWaveform } from "./PillWaveform";

type PillState = RecordingStatus | "success";

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

  // Mount: transparent bg, shrink to idle
  useEffect(() => {
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

  const handleClick = useCallback(async () => {
    try {
      if (status === "idle") await startRecording();
      else if (status === "recording") await stopRecording();
    } catch (err) {
      console.error("Pill recording toggle failed:", err);
    }
  }, [status]);

  const isIdle = pillState === "idle";
  const isRecording = pillState === "recording";
  const isProcessing = pillState === "processing";
  const isSuccess = pillState === "success";
  const isError = pillState === "error";

  return (
    <button
      onClick={handleClick}
      disabled={isProcessing}
      className={cn(
        // Fill the entire window — the window IS the pill
        "w-screen h-screen",
        "relative flex items-center overflow-hidden",
        isProcessing ? "cursor-default" : "cursor-pointer",

        // Idle
        isIdle && "bg-[#1a1a1a] rounded-full",

        // Recording
        isRecording && "bg-[#1a1a1a] border border-recording-500/30 rounded-full gap-2.5 px-3.5",

        // Processing
        isProcessing && "bg-[#1a1a1a] border border-amber-500/25 rounded-full gap-2.5 px-3.5",

        // Success
        isSuccess && "bg-[#1a1a1a] border border-success/30 rounded-full gap-2.5 px-3.5",

        // Error
        isError && "bg-[#1a1a1a] border border-recording-500/35 rounded-full gap-2.5 px-3.5",
      )}
    >
      {/* ── Idle: sleek ambient waveform ── */}
      {isIdle && <IdleWaveform />}

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

          {/* Center: waveform / status text */}
          <div className="flex-1 flex items-center justify-center overflow-hidden">
            {isRecording && <PillWaveform active />}
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
  );
}

/* ── Idle waveform: subtle ambient bars ── */
function IdleWaveform() {
  const BAR_COUNT = 5;

  return (
    <div className="flex items-center justify-center gap-[3px] w-full h-full">
      {Array.from({ length: BAR_COUNT }).map((_, i) => (
        <div
          key={i}
          className="bg-amber-400/25 rounded-full"
          style={{
            width: 2,
            height: 6,
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
