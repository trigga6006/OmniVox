import { useEffect, useState, useCallback } from "react";
import { Copy, Check, X, ArrowRight, Sparkles, Loader2 } from "lucide-react";
import { RecordButton } from "./RecordButton";
import { AudioVisualizer } from "./AudioVisualizer";
import { useRecordingStore } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import {
  getSettings,
  getDictationStats,
  onCleanupStatus,
  onCleanupResult,
  onCleanupError,
  type DictationStats,
  type AppSettings,
  type CleanupResult,
} from "@/lib/tauri";
import { useAppStore } from "@/stores/appStore";
import { cn } from "@/lib/utils";

export function DictationPanel() {
  // Wire up Tauri event listeners for recording state, audio level, transcription
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  const [hotkeyLabel, setHotkeyLabel] = useState("Ctrl + Alt");
  const [cleanupResult, setCleanupResult] = useState<CleanupResult | null>(null);
  const [cleanupStatus, setCleanupStatus] = useState<string>("idle");
  const [cleanupError, setCleanupError] = useState<string | null>(null);
  const [cleanupEnabled, setCleanupEnabled] = useState(false);

  useEffect(() => {
    getSettings()
      .then((s) => {
        if (s.hotkey?.labels?.length) {
          setHotkeyLabel(s.hotkey.labels.join(" + "));
        }
        setCleanupEnabled(s.cleanup_enabled);
      })
      .catch(() => {});
  }, []);

  // Listen for cleanup events
  useEffect(() => {
    const unlistenStatus = onCleanupStatus((s) => {
      setCleanupStatus(s);
      if (s === "running") {
        setCleanupError(null);
      }
    });
    const unlistenResult = onCleanupResult((r) => {
      setCleanupResult(r);
    });
    const unlistenError = onCleanupError((e) => {
      setCleanupError(e);
    });
    return () => {
      unlistenStatus.then((fn) => fn());
      unlistenResult.then((fn) => fn());
      unlistenError.then((fn) => fn());
    };
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

        {/* ── Feature discovery tip ────────────────────────────── */}
        <FeatureTip />

        {/* ── Last transcription card ──────────────────────────── */}
        {lastTranscription && (
          cleanupEnabled ? (
            <CleanupTranscriptionCard
              rawText={lastTranscription}
              cleanupResult={cleanupResult}
              cleanupStatus={cleanupStatus}
              cleanupError={cleanupError}
            />
          ) : (
            <TranscriptionCard text={lastTranscription} />
          )
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

/* ── Cleanup transcription card with raw/cleaned tabs ── */

function CleanupTranscriptionCard({
  rawText,
  cleanupResult,
  cleanupStatus,
  cleanupError,
}: {
  rawText: string;
  cleanupResult: CleanupResult | null;
  cleanupStatus: string;
  cleanupError: string | null;
}) {
  const [activeTab, setActiveTab] = useState<"raw" | "cleaned">("cleaned");
  const [copied, setCopied] = useState(false);

  const displayText =
    activeTab === "cleaned" && cleanupResult?.cleaned_text
      ? cleanupResult.cleaned_text
      : rawText;

  const handleCopy = useCallback(() => {
    navigator.clipboard
      .writeText(displayText)
      .then(() => {
        setCopied(true);
        setTimeout(() => setCopied(false), 1500);
      })
      .catch(() => {});
  }, [displayText]);

  return (
    <div
      className={cn(
        "w-full max-w-lg rounded-lg bg-surface-1 p-5 mt-4",
        "opacity-0 animate-slide-up"
      )}
    >
      {/* Tab header */}
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-center gap-1 bg-surface-2 rounded-lg p-0.5">
          <button
            onClick={() => setActiveTab("raw")}
            className={cn(
              "px-2.5 py-1 text-xs font-medium rounded-md transition-colors",
              activeTab === "raw"
                ? "bg-surface-1 text-text-primary shadow-sm"
                : "text-text-muted hover:text-text-secondary"
            )}
          >
            Raw
          </button>
          <button
            onClick={() => setActiveTab("cleaned")}
            className={cn(
              "px-2.5 py-1 text-xs font-medium rounded-md transition-colors inline-flex items-center gap-1",
              activeTab === "cleaned"
                ? "bg-surface-1 text-text-primary shadow-sm"
                : "text-text-muted hover:text-text-secondary"
            )}
          >
            <Sparkles size={10} />
            Cleaned
          </button>
        </div>

        <div className="flex items-center gap-2">
          {/* Cleanup status indicator */}
          {cleanupStatus === "running" && (
            <Loader2 size={12} className="animate-spin text-amber-400" />
          )}
          {cleanupResult?.duration_ms != null && cleanupStatus === "success" && (
            <span className="text-[10px] text-text-muted">
              {(cleanupResult.duration_ms / 1000).toFixed(1)}s
            </span>
          )}

          <button
            onClick={handleCopy}
            className="flex items-center gap-1 text-xs text-text-muted hover:text-text-secondary transition-colors"
          >
            {copied ? (
              <Check size={12} className="text-emerald-400" />
            ) : (
              <Copy size={12} />
            )}
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
        </div>
      </div>

      {/* Error state */}
      {activeTab === "cleaned" && cleanupError && (
        <p className="text-xs text-red-400 mb-2">{cleanupError}</p>
      )}

      {/* Text display */}
      <div className="max-h-40 overflow-y-auto">
        {activeTab === "cleaned" && cleanupStatus === "running" && !cleanupResult ? (
          <div className="flex items-center gap-2 py-4 justify-center">
            <Loader2 size={14} className="animate-spin text-amber-400" />
            <span className="text-sm text-text-muted">Cleaning up transcript...</span>
          </div>
        ) : (
          <p className="font-sans text-base leading-relaxed text-text-primary select-text">
            {displayText}
          </p>
        )}
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

/* ── Feature discovery tips ── */

interface Tip {
  id: string;
  text: string;
  /** Return true to show this tip (feature not yet explored). */
  shouldShow: (s: AppSettings) => boolean;
  page: "settings" | "modes" | "models";
}

const TIPS: Tip[] = [
  {
    id: "ship_mode",
    text: "Try Ship Mode — auto-send messages after dictation",
    shouldShow: (s) => !s.ship_mode,
    page: "settings",
  },
  {
    id: "gpu",
    text: "Try GPU Acceleration — faster transcription with Vulkan",
    shouldShow: (s) => !s.gpu_acceleration,
    page: "settings",
  },
  {
    id: "voice_commands",
    text: "Try voice commands — say \"new line\" or \"send\" while dictating",
    shouldShow: (s) => !s.voice_commands,
    page: "settings",
  },
  {
    id: "live_preview",
    text: "Try Live Preview — see words appear as you speak",
    shouldShow: (s) => !s.live_preview,
    page: "settings",
  },
  {
    id: "context_modes",
    text: "Try Context Modes — customize behavior per app",
    shouldShow: () => true,
    page: "modes",
  },
  {
    id: "noise_reduction",
    text: "Try Noise Reduction — filter background sounds with RNNoise",
    shouldShow: (s) => !s.noise_reduction,
    page: "settings",
  },
];

const DISMISSED_KEY = "omnivox_dismissed_tips";

function getDismissed(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(DISMISSED_KEY) ?? "[]"));
  } catch {
    return new Set();
  }
}

function dismissTip(id: string) {
  const dismissed = getDismissed();
  dismissed.add(id);
  localStorage.setItem(DISMISSED_KEY, JSON.stringify([...dismissed]));
}

function FeatureTip() {
  const [tip, setTip] = useState<Tip | null>(null);
  const setPage = useAppStore((s) => s.setPage);
  const status = useRecordingStore((s) => s.status);
  const isRecording = status === "recording";

  useEffect(() => {
    getSettings()
      .then((s) => {
        const dismissed = getDismissed();
        const available = TIPS.filter(
          (t) => !dismissed.has(t.id) && t.shouldShow(s)
        );
        if (available.length > 0) {
          // Pick a random tip so it feels fresh each session
          setTip(available[Math.floor(Math.random() * available.length)]);
        }
      })
      .catch(() => {});
  }, []);

  const handleDismiss = useCallback(() => {
    if (tip) {
      dismissTip(tip.id);
      setTip(null);
    }
  }, [tip]);

  const handleNavigate = useCallback(() => {
    if (tip) {
      dismissTip(tip.id);
      setPage(tip.page);
    }
  }, [tip, setPage]);

  if (!tip) return null;

  return (
    <div
      className={cn(
        "w-full max-w-lg mt-3 flex items-center gap-2 rounded-lg px-3 py-2",
        "border transition-colors duration-300 opacity-0 animate-fade-in",
        isRecording
          ? "bg-recording-500/5 border-recording-500/20"
          : "bg-surface-1/60 border-border/50"
      )}
      style={{ animationDelay: "400ms", animationFillMode: "forwards" }}
    >
      <p
        className={cn(
          "flex-1 text-xs transition-colors duration-300",
          isRecording ? "text-recording-400/80" : "text-text-muted"
        )}
      >
        {tip.text}
      </p>
      <button
        onClick={handleNavigate}
        className={cn(
          "shrink-0 p-1 rounded transition-colors",
          isRecording
            ? "text-recording-400/60 hover:text-recording-300"
            : "text-text-muted hover:text-text-secondary"
        )}
        title="Go to setting"
      >
        <ArrowRight size={13} strokeWidth={2} />
      </button>
      <button
        onClick={handleDismiss}
        className={cn(
          "shrink-0 p-1 rounded transition-colors",
          isRecording
            ? "text-recording-400/40 hover:text-recording-300"
            : "text-text-muted/50 hover:text-text-secondary"
        )}
        title="Dismiss"
      >
        <X size={12} strokeWidth={2} />
      </button>
    </div>
  );
}

/* ── Stats card ── */

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
