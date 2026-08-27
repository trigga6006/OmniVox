import { useEffect, useState, useCallback } from "react";
import { Copy, Check, X, ArrowRight } from "lucide-react";
import { RecordButton } from "./RecordButton";
import { AudioVisualizer } from "./AudioVisualizer";
import { useRecordingStore } from "@/stores/recordingStore";
import { useRecordingState } from "@/hooks/useRecordingState";
import {
  getSettings,
  getDictationStats,
  getActiveModel,
  getAudioDevices,
  getSelectedAudioDevice,
  type DictationStats,
  type AppSettings,
} from "@/lib/tauri";
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

  const hotkeys = settings?.hotkey?.labels?.length
    ? settings.hotkey.labels
    : ["Ctrl", "Alt"];

  const isIdle = status === "idle";
  const isRecording = status === "recording";
  const isProcessing = status === "processing";

  return (
    // overflow-hidden + the min-h-0 chain below are the no-scroll guarantee:
    // this page must NEVER scroll as a whole — only the transcription card's
    // text area scrolls when a dictation is long.
    <div className="relative flex h-full flex-col items-center overflow-hidden px-8 pt-6 pb-4">
      {/* Top spacer. It and the results zone below are both flex-1, so they
          always split the free space evenly: the stage sits at the optical
          centre AND the record button holds one y-position in every state,
          whether or not a transcription card is on screen. */}
      <div className="min-h-0 flex-1" aria-hidden="true" />

      {/* ── Stage: headline + hotkey keycaps + button + visualizer ───────
          Every line here has a fixed height, so nothing inside the stage can
          nudge the button between idle / recording / transcribing. */}
      <div className="flex shrink-0 flex-col items-center">
        <h1
          className={cn(
            "flex h-9 items-center font-display text-2xl font-semibold opacity-0 animate-fade-in",
            isRecording ? "text-amber-300" : "text-text-primary"
          )}
        >
          {isIdle && "Ready to listen"}
          {isRecording && "Listening…"}
          {isProcessing && "Transcribing…"}
          {status === "error" && "Something went wrong"}
        </h1>

        <div
          className="mt-2 flex h-7 items-center gap-1.5 text-sm text-text-muted opacity-0 animate-fade-in"
          style={{ animationDelay: "80ms" }}
        >
          {isIdle && (
            <>
              <span>Press</span>
              {/* Real keycaps, breathing while idle — the hotkey is the whole
                  interaction, so it gets to be the thing you see. */}
              {hotkeys.map((key) => (
                <Kbd key={key} className="animate-breathe px-2 py-1 text-xs">
                  {key}
                </Kbd>
              ))}
              <span>to begin</span>
            </>
          )}
          {isRecording && "Speak now — press again to stop"}
          {isProcessing && "Hang tight, processing your audio…"}
          {status === "error" && "Try recording again"}
        </div>

        <div
          className="my-5 shrink-0 opacity-0 animate-scale-in"
          style={{ animationDelay: "150ms" }}
        >
          <RecordButton />
        </div>

        {/* Audio Visualizer — occupies space but invisible when not recording */}
        <div
          className={cn(
            "h-11 shrink-0 transition-opacity duration-[var(--dur-3)] ease-out",
            isRecording ? "opacity-100" : "pointer-events-none opacity-0"
          )}
        >
          {isRecording && <AudioVisualizer />}
        </div>

        {/* Discovery hint — a fixed slot so the button never moves; a single
            quiet line, only while idle. */}
        <div className="mt-1 flex h-8 shrink-0 items-center">
          {isIdle && <FeatureTip settings={settings} />}
        </div>
      </div>

      {/* ── Results zone ─────────────────────────────────────
          The ONLY flexible region. overflow-hidden makes spill into the dock
          impossible by construction; justify-end anchors the card to the
          dock instead of floating it mid-void. */}
      <div className="flex min-h-0 w-full flex-1 flex-col items-center justify-end overflow-hidden pt-3 pb-4">
        {lastTranscription && (
          <TranscriptionCard text={lastTranscription} />
        )}
      </div>

      {/* ── Bottom dock: milestone hairline + words · milestone | model ·
          mic · style. Fixed height, always last, can never be overlapped. */}
      <BottomDock settings={settings} />
    </div>
  );
}

/* ── Bottom dock: stats + ambient status as one grounded bar ── */

const WRITING_STYLE_LABELS: Record<string, string> = {
  formal: "Formal",
  casual: "Casual",
  very_casual: "Very Casual",
};

/**
 * The page's grounded chrome: milestone progress as a hairline across the
 * top edge, then one 44px row — words · milestone on the left, the app's
 * live setup (model · mic · style) in mono on the right. Fixed height and
 * always the last child, so no window size can make anything overlap it.
 * Everything is read from bindings the app already has; whatever hasn't
 * resolved simply isn't shown.
 */
