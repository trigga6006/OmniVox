import { useEffect, useState, useCallback } from "react";
import { Copy, Check, X, ArrowRight } from "lucide-react";
import { RecordButton } from "./RecordButton";
import { AudioVisualizer } from "./AudioVisualizer";
import { useRecordingStore } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import { getSettings, getDictationStats, type DictationStats, type AppSettings } from "@/lib/tauri";
import { useAppStore } from "@/stores/appStore";
import { Button, Card, Kbd } from "@/components/ui";
import { cn } from "@/lib/utils";

export function DictationPanel() {
  // Wire up Tauri event listeners for recording state, audio level, transcription
  useRecordingState();

  const status = useRecordingStore((s) => s.status);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);

  // One settings fetch for the whole page — hotkey label here, feature-tip
  // filtering below via prop.
  const [settings, setSettings] = useState<AppSettings | null>(null);

  useEffect(() => {
    getSettings().then(setSettings).catch(() => {});
  }, []);

  const hotkeyLabel = settings?.hotkey?.labels?.length
    ? settings.hotkey.labels.join(" + ")
    : "Ctrl + Alt";

  const isIdle = status === "idle";
  const isRecording = status === "recording";
  const isProcessing = status === "processing";

  return (
    // overflow-hidden + the min-h-0 chain below are the no-scroll guarantee:
    // this page must NEVER scroll as a whole — only the transcription card's
    // text area scrolls when a dictation is long.
    <div className="relative flex h-full flex-col items-center overflow-hidden px-8 py-6">
      {/* ── Top section: headline + instruction ───────────────
          Fixed-height lines (not flex-1): everything above the record
          button has constant height in every state, so the button sits at
          the exact same y-position whether idle, recording, or
          transcribing.  (The kbd chip is a few px taller than the plain
          status text — without the fixed line heights that alone nudged
          the button between states.) */}
      <div className="flex shrink-0 flex-col items-center">
        <h1
          className={cn(
            "flex h-10 items-center font-display text-[2rem] font-semibold tracking-[-0.022em] opacity-0 animate-fade-in",
            isIdle && "text-text-primary",
            isRecording && "text-amber-300",
            isProcessing && "text-text-secondary"
          )}
        >
          {isIdle && "Ready to listen"}
          {isRecording && "Listening…"}
          {isProcessing && "Transcribing…"}
          {status === "error" && "Something went wrong"}
        </h1>

        <p
          className="mt-2 flex h-7 items-center text-sm text-text-muted opacity-0 animate-fade-in"
          style={{ animationDelay: "80ms" }}
        >
          {isIdle && (
            <>
              Press <Kbd className="mx-1.5">{hotkeyLabel}</Kbd> to begin
            </>
          )}
          {isRecording && "Speak now — press again to stop"}
          {isProcessing && "Hang tight, processing your audio…"}
          {status === "error" && "Try recording again"}
        </p>
      </div>

      {/* ── Center: Record Button (fixed position) ─────────── */}
      <div className="my-4 shrink-0 opacity-0 animate-scale-in" style={{ animationDelay: "150ms" }}>
        <RecordButton />
      </div>

      {/* ── Bottom section: visualizer + transcription ───────
          min-h-0 lets this section absorb whatever height remains and
          compress its content instead of pushing the button up or
          overflowing the page. */}
      <div className="flex min-h-0 w-full flex-1 flex-col items-center">
        {/* Audio Visualizer — occupies space but invisible when not recording */}
        <div
          className={cn(
            "shrink-0 transition-opacity duration-300",
            isRecording ? "opacity-100" : "opacity-0 pointer-events-none"
          )}
          style={{ height: 44 }}
        >
          {isRecording && <AudioVisualizer />}
        </div>

        <div className="h-4 shrink-0" />

        {/* ── Word count & milestone ─────────────────────────── */}
        <StatsCard />

        {/* ── Feature discovery tip ────────────────────────────── */}
        <FeatureTip settings={settings} />

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
  const setPage = useAppStore((s) => s.setPage);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(text).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    }).catch(() => {});
  }, [text]);

  return (
    // min-h-0 + internal overflow: the card grows naturally for short
    // dictations, but when space runs out it compresses and ONLY the text
    // area scrolls — the page itself never does.
    <Card className="mt-4 flex min-h-0 w-full max-w-lg flex-col px-5 py-4 opacity-0 animate-slide-up">
      <div className="mb-2.5 flex shrink-0 items-center justify-between">
        <div className="flex items-center gap-2.5">
          <span className="font-mono text-[10px] font-semibold uppercase tracking-[0.2em] text-text-muted">
            Last transcription
          </span>
          <button
            onClick={() => setPage("history")}
            title="View all transcriptions"
            className="group inline-flex items-center gap-0.5 text-[10px] font-medium text-text-muted/60 transition-colors hover:text-amber-300"
          >
            All transcriptions
            <ArrowRight
              size={10}
              strokeWidth={2}
              className="opacity-60 transition-transform group-hover:translate-x-0.5"
            />
          </button>
        </div>
        <Button
          variant="ghost"
          size="sm"
          icon={copied ? <Check className="text-success" /> : <Copy />}
          onClick={handleCopy}
        >
          {copied ? "Copied" : "Copy"}
        </Button>
      </div>
      <div className="min-h-0 overflow-y-auto">
        <p className="font-sans text-[15px] leading-[1.65] text-text-primary select-text">
          {text}
        </p>
      </div>
    </Card>
  );
}

