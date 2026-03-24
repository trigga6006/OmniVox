import { useEffect, useState, useCallback } from "react";
import { Copy, Check } from "lucide-react";
import { RecordButton } from "./RecordButton";
import { AudioVisualizer } from "./AudioVisualizer";
import { useRecordingStore } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import { getSettings, getDictationStats, type DictationStats } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export function DictationPanel() {
  // Wire up Tauri event listeners for recording state, audio level, transcription
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  const [hotkeyLabel, setHotkeyLabel] = useState("Ctrl + Alt");

  useEffect(() => {
    getSettings()
      .then((s) => {
        if (s.hotkey?.labels?.length) {
          setHotkeyLabel(s.hotkey.labels.join(" + "));
        }
      })
      .catch(() => {});
  }, []);

  const isIdle = status === "idle";
  const isRecording = status === "recording";
  const isProcessing = status === "processing";

  return (
    <div className="relative flex h-full flex-col items-center px-8 py-12">
      {/* ── Top section: headline + instruction ─────────────── */}
      <div className="flex flex-1 flex-col items-center justify-end">
        <h1
          className={cn(
            "font-display font-semibold text-3xl tracking-tight opacity-0 animate-fade-in",
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

        <p
          className="mt-3 text-sm text-text-muted opacity-0 animate-fade-in"
          style={{ animationDelay: "80ms" }}
        >
          {isIdle && `${hotkeyLabel} to begin`}
          {isRecording && "Speak now \u2014 press again to stop"}
          {isProcessing && "Hang tight, processing your audio\u2026"}
          {status === "error" && "Try recording again"}
        </p>
      </div>

      {/* ── Center: Record Button (fixed position) ─────────── */}
      <div className="my-8 shrink-0 opacity-0 animate-scale-in" style={{ animationDelay: "150ms" }}>
        <RecordButton />
      </div>

      {/* ── Bottom section: visualizer + transcription ─────── */}
      <div className="flex flex-1 flex-col items-center justify-start w-full">
        {/* Audio Visualizer — occupies space but invisible when not recording */}
        <div
          className={cn(
            "transition-opacity duration-300",
            isRecording ? "opacity-100" : "opacity-0 pointer-events-none"
          )}
          style={{ height: 44 }}
        >
          {isRecording && <AudioVisualizer />}
        </div>

        <div className="h-6" />

        {/* ── Word count & milestone ─────────────────────────── */}
        <StatsCard />

        {/* ── Last transcription card ──────────────────────────── */}
        {lastTranscription && (
          <TranscriptionCard text={lastTranscription} />
        )}
      </div>
    </div>
  );
}

/* ── Last transcription card with copy button ── */

function TranscriptionCard({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  }, [text]);

  return (
    <div
      className={cn(
        "w-full max-w-lg rounded-lg bg-surface-1 p-5 mt-4",
        "opacity-0 animate-slide-up"
      )}
    >
      <div className="flex items-center justify-between mb-2">
        <p className="text-xs font-medium uppercase tracking-wider text-text-muted">
          Last transcription
        </p>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1 text-xs text-text-muted hover:text-text-secondary transition-colors"
        >
          {copied ? <Check size={12} className="text-emerald-400" /> : <Copy size={12} />}
          <span>{copied ? "Copied" : "Copy"}</span>
        </button>
      </div>
      <div className="max-h-32 overflow-y-auto">
        <p className="font-sans text-base leading-relaxed text-text-primary select-text">
          {text}
        </p>
      </div>
    </div>
  );
}

/* ── Milestones ── */

const MILESTONES = [
  { words: 0, label: "Just Getting Started" },
  { words: 100, label: "First Steps" },
  { words: 500, label: "Finding Your Voice" },
  { words: 1000, label: "Chatterbox" },
  { words: 5000, label: "Storyteller" },
  { words: 10000, label: "Bookworm" },
  { words: 25000, label: "Novelist in Training" },
  { words: 50000, label: "Novel Complete" },
  { words: 100000, label: "Prolific Author" },
];

function getCurrentMilestone(words: number) {
  let current = MILESTONES[0];
  for (const m of MILESTONES) {
    if (words >= m.words) current = m;
    else break;
  }
  return current;
}

function getNextMilestone(words: number) {
  for (const m of MILESTONES) {
    if (words > 0 && words < m.words) return m;
  }
  return null;
}

function StatsCard() {
  const [stats, setStats] = useState<DictationStats | null>(null);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  useEffect(() => {
    getDictationStats().then(setStats).catch(() => {});
  }, []);

  // Refresh after each new transcription
  useEffect(() => {
    if (lastTranscription) {
      getDictationStats().then(setStats).catch(() => {});
    }
  }, [lastTranscription]);

  if (!stats || stats.total_words === 0) return null;

  const milestone = getCurrentMilestone(stats.total_words);
  const next = getNextMilestone(stats.total_words);
  const progress = next
    ? ((stats.total_words - milestone.words) / (next.words - milestone.words)) * 100
    : 100;

  return (
    <div className="w-full max-w-lg opacity-0 animate-fade-in" style={{ animationDelay: "200ms", animationFillMode: "forwards" }}>
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium text-amber-400">
            {stats.total_words.toLocaleString()} words
          </span>
          <span className="text-xs text-text-muted">&middot;</span>
          <span className="text-xs text-text-muted">{milestone.label}</span>
        </div>
        {next && (
          <span className="text-[11px] text-text-muted">
            {next.words.toLocaleString()} next
          </span>
        )}
      </div>
      {next && (
        <div className="mt-1.5 h-1 w-full rounded-full bg-surface-2">
          <div
            className="h-1 rounded-full bg-amber-500/40 transition-all duration-500"
            style={{ width: `${Math.min(progress, 100)}%` }}
          />
        </div>
      )}
    </div>
  );
}