function BottomDock({ settings }: { settings: AppSettings | null }) {
  const [stats, setStats] = useState<DictationStats | null>(null);
  const lastTranscription = useRecordingStore((s) => s.lastTranscription);
  const [modelName, setModelName] = useState<string | null>(null);
  const [micName, setMicName] = useState<string | null>(null);

  useEffect(() => {
    getDictationStats().then(setStats).catch(() => {});
  }, []);

  useEffect(() => {
    if (lastTranscription) {
      getDictationStats().then(setStats).catch(() => {});
    }
  }, [lastTranscription]);

  useEffect(() => {
    let cancelled = false;
    Promise.all([
      getActiveModel().catch(() => null),
      getAudioDevices().catch(() => []),
      getSelectedAudioDevice().catch(() => null),
    ]).then(([model, devices, selectedId]) => {
      if (cancelled) return;
      setModelName(model?.name ?? null);
      const device =
        devices.find((d) => d.id === selectedId) ??
        devices.find((d) => d.is_default) ??
        devices[0];
      setMicName(device?.name ?? null);
    });
    return () => {
      cancelled = true;
    };
  }, []);

  const style = settings?.writing_style;
  const statusSegments = [
    modelName,
    micName,
    style ? (WRITING_STYLE_LABELS[style] ?? style) : null,
  ].filter(Boolean) as string[];

  const words = stats?.total_words ?? 0;
  const milestone = getCurrentMilestone(words);
  const next = getNextMilestone(words);
  const progress = next
    ? ((words - milestone.words) / (next.words - milestone.words)) * 100
    : 100;

  return (
    <footer
      className="w-full shrink-0 opacity-0 animate-fade-in"
      style={{ animationDelay: "250ms", animationFillMode: "forwards" }}
    >
      {/* Milestone progress lives in the dock's top hairline. */}
      <div className="h-px w-full bg-border/60">
        {words > 0 && next && (
          <div
            className="h-full bg-amber-400/70 transition-[width] duration-[var(--dur-4)] ease-out"
            style={{ width: `${Math.min(progress, 100)}%` }}
          />
        )}
      </div>
      <div className="flex h-11 w-full items-center justify-between gap-4">
        <div className="flex min-w-0 items-center gap-2 text-xs">
          {words > 0 ? (
            <>
              <span className="font-semibold tabular-nums text-amber-300">
                {words.toLocaleString()}
              </span>
              <span className="text-text-muted">words</span>
              <span className="text-text-muted/50">·</span>
              <span className="truncate text-text-muted">{milestone.label}</span>
              {next && (
                <span className="hidden whitespace-nowrap font-mono text-2xs tabular-nums text-text-muted/60 sm:inline">
                  → {next.words.toLocaleString()}
                </span>
              )}
            </>
          ) : (
            <span className="text-text-muted/70">No dictations yet</span>
          )}
        </div>
        {statusSegments.length > 0 && (
          <p className="flex min-w-0 shrink items-center gap-2 font-mono text-2xs text-text-muted">
            {statusSegments.map((segment, i) => (
              <span key={segment} className="flex min-w-0 items-center gap-2">
                {i > 0 && <span className="shrink-0 opacity-50">·</span>}
                <span className="truncate">{segment}</span>
              </span>
            ))}
          </p>
        )}
      </div>
    </footer>
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
    <Card className="flex min-h-0 w-full max-w-lg flex-col px-5 py-4 opacity-0 animate-slide-up">
      <div className="mb-2.5 flex shrink-0 items-center justify-between">
        <div className="flex items-center gap-2.5">
          <span className="eyebrow">
            Last transcription
          </span>
          <button
            onClick={() => setPage("history")}
            title="View all transcriptions"
            className="group inline-flex items-center gap-0.5 text-xs font-medium text-text-muted/60 transition-colors duration-[var(--dur-2)] ease-out hover:text-amber-300"
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
        <p className="select-text font-sans text-base leading-[1.65] text-text-primary">
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

  // A single quiet line inside the stage's fixed slot — discovery, not a
  // banner. Only rendered while idle, so no recording styling is needed.
  return (
    <div
      className="flex items-center gap-1 opacity-0 animate-fade-in"
      style={{ animationDelay: "400ms", animationFillMode: "forwards" }}
    >
      <button
        onClick={handleNavigate}
        className="group flex items-center gap-1.5 text-xs text-text-muted/70 transition-colors duration-[var(--dur-2)] ease-out hover:text-text-secondary"
      >
        {tip.text}
        <ArrowRight
          size={11}
          strokeWidth={2}
          className="opacity-50 transition-transform duration-[var(--dur-2)] group-hover:translate-x-0.5"
        />
      </button>
      <button
        onClick={handleDismiss}
        className="pressable rounded-[var(--radius-s)] p-1 text-text-muted/40 transition-colors duration-[var(--dur-2)] ease-out hover:text-text-secondary"
        title="Dismiss"
      >
        <X size={11} strokeWidth={2} />
      </button>
    </div>
  );
}

