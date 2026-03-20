import { useEffect, useState, useCallback } from "react";
import { Loader2 } from "lucide-react";
import { useRecordingStore, type RecordingStatus } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import { startRecording, stopRecording } from "@/lib/tauri";
import { formatDuration, cn } from "@/lib/utils";
import { PillWaveform } from "./PillWaveform";

/**
 * Floating Pill — always-on-top status capsule that sits above the taskbar.
 *
 * Glass morphism surface, 16-bar waveform, contextual state indicators.
 * This is the primary interface users see while dictating into other apps.
 *
 * States:
 *   idle       → muted, flat waveform, "OV" brand, barely visible
 *   recording  → waveform alive, crimson record dot, duration timer
 *   processing → amber shimmer, "Transcribing..." text, spinner
 *   success    → brief green flash with transcription preview (2s)
 *   error      → brief red flash (2s), then back to idle
 */

type PillState = RecordingStatus | "success";

export function FloatingPill() {
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const duration = useRecordingStore((s) => s.duration);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  // Extended state: adds "success" flash after transcription completes
  const [pillState, setPillState] = useState<PillState>("idle");
  const [flashText, setFlashText] = useState<string | null>(null);

  // Track transcription result for success flash
  useEffect(() => {
    if (status === "idle" && lastTranscription && pillState === "processing") {
      // Transcription just completed — show success flash
      setFlashText(
        lastTranscription.length > 40
          ? lastTranscription.slice(0, 40) + "…"
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

  // Make the overlay window background transparent
  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.body.style.background = "transparent";
  }, []);

  const handleClick = useCallback(async () => {
    try {
      if (status === "idle") {
        await startRecording();
      } else if (status === "recording") {
        await stopRecording();
      }
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
    <div className="h-screen w-screen flex items-center justify-center">
      <button
        onClick={handleClick}
        disabled={isProcessing}
        className={cn(
          // Base pill shape
          "relative flex items-center gap-3 rounded-full",
          "h-[48px] min-w-[340px] px-5",
          // Glass morphism
          "backdrop-blur-2xl backdrop-saturate-150",
          "shadow-[0_4px_30px_rgba(0,0,0,0.35)]",
          // Transitions
          "transition-all duration-300 ease-out",
          // Cursor
          isProcessing ? "cursor-default" : "cursor-pointer",

          // ── State-specific styles ──

          // Idle: muted, barely-there
          isIdle && [
            "bg-[rgba(18,16,14,0.7)]",
            "border border-[rgba(255,255,255,0.05)]",
            "hover:bg-[rgba(22,20,18,0.8)]",
            "hover:border-[rgba(255,255,255,0.08)]",
            "opacity-60 hover:opacity-90",
          ],

          // Recording: alive, crimson accent
          isRecording && [
            "bg-[rgba(18,16,14,0.82)]",
            "border border-recording-500/30",
            "shadow-[0_4px_30px_rgba(0,0,0,0.35),0_0_20px_rgba(180,50,40,0.15)]",
            "opacity-100",
          ],

          // Processing: amber accent, shimmer
          isProcessing && [
            "bg-[rgba(18,16,14,0.82)]",
            "border border-amber-500/25",
            "opacity-100",
          ],

          // Success: brief green flash
          isSuccess && [
            "bg-[rgba(18,16,14,0.82)]",
            "border border-success/30",
            "opacity-100",
          ],

          // Error: brief red flash
          isError && [
            "bg-[rgba(18,16,14,0.82)]",
            "border border-recording-500/40",
            "opacity-100",
          ]
        )}
      >
        {/* ── Processing shimmer overlay ── */}
        {isProcessing && (
          <div
            className="absolute inset-0 rounded-full overflow-hidden pointer-events-none"
            aria-hidden="true"
          >
            <div
              className="absolute inset-0 -translate-x-full"
              style={{
                background:
                  "linear-gradient(90deg, transparent 0%, oklch(0.65 0.16 55 / 0.08) 50%, transparent 100%)",
                animation: "shimmer 2s ease-in-out infinite",
              }}
            />
          </div>
        )}

        {/* ── Left section: Brand / Timer ── */}
        <div className="shrink-0 w-[52px] flex items-center justify-start">
          {isIdle && (
            <span className="font-display text-base text-text-muted/60 select-none">
              OV
            </span>
          )}

          {isRecording && (
            <span className="font-mono text-sm tabular-nums text-recording-300 tracking-wide">
              {formatDuration(duration)}
            </span>
          )}

          {isProcessing && (
            <Loader2
              size={16}
              className="text-amber-400 animate-spin"
              strokeWidth={2}
            />
          )}

          {isSuccess && (
            <svg
              width="16"
              height="16"
              viewBox="0 0 16 16"
              className="text-success"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polyline points="3 8.5 6.5 12 13 4" />
            </svg>
          )}

          {isError && (
            <span className="text-recording-400 text-sm font-medium">!</span>
          )}
        </div>

        {/* ── Center section: Waveform / Status text ── */}
        <div className="flex-1 flex items-center justify-center overflow-hidden">
          {(isIdle || isRecording) && (
            <PillWaveform active={isRecording} />
          )}

          {isProcessing && (
            <span className="text-xs font-medium text-amber-400/80 tracking-wide truncate">
              Transcribing…
            </span>
          )}

          {isSuccess && flashText && (
            <span className="text-xs text-text-secondary truncate select-text">
              {flashText}
            </span>
          )}

          {isError && (
            <span className="text-xs text-recording-300 truncate">
              Something went wrong
            </span>
          )}
        </div>

        {/* ── Right section: Record indicator ── */}
        <div className="shrink-0 w-[36px] flex items-center justify-end">
          {isIdle && (
            <div className="flex items-center gap-1.5 opacity-50">
              <div className="h-2 w-2 rounded-full border border-text-muted/40" />
            </div>
          )}

          {isRecording && (
            <div className="relative flex items-center justify-center">
              {/* Pulsing glow ring */}
              <span
                className="absolute h-5 w-5 rounded-full bg-recording-500/20"
                style={{ animation: "recording-pulse 2s ease-in-out infinite" }}
              />
              {/* Solid dot */}
              <span className="relative h-2.5 w-2.5 rounded-full bg-recording-500 shadow-[0_0_8px_rgba(180,50,40,0.5)]" />
            </div>
          )}

          {isProcessing && (
            <div className="h-2 w-2 rounded-full bg-amber-500/60" />
          )}

          {isSuccess && (
            <div className="h-2 w-2 rounded-full bg-success/60" />
          )}
        </div>
      </button>

      {/* Shimmer keyframe — injected once */}
      <style>{`
        @keyframes shimmer {
          0% { transform: translateX(-100%); }
          100% { transform: translateX(200%); }
        }
      `}</style>
    </div>
  );
}
