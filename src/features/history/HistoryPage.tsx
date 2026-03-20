import { Clock } from "lucide-react";

export function HistoryPage() {
  return (
    <div className="flex h-full flex-col p-6">
      {/* Header */}
      <div
        className="animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl text-text-primary">History</h1>
        <p className="text-sm text-text-muted mt-1">Transcription archive</p>
      </div>

      {/* Empty state */}
      <div
        className="flex flex-1 flex-col items-center justify-center gap-4 animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.15s", animationFillMode: "forwards" }}
      >
        {/* Icon with amber ring */}
        <div className="relative flex items-center justify-center">
          <div className="absolute h-20 w-20 rounded-full bg-amber-500/5 border border-amber-700/20" />
          <Clock size={40} strokeWidth={1.5} className="relative text-text-muted" />
        </div>

        <div className="text-center mt-2">
          <p className="text-sm font-medium text-text-secondary">
            No transcriptions yet
          </p>
          <p className="text-xs text-text-muted mt-1">
            Your dictation history will appear here
          </p>
        </div>
      </div>
    </div>
  );
}