/* ── Milestones ── */

// Milestone labels above 100k reference real word counts from
// well-known books and collected works.
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
  { words: 125000, label: "The Great Gatsby × 2.5" },
  { words: 150000, label: "Literary Luminary" },
  { words: 200000, label: "Fellowship Scribe" },
  { words: 250000, label: "Moby-Dick Whisperer" },
  { words: 300000, label: "Epic Pen" },
  { words: 400000, label: "Saga Weaver" },
  { words: 500000, label: "Voice of an Era" },
  { words: 587000, label: "Tolstoy's Peer" },
  { words: 650000, label: "Atlas Lifter" },
  { words: 783000, label: "Scripturist" },
  { words: 884000, label: "The Bard Incarnate" },
  { words: 1000000, label: "Million-Word Sage" },
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
    text: "Try voice commands — say “new line” or “send” while dictating",
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

function FeatureTip({ settings }: { settings: AppSettings | null }) {
  const [tip, setTip] = useState<Tip | null>(null);
  const setPage = useAppStore((s) => s.setPage);
  const status = useRecordingStore((s) => s.status);
  const isRecording = status === "recording";

  useEffect(() => {
    if (!settings) return;
    const dismissed = getDismissed();
    const available = TIPS.filter(
      (t) => !dismissed.has(t.id) && t.shouldShow(settings)
    );
    if (available.length > 0) {
      setTip(available[Math.floor(Math.random() * available.length)]);
    }
  }, [settings]);

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
        "w-full max-w-lg mt-3 flex shrink-0 items-center gap-2 rounded-lg px-3 py-2",
        "border transition-colors duration-300 opacity-0 animate-fade-in",
        isRecording
          ? "bg-recording-500/[0.06] border-recording-500/20"
          : "bg-surface-1/55 border-border/45"
      )}
      style={{ animationDelay: "400ms", animationFillMode: "forwards" }}
    >
      <p
        className={cn(
          "flex-1 text-xs transition-colors duration-300",
          isRecording ? "text-recording-400/85" : "text-text-secondary/85"
        )}
      >
        {tip.text}
      </p>
      <button
        onClick={handleNavigate}
        className={cn(
          "shrink-0 p-1 rounded-md transition-colors",
          isRecording
            ? "text-recording-400/60 hover:text-recording-300"
            : "text-text-muted hover:text-text-secondary hover:bg-surface-2/60"
        )}
        title="Go to setting"
      >
        <ArrowRight size={13} strokeWidth={2} />
      </button>
      <button
        onClick={handleDismiss}
        className={cn(
          "shrink-0 p-1 rounded-md transition-colors",
          isRecording
            ? "text-recording-400/40 hover:text-recording-300"
            : "text-text-muted/60 hover:text-text-secondary hover:bg-surface-2/60"
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
    <div
      className="w-full max-w-lg shrink-0 opacity-0 animate-fade-in"
      style={{ animationDelay: "200ms", animationFillMode: "forwards" }}
    >
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-xs font-semibold tabular-nums text-amber-300">
            {stats.total_words.toLocaleString()} words
          </span>
          <span className="text-xs text-text-muted/60">·</span>
          <span className="text-xs text-text-muted">{milestone.label}</span>
        </div>
        {next && (
          <span className="text-[11px] tabular-nums text-text-muted/80">
            {next.words.toLocaleString()} next
          </span>
        )}
      </div>
      {next && (
        <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-surface-2">
          <div
            className="h-full rounded-full bg-amber-400/55 transition-all duration-700 ease-out"
            style={{ width: `${Math.min(progress, 100)}%` }}
          />
        </div>
      )}
    </div>
  );
}
