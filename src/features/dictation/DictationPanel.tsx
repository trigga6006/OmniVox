import { RecordButton } from "./RecordButton";
import { AudioVisualizer } from "./AudioVisualizer";
import { useRecordingStore } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import { cn } from "@/lib/utils";

export function DictationPanel() {
  // Wire up Tauri event listeners for recording state, audio level, transcription
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  const isIdle = status === "idle";
  const isRecording = status === "recording";
  const isProcessing = status === "processing";

  return (
    <div className="flex h-full flex-col items-center justify-center px-8 py-12">
      {/* ── Status headline ──────────────────────────────────── */}
      <h1
        className={cn(
          "font-display text-3xl tracking-tight opacity-0 animate-fade-in",
          isIdle && "text-text-primary",
          isRecording && "text-amber-400",
          isProcessing && "text-text-secondary"
        )}
      >
        {isIdle && "Ready to listen"}
        {isRecording && "Listening\u2026"}
        {isProcessing && "Transcribing\u2026"}
        {status === "error" && "Something went wrong"}
      </h1>

      {/* ── Instruction line ─────────────────────────────────── */}
      <p
        className="mt-3 text-sm text-text-muted opacity-0 animate-fade-in"
        style={{ animationDelay: "80ms" }}
      >
        {isIdle && "Ctrl + Win to begin"}
        {isRecording && "Speak now \u2014 press again to stop"}
        {isProcessing && "Hang tight, processing your audio\u2026"}
        {status === "error" && "Try recording again"}
      </p>

      {/* ── Spacer ───────────────────────────────────────────── */}
      <div className="flex-1 max-h-20 min-h-10" />

      {/* ── Hero: Record Button ──────────────────────────────── */}
      <div className="opacity-0 animate-scale-in" style={{ animationDelay: "150ms" }}>
        <RecordButton />
      </div>

      {/* ── Audio Visualizer (recording only) ────────────────── */}
      <div
        className={cn(
          "mt-6 transition-all duration-300",
          isRecording ? "opacity-100 translate-y-0" : "opacity-0 translate-y-2 pointer-events-none"
        )}
        style={{ minHeight: 44 }}
      >
        {isRecording && <AudioVisualizer />}
      </div>

      {/* ── Spacer ───────────────────────────────────────────── */}
      <div className="flex-1 max-h-24 min-h-10" />

      {/* ── Last transcription card ──────────────────────────── */}
      {lastTranscription && (
        <div
          className={cn(
            "w-full max-w-lg rounded-lg bg-surface-1 p-5",
            "border-l-2 border-amber-700",
            "opacity-0 animate-slide-up"
          )}
        >
          <p className="font-display italic text-sm text-text-muted mb-2">
            Last transcription
          </p>
          <p className="font-sans text-base leading-relaxed text-text-primary select-text">
            {lastTranscription}
          </p>
        </div>
      )}
    </div>
  );
}
