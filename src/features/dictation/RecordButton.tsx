import { Mic, Square, Loader2 } from "lucide-react";
import { useRecordingStore } from "@/stores/recordingStore";
import { startRecording, stopRecording, cancelRecording } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { formatDuration } from "@/lib/utils";

export function RecordButton() {
  const status = useRecordingStore((s) => s.status);
  const duration = useRecordingStore((s) => s.duration);

  const handleClick = async () => {
    try {
      if (status === "idle") {
        await startRecording();
      } else if (status === "recording") {
        await stopRecording();
      }
    } catch (err) {
      console.error("Recording action failed:", err);
    }
  };

  const handleCancel = async () => {
    try {
      await cancelRecording();
    } catch (err) {
      console.error("Cancel failed:", err);
    }
  };

  const isIdle = status === "idle";
  const isRecording = status === "recording";
  const isProcessing = status === "processing";

  return (
    <div className="flex flex-col items-center">
      {/* Reserved top spacer — mirrors the metadata slot below so the button
          stays vertically centered and never moves between idle / recording /
          processing states. */}
      <div className="h-[64px]" aria-hidden="true" />

      {/* Button container — holds the button and the animated ring layers */}
      <div className="relative flex items-center justify-center">
        {/* Recording: expanding ring animation */}
        {isRecording && (
          <span
            className="absolute inset-0 rounded-full animate-recording-ring"
            aria-hidden="true"
          />
        )}

        {/* Recording: warm crimson glow */}
        {isRecording && (
          <span
            className="absolute inset-[-10px] rounded-full bg-recording-500/22 blur-2xl animate-recording-pulse"
            aria-hidden="true"
          />
        )}

        {/* Processing: spinning amber ring */}
        {isProcessing && (
          <svg
            className="absolute left-1/2 top-1/2 h-[112px] w-[112px] -translate-x-1/2 -translate-y-1/2"
            viewBox="0 0 112 112"
            aria-hidden="true"
            style={{ animation: "spin-slow 2s linear infinite" }}
          >
            <circle
              cx="56"
              cy="56"
              r="53"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeDasharray="80 260"
              strokeLinecap="round"
              className="text-amber-400"
            />
          </svg>
        )}

        {/* The button itself */}
        <button
          onClick={handleClick}
          disabled={isProcessing}
          aria-label={
            isIdle
              ? "Start recording"
              : isRecording
                ? "Stop recording"
                : "Processing transcription"
          }
          className={cn(
            // Base: 96px circle with smooth transitions and press feel
            "relative flex h-[96px] w-[96px] items-center justify-center rounded-full",
            "transition-all duration-300 ease-out",
            "active:scale-[0.97]",
            "focus-visible:outline-2 focus-visible:outline-offset-[5px] focus-visible:outline-amber-400",

            // Idle state — clean, soft amber halo
            isIdle && [
              "bg-gradient-to-b from-surface-2 to-surface-1",
              "border border-amber-400/35",
              "shadow-[0_1px_0_0_rgb(255_255_255_/_0.04)_inset,0_2px_4px_-2px_rgb(0_0_0_/_0.30)]",
              "hover:border-amber-400/65 hover:scale-[1.025]",
              "hover:shadow-[0_1px_0_0_rgb(255_255_255_/_0.06)_inset,0_2px_4px_-2px_rgb(0_0_0_/_0.30)]",
            ],

            // Recording state
            isRecording && [
              "bg-gradient-to-b from-recording-400 to-recording-500",
              "border border-recording-300/55",
              "animate-recording-pulse",
              "shadow-[0_0_0_1px_rgb(255_255_255_/_0.04)_inset,0_4px_12px_-4px_rgb(0_0_0_/_0.40)]",
            ],

            // Processing state
            isProcessing && [
              "bg-surface-2 border border-surface-4",
              "cursor-not-allowed opacity-80",
            ]
          )}
        >
          {isIdle && <Mic size={34} className="text-amber-300" strokeWidth={1.5} />}
          {isRecording && (
            <Square size={26} className="text-white drop-shadow-[0_1px_2px_rgb(0_0_0_/_0.25)]" fill="currentColor" strokeWidth={0} />
          )}
          {isProcessing && (
            <Loader2
              size={30}
              className="text-amber-400 animate-spin"
              strokeWidth={1.5}
            />
          )}
        </button>
      </div>

      {/* Recording metadata: timer + cancel — lives in a fixed-height slot
          that is always reserved, so showing/hiding it never shifts the
          button above it. */}
      <div className="flex h-[64px] flex-col items-center gap-1.5 pt-4">
        {isRecording && (
          <div className="flex flex-col items-center gap-1.5 animate-fade-in">
            <span className="font-mono text-lg tracking-wider text-recording-300 tabular-nums">
              {formatDuration(duration)}
            </span>
            <button
              onClick={handleCancel}
              className={cn(
                "text-[11px] font-medium tracking-[0.14em] uppercase text-text-muted",
                "transition-colors duration-150",
                "hover:text-text-secondary"
              )}
            >
              Cancel
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
