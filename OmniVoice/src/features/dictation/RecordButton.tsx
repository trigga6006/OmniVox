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
    <div className="flex flex-col items-center gap-5">
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
            className="absolute inset-[-8px] rounded-full bg-recording-500/20 blur-xl animate-recording-pulse"
            aria-hidden="true"
          />
        )}

        {/* Processing: spinning amber ring */}
        {isProcessing && (
          <svg
            className="absolute h-[108px] w-[108px]"
            viewBox="0 0 108 108"
            aria-hidden="true"
            style={{ animation: "spin-slow 2s linear infinite" }}
          >
            <circle
              cx="54"
              cy="54"
              r="51"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeDasharray="80 240"
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
            "relative flex h-24 w-24 items-center justify-center rounded-full",
            "transition-all duration-200 ease-out",
            "active:scale-95",
            "focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-amber-500",

            // Idle state
            isIdle && [
              "bg-surface-2 border border-amber-400/40",
              "hover:border-amber-500 hover:scale-105",
              "hover:shadow-[0_0_24px_-4px] hover:shadow-amber-500/20",
            ],

            // Recording state
            isRecording && [
              "bg-recording-500 border border-recording-400/50",
              "animate-recording-pulse",
              "shadow-[0_0_40px_-4px] shadow-recording-500/40",
            ],

            // Processing state
            isProcessing && [
              "bg-surface-2 border border-surface-4",
              "cursor-not-allowed opacity-80",
            ]
          )}
        >
          {isIdle && <Mic size={36} className="text-amber-400" strokeWidth={1.5} />}
          {isRecording && (
            <Square size={28} className="text-white" fill="currentColor" strokeWidth={0} />
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

      {/* Recording metadata: timer + cancel */}
      {isRecording && (
        <div className="flex flex-col items-center gap-2 animate-fade-in">
          <span className="font-mono text-lg tracking-wider text-recording-400 tabular-nums">
            {formatDuration(duration)}
          </span>
          <button
            onClick={handleCancel}
            className={cn(
              "text-xs font-sans tracking-wide uppercase text-text-muted",
              "transition-colors duration-150",
              "hover:text-text-secondary"
            )}
          >
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}
