import { useEffect, useRef, useState } from "react";
import {
  Copy,
  ClipboardPaste,
  Pencil,
  X,
  ChevronDown,
  FileText,
  Mic,
  Loader2,
} from "lucide-react";
import {
  pasteStructuredOutput,
  startRecording,
  stopRecording,
  onTranscriptionResult,
  type StructuredOutputPayload,
} from "@/lib/tauri";
import { useRecordingStore } from "@/stores/recordingStore";
import { cn } from "@/lib/utils";

interface Props {
  payload: StructuredOutputPayload;
  onClose: () => void;
  /**
   * Called whenever the panel enters/leaves "dictating-into-textarea" mode.
   * Parent (FloatingPill) uses this to (a) skip closing the panel when
   * recording starts and (b) drop any `structured-output-ready` event that
   * fires from the panel's own dictation pass.
   */
  onDictatingChange?: (active: boolean) => void;
}

/**
 * Structured Mode preview panel.
 *
 * Not a modal — dismissible at any time via ESC or the close button.  Paste
 * commits the current Markdown (possibly user-edited) through the active
 * OutputConfig.  Copy writes to the system clipboard.  Edit flips the preview
 * into a textarea so the user can tweak before pasting.
 */
export function StructuredPanel({ payload, onClose, onDictatingChange }: Props) {
  const [markdown, setMarkdown] = useState(payload.markdown);
  const [isEditing, setIsEditing] = useState(false);
  const [showRaw, setShowRaw] = useState(false);
  const [justCopied, setJustCopied] = useState(false);
  const [pasteError, setPasteError] = useState<string | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // ── Dictate-into-textarea state ─────────────────────────────────────
  //
  // Why we listen to `transcription-result` here instead of reading
  // `lastTranscription` from the store:
  // The overlay window's JS runtime is isolated from the main window's.
  // The global `transcription-result → setLastTranscription` wiring lives in
  // App.tsx (main window only), so in the overlay the store's
  // `lastTranscription` is never populated — waiting on it would deadlock
  // the append forever.  Subscribing to the event directly sidesteps the
  // cross-window store gap entirely.
  const [isDictating, setIsDictating] = useState(false);
  const recordingStatus = useRecordingStore((s) => s.status);
  // Mirror of `isDictating` readable from unmount cleanup without stale closure.
  const isDictatingRef = useRef(false);
  // Parent uses this to both (a) keep the panel alive during dictation and
  // (b) drop the `structured-output-ready` event fired by the dictation pass.
  // Flipping to `false` is delayed via a grace period in FloatingPill because
  // `transcription-result` and `structured-output-ready` are emitted
  // back-to-back in pipeline.rs — the parent must keep guarding until the
  // trailing event has been dropped.
  useEffect(() => {
    isDictatingRef.current = isDictating;
    onDictatingChange?.(isDictating);
  }, [isDictating, onDictatingChange]);

  // Reset local edits whenever the pipeline delivers a new payload.
  useEffect(() => {
    setMarkdown(payload.markdown);
    setIsEditing(false);
    setShowRaw(false);
    setPasteError(null);
  }, [payload]);

  // When the panel unmounts mid-dictation, make sure we don't leave the
  // recorder running in the background.
  useEffect(() => {
    return () => {
      if (isDictatingRef.current) {
        stopRecording().catch(() => {});
        onDictatingChange?.(false);
      }
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Hotkey support: when a recording starts by any means (mic button OR
  // global hotkey OR programmatic) while the panel is open, treat it as
  // dictation into the textarea.  Auto-enters edit mode so the user can
  // see the appended text land.
  const prevStatusRef = useRef(recordingStatus);
  useEffect(() => {
    const prev = prevStatusRef.current;
    prevStatusRef.current = recordingStatus;
    if (
      recordingStatus === "recording" &&
      prev !== "recording" &&
      !isDictatingRef.current
    ) {
      setIsEditing((e) => (e ? e : true));
      setIsDictating(true);
    }
  }, [recordingStatus]);

  // Capture the next `transcription-result` after dictation starts and
  // append it to the textarea.  A single-shot subscription — we tear it
  // down once the event is consumed so a subsequent dictation pass wires
  // up fresh.
  useEffect(() => {
    if (!isDictating) return;
    let active = true;
    let unlistenFn: (() => void) | null = null;
    const handler = (text: string) => {
      if (!active) return;
      const incoming = text.trim();
      if (!incoming) {
        setIsDictating(false);
        return;
      }
      setMarkdown((prev) => {
        const base = prev.replace(/\s+$/, "");
        if (!base) return incoming;
        // Land on a fresh line; structured markdown is line-oriented and
        // this preserves any list/heading the user was editing under.
        return `${base}\n${incoming}`;
      });
      setIsDictating(false);
      window.setTimeout(() => textareaRef.current?.focus(), 0);
    };
    const p = onTranscriptionResult(handler);
    p.then((fn) => {
      if (!active) {
        fn();
        return;
      }
      unlistenFn = fn;
    });
    return () => {
      active = false;
      if (unlistenFn) unlistenFn();
      else p.then((fn) => fn()).catch(() => {});
    };
  }, [isDictating]);

  // ESC dismiss and Cmd/Ctrl+Enter paste.
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        onClose();
      } else if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
        e.preventDefault();
        handlePaste();
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [markdown, onClose]);

  const handlePaste = async () => {
    try {
      await pasteStructuredOutput(markdown);
      onClose();
    } catch (err) {
      setPasteError(String(err));
    }
  };

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(markdown);
      setJustCopied(true);
      window.setTimeout(() => setJustCopied(false), 1100);
    } catch {
      // fall through — the Paste button still works via OutputRouter's clipboard path
    }
  };

  const slots = payload.slots;
  const fileCount = slots?.files?.length ?? 0;
  const urgency = slots?.urgency ?? null;
  const hasMetadata = fileCount > 0 || !!urgency;

  const handleToggleDictation = async () => {
    if (isDictating) {
      try {
        await stopRecording();
      } catch {
        // Status listeners will still eventually reset us to idle; swallow.
      }
      return;
    }
    // Auto-enter edit mode so the user actually sees their dictated text land
    // in the textarea.  The status-watch effect above will also flip
    // `isDictating` true once recording starts, but setting it here too
    // guarantees the transcription-result subscription is armed before the
    // backend has a chance to emit.
    if (!isEditing) setIsEditing(true);
    setIsDictating(true);
    try {
      await startRecording();
    } catch {
      setIsDictating(false);
    }
  };

  // During dictation the mic control is purely driven by recorder status;
  // disallow clicks while the pipeline is in the post-recording phases.
  const dictationPhase: "idle" | "recording" | "processing" = !isDictating
    ? "idle"
    : recordingStatus === "recording"
      ? "recording"
      : "processing";

  return (
    <div
      className={cn(
        "structured-panel",
        showRaw && "structured-panel--raw-open"
      )}
    >
      <div className="sp-bloom" aria-hidden="true" />
      <div className="sp-grain" aria-hidden="true" />
      <div className="sp-ring" aria-hidden="true" />

      {/* Header */}
      <div className="sp-header">
        <div className="sp-header-lead">
          <span className="sp-indicator" aria-hidden="true" />
          <span className="sp-kicker">Structured</span>
          <span className="sp-kicker-faint">· AI</span>
        </div>
        <button
          onClick={onClose}
          className="sp-close"
          aria-label="Close"
          title="Dismiss (Esc)"
        >
          <X size={11} strokeWidth={2.5} />
        </button>
      </div>

      {/* Metadata strip — only renders if the LLM populated any slot */}
      {hasMetadata && (
        <div className="sp-meta">
          {urgency && <UrgencyChip value={urgency} />}
          {fileCount > 0 && (
            <span className="sp-chip">
              <FileText size={9} strokeWidth={2.2} />
              {fileCount} {fileCount === 1 ? "file" : "files"}
            </span>
          )}
        </div>
      )}

      {/* Preview / editor */}
      <div className="sp-body">
        {isEditing ? (
          <textarea
            ref={textareaRef}
            value={markdown}
            onChange={(e) => setMarkdown(e.target.value)}
            className="sp-textarea"
            autoFocus
          />
        ) : (
          <MarkdownPreview markdown={markdown} />
        )}
      </div>

      {/* Raw disclosure */}
      <button
        onClick={() => setShowRaw((s) => !s)}
        className={cn("sp-raw-toggle", showRaw && "sp-raw-toggle--open")}
      >
        <ChevronDown size={10} strokeWidth={2.4} className="sp-raw-chev" />
        <span>Raw transcript</span>
      </button>
      <div className={cn("sp-raw", showRaw && "sp-raw--open")}>
        <div className="sp-raw-inner">
          <div className="sp-raw-rail" aria-hidden="true" />
          <p>{payload.raw_transcript}</p>
        </div>
      </div>

      {/* Error banner */}
      {pasteError && (
        <div className="sp-error">
          <span className="sp-error-dot" />
          <span className="sp-error-text">{pasteError}</span>
        </div>
      )}

      {/* Actions */}
      <div className="sp-actions">
        <button
          className="sp-btn sp-btn--primary"
          onClick={handlePaste}
          title="Paste into active app"
        >
          <ClipboardPaste size={11} strokeWidth={2.2} />
          <span>Paste</span>
          <kbd className="sp-kbd sp-kbd--primary">
            <span className="sp-kbd-mod">⌘</span>↵
          </kbd>
        </button>
        <button
          className={cn("sp-btn", justCopied && "sp-btn--confirm")}
          onClick={handleCopy}
          title="Copy Markdown to clipboard"
        >
          <Copy size={11} strokeWidth={2.2} />
          <span>{justCopied ? "Copied" : "Copy"}</span>
        </button>
        <button
          className={cn("sp-btn", isEditing && "sp-btn--active")}
          onClick={() => setIsEditing((e) => !e)}
          title={isEditing ? "Finish editing" : "Edit before paste"}
        >
          <Pencil size={11} strokeWidth={2.2} />
          <span>{isEditing ? "Done" : "Edit"}</span>
        </button>
        <div className="sp-spacer" />
        <button
          className="sp-btn sp-btn--ghost"
          onClick={onClose}
          title="Dismiss panel"
        >
          <span>Dismiss</span>
          <kbd className="sp-kbd sp-kbd--ghost">ESC</kbd>
        </button>
        <button
          className={cn(
            "sp-mic",
            dictationPhase === "recording" && "sp-mic--recording",
            dictationPhase === "processing" && "sp-mic--processing"
          )}
          onClick={handleToggleDictation}
          disabled={dictationPhase === "processing"}
          aria-label={
            dictationPhase === "recording"
              ? "Stop dictation"
              : "Dictate into preview"
          }
          title={
            dictationPhase === "recording"
              ? "Stop dictation"
              : dictationPhase === "processing"
                ? "Transcribing…"
                : "Dictate into preview"
          }
        >
          {dictationPhase === "recording" ? (
            <span className="sp-mic-wave">
              <MiniWaveform color="rgba(248,200,130,0.95)" />
            </span>
          ) : dictationPhase === "processing" ? (
            <Loader2
              size={11}
              strokeWidth={2.2}
              className="sp-mic-spin"
            />
          ) : (
            <Mic size={11} strokeWidth={2.2} />
          )}
        </button>
      </div>

      <style>{styles}</style>
    </div>
  );
}

/**
 * Compact waveform tuned for the 62×24 mic pill in the action bar.
 * Full PillWaveform is 46×18 and doesn't leave room for the pill's
 * rounded corners once the action-bar button neighbors are factored in;
 * this variant is 7 bars × ~2px with a 14px ceiling so it never clips.
 */
function MiniWaveform({ color }: { color: string }) {
  const audioLevel = useRecordingStore((s) => s.audioLevel);
  const WEIGHTS = [0.45, 0.7, 0.9, 1.0, 0.9, 0.7, 0.45];
  const MIN = 3;
  const MAX = 14;
  return (
    <div className="sp-mini-wave" aria-hidden="true">
      {WEIGHTS.map((w, i) => {
        const level = Math.min(1, audioLevel * w);
        const h = MIN + level * (MAX - MIN);
        return (
          <span
            key={i}
            className="sp-mini-wave-bar"
            style={{
              height: `${h}px`,
              backgroundColor: color,
              opacity: 0.65 + level * 0.3,
            }}
          />
        );
      })}
    </div>
  );
}

function UrgencyChip({ value }: { value: "low" | "normal" | "high" }) {
  const tone = {
    low: {
      bg: "rgba(110,128,140,0.14)",
      border: "rgba(150,170,185,0.16)",
      fg: "rgba(200,215,225,0.85)",
      dot: "rgba(170,190,205,0.85)",
      label: "Low",
    },
    normal: {
      bg: "rgba(160,120,50,0.14)",
      border: "rgba(232,180,95,0.22)",
      fg: "rgba(240,208,150,0.95)",
      dot: "rgba(244,190,110,0.95)",
      label: "Normal",
    },
    high: {
      bg: "rgba(190,64,64,0.16)",
      border: "rgba(248,140,130,0.26)",
      fg: "rgba(252,195,185,0.96)",
      dot: "rgba(250,140,125,1)",
      label: "Urgent",
    },
  }[value];
  return (
    <span
      className="sp-chip"
      style={{
        backgroundColor: tone.bg,
        borderColor: tone.border,
        color: tone.fg,
      }}
    >
      <span className="sp-chip-dot" style={{ backgroundColor: tone.dot }} />
      {tone.label}
    </span>
  );
}

/**
 * Minimal Markdown renderer — handles headings (##), unordered lists (- ),
 * and inline code (`…`).  The Structured Mode template only uses these
 * features, so we avoid pulling in a full Markdown library just for the panel.
 */
function MarkdownPreview({ markdown }: { markdown: string }) {
  const lines = markdown.split("\n");
  const elements: React.ReactNode[] = [];
  let listBuffer: React.ReactNode[] = [];

  const flushList = (key: string) => {
    if (listBuffer.length) {
      elements.push(
        <ul key={`list-${key}`} className="sp-list">
          {listBuffer}
        </ul>
      );
      listBuffer = [];
    }
  };

  lines.forEach((line, i) => {
    if (line.startsWith("## ")) {
      flushList(String(i));
      elements.push(
        <div key={`h-${i}`} className="sp-h">
          <span className="sp-h-rule" aria-hidden="true" />
          <span className="sp-h-text">{line.slice(3)}</span>
        </div>
      );
    } else if (line.startsWith("- ")) {
      listBuffer.push(
        <li key={`li-${i}`} className="sp-li">
          {renderInline(line.slice(2))}
        </li>
      );
    } else if (line.trim() === "") {
      flushList(String(i));
    } else {
      flushList(String(i));
      elements.push(
        <p key={`p-${i}`} className="sp-p">
          {renderInline(line)}
        </p>
      );
    }
  });
  flushList("end");

  return <div className="sp-md">{elements}</div>;
}

/** Render `inline code` spans — very small subset, good enough for the template. */
function renderInline(text: string): React.ReactNode[] {
  const parts: React.ReactNode[] = [];
  const regex = /`([^`]+)`/g;
  let last = 0;
  let match: RegExpExecArray | null;
  let idx = 0;
  while ((match = regex.exec(text))) {
    if (match.index > last) {
      parts.push(text.slice(last, match.index));
    }
    parts.push(
      <code key={`c-${idx++}`} className="sp-code">
        {match[1]}
      </code>
    );
    last = match.index + match[0].length;
  }
  if (last < text.length) parts.push(text.slice(last));
  return parts.length ? parts : [text];
}

const styles = `
/* ══════════════════════════════════════════════════════════════
   Structured Panel — refined premium surface
   Warm charcoal base + violet atmospheric bloom + gold inline code
   ══════════════════════════════════════════════════════════════ */

.structured-panel {
  position: relative;
  width: 420px;
  /* Match the right-click menu spacing pattern: 4 px gap between the
     panel and the pill below (same as the ModeSelector margin-bottom).
     With a real gap the panel no longer merges visually with the pill,
     so bottom corners go back to the full 14 px radius and the bottom
     border becomes visible again for a cleanly bounded shape. */
  margin-bottom: 4px;
  border-radius: 14px;
  background:
    linear-gradient(180deg,
      rgba(28,26,24,1) 0%,
      rgba(22,21,20,1) 100%);
  border: 1px solid rgba(255,255,255,0.055);
  overflow: hidden;
  isolation: isolate;
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.05),
    0 1px 2px rgba(0,0,0,0.5),
    0 10px 24px -8px rgba(0,0,0,0.75),
    0 24px 48px -14px rgba(0,0,0,0.85);
  /* "Reverse Dynamic Island": the panel starts as a pill-sized spot at
     the bottom-centre and grows outward to full size.  420ms with
     easeOutQuint reads as smooth but not ceremonial — longer durations
     start to feel like a macOS sheet. */
  transform-origin: bottom center;
  animation: sp-in 420ms cubic-bezier(0.22, 1, 0.36, 1) both;
}

/* Atmosphere layers ------------------------------------------- */
/* Soft warm top spotlight — like ambient light catching a console */
.sp-bloom {
  position: absolute;
  inset: -60% -20% auto -20%;
  height: 200px;
  background:
    radial-gradient(ellipse at 50% 0%,
      rgba(255,238,210,0.035) 0%,
      rgba(255,230,190,0.018) 28%,
      transparent 60%);
  pointer-events: none;
  z-index: 0;
}
/* Neutral warm rim-light along the very top edge */
.sp-ring {
  position: absolute;
  top: 0; left: 0; right: 0;
  height: 1px;
  background: linear-gradient(90deg,
    transparent 0%,
    rgba(255,235,200,0.08) 25%,
    rgba(255,240,210,0.14) 50%,
    rgba(255,235,200,0.08) 75%,
    transparent 100%);
  pointer-events: none;
  z-index: 2;
}
/* Warm charcoal grain — tactile depth */
.sp-grain {
  position: absolute;
  inset: 0;
  background-image: url("data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='160' height='160'><filter id='n'><feTurbulence type='fractalNoise' baseFrequency='0.85' numOctaves='2' stitchTiles='stitch'/><feColorMatrix values='0 0 0 0 0.7  0 0 0 0 0.62  0 0 0 0 0.55  0 0 0 0.05 0'/></filter><rect width='100%' height='100%' filter='url(%23n)'/></svg>");
  opacity: 0.4;
  mix-blend-mode: overlay;
  pointer-events: none;
  z-index: 1;
}

.structured-panel > *:not(.sp-bloom):not(.sp-grain):not(.sp-ring) {
  position: relative;
  z-index: 3;
}

/* Header ------------------------------------------------------- */
.sp-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 11px 13px 9px;
}
.sp-header-lead {
  display: flex;
  align-items: center;
  gap: 7px;
}
.sp-indicator {
  display: inline-block;
  width: 6px;
  height: 6px;
  border-radius: 50%;
  background: rgb(244,190,110);
  box-shadow:
    0 0 0 1.5px rgba(244,190,110,0.2),
    0 0 8px rgba(244,190,110,0.55);
  animation: sp-indicator-breathe 2.6s ease-in-out infinite;
}
.sp-kicker {
  font-family: var(--font-display);
  font-size: 9.5px;
  font-weight: 700;
  text-transform: uppercase;
  letter-spacing: 0.22em;
  color: rgba(240,218,182,0.95);
}
.sp-kicker-faint {
  font-family: var(--font-display);
  font-size: 9.5px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.22em;
  color: rgba(220,190,150,0.4);
}
.sp-close {
  display: grid;
  place-items: center;
  width: 18px;
  height: 18px;
  border-radius: 6px;
  color: rgba(255,255,255,0.42);
  background: transparent;
  border: none;
  cursor: pointer;
  transition:
    background 140ms ease,
    color 140ms ease,
    transform 160ms ease;
}
.sp-close:hover {
  background: rgba(255,255,255,0.07);
  color: rgba(255,255,255,0.88);
}
.sp-close:active { transform: scale(0.92); }

/* Metadata chips ---------------------------------------------- */
.sp-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 5px;
  padding: 0 13px 10px;
}
.sp-chip {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 2.5px 8px;
  border-radius: 999px;
  background: rgba(255,255,255,0.04);
  color: rgba(235,232,245,0.7);
  font-family: var(--font-sans);
  font-size: 9.5px;
  font-weight: 500;
  letter-spacing: 0.01em;
  border: 1px solid rgba(255,255,255,0.055);
  white-space: nowrap;
}
.sp-chip svg { opacity: 0.85; }
.sp-chip-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
}
.sp-chip--format {
  font-family: var(--font-mono);
  font-size: 9px;
  letter-spacing: 0;
}

/* Body --------------------------------------------------------- */
.sp-body {
  max-height: 300px;
  min-height: 140px;
  overflow-y: auto;
  padding: 2px 14px 14px;
  border-top: 1px solid rgba(255,255,255,0.04);
  scrollbar-width: thin;
  scrollbar-color: rgba(232,180,95,0.3) transparent;
  transition: max-height 240ms cubic-bezier(0.4, 0, 0.2, 1);
}

/*
 * When the raw-transcript drawer is open the body area shrinks so the
 * total panel height stays within the 480 px overlay window.  Without
 * this cap, opening "Raw transcript" pushes the header and the top of
 * the body above the window's visible edge — and because the overlay
 * window itself doesn't scroll, that content becomes completely
 * unreachable.  The body still scrolls internally, so no structured
 * content is lost; you just see a smaller window of it at a time.
 */
.structured-panel--raw-open .sp-body {
  max-height: 140px;
}
.sp-body::-webkit-scrollbar { width: 4px; }
.sp-body::-webkit-scrollbar-track { background: transparent; }
.sp-body::-webkit-scrollbar-thumb {
  background: rgba(232,180,95,0.25);
  border-radius: 2px;
}
.sp-body::-webkit-scrollbar-thumb:hover {
  background: rgba(232,180,95,0.45);
}

/* Markdown typography ----------------------------------------- */
.sp-md { font-family: var(--font-sans); padding-top: 4px; }
.sp-h {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 13px 0 5px;
}
.sp-h:first-child { margin-top: 4px; }
.sp-h-rule {
  width: 10px;
  height: 1px;
  background: linear-gradient(90deg,
    rgba(196,170,230,0.55),
    rgba(196,170,230,0));
  flex-shrink: 0;
}
.sp-h-text {
  font-family: var(--font-display);
  font-weight: 600;
  font-size: 9.5px;
  text-transform: uppercase;
  letter-spacing: 0.18em;
  color: rgba(200,175,236,0.82);
}
.sp-p {
  margin: 0 0 6px 0;
  font-size: 11.5px;
  line-height: 1.55;
  color: rgba(232,228,240,0.88);
  letter-spacing: -0.005em;
}
.sp-list {
  margin: 2px 0 8px;
  padding: 0;
  list-style: none;
}
.sp-li {
  position: relative;
  padding: 2px 0 2px 14px;
  font-size: 11.5px;
  line-height: 1.55;
  color: rgba(232,228,240,0.88);
  letter-spacing: -0.005em;
}
.sp-li::before {
  content: "";
  position: absolute;
  left: 3px;
  top: 9px;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: rgba(186,148,234,0.55);
  box-shadow: 0 0 6px rgba(186,148,234,0.35);
}
.sp-code {
  display: inline-block;
  padding: 1px 5px;
  margin: 0 1px;
  border-radius: 4px;
  background: rgba(232,180,95,0.08);
  border: 1px solid rgba(232,180,95,0.14);
  color: rgba(250,215,160,0.95);
  font-family: var(--font-mono);
  font-size: 10.5px;
  font-weight: 500;
  letter-spacing: 0;
}

/* Editor ------------------------------------------------------- */
.sp-textarea {
  width: 100%;
  min-height: 200px;
  background: transparent;
  border: none;
  outline: none;
  resize: none;
  color: rgba(232,228,240,0.92);
  font-family: var(--font-mono);
  font-size: 11px;
  line-height: 1.6;
  padding: 8px 0 4px;
  letter-spacing: 0;
}

/* Raw disclosure ---------------------------------------------- */
.sp-raw-toggle {
  display: flex;
  align-items: center;
  gap: 5px;
  width: 100%;
  padding: 7px 13px;
  background: transparent;
  border: none;
  border-top: 1px solid rgba(255,255,255,0.04);
  cursor: pointer;
  color: rgba(255,255,255,0.36);
  font-family: var(--font-display);
  font-size: 9px;
  font-weight: 500;
  text-transform: uppercase;
  letter-spacing: 0.16em;
  transition: color 140ms ease, background 140ms ease;
}
.sp-raw-toggle:hover {
  color: rgba(255,255,255,0.72);
  background: rgba(255,255,255,0.015);
}
.sp-raw-chev {
  transition: transform 220ms cubic-bezier(0.4, 0, 0.2, 1);
}
.sp-raw-toggle--open .sp-raw-chev { transform: rotate(-180deg); }

.sp-raw {
  overflow: hidden;
  max-height: 0;
  transition:
    max-height 260ms cubic-bezier(0.4, 0, 0.2, 1),
    border-color 240ms ease;
  border-top: 1px solid transparent;
}
.sp-raw--open {
  max-height: 180px;
  border-top: 1px solid rgba(255,255,255,0.04);
}
.sp-raw-inner {
  position: relative;
  padding: 10px 13px 10px 22px;
  max-height: 180px;
  overflow-y: auto;
}
.sp-raw-rail {
  position: absolute;
  left: 13px;
  top: 10px;
  bottom: 10px;
  width: 1.5px;
  border-radius: 1px;
  background: linear-gradient(180deg,
    rgba(186,148,234,0.45),
    rgba(186,148,234,0.05));
}
.sp-raw p {
  margin: 0;
  font-family: var(--font-mono);
  font-size: 10px;
  line-height: 1.65;
  color: rgba(205,200,220,0.62);
}

/* Error -------------------------------------------------------- */
.sp-error {
  display: flex;
  align-items: center;
  gap: 7px;
  padding: 7px 13px;
  border-top: 1px solid rgba(240,110,110,0.22);
  background: linear-gradient(90deg, rgba(200,60,60,0.12), rgba(200,60,60,0.03));
  color: rgba(252,202,202,0.95);
  font-family: var(--font-sans);
  font-size: 10.5px;
  letter-spacing: -0.005em;
}
.sp-error-dot {
  width: 5px;
  height: 5px;
  border-radius: 50%;
  background: rgba(252,130,130,0.95);
  box-shadow: 0 0 6px rgba(252,130,130,0.6);
  flex-shrink: 0;
}
.sp-error-text { flex: 1; }

/* Actions ------------------------------------------------------ */
.sp-actions {
  display: flex;
  align-items: center;
  gap: 5px;
  padding: 8px 10px 10px;
  border-top: 1px solid rgba(255,255,255,0.05);
  background: linear-gradient(180deg,
    rgba(0,0,0,0) 0%,
    rgba(0,0,0,0.22) 100%);
}
.sp-btn {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 4.5px 9px;
  border-radius: 7px;
  border: 1px solid rgba(255,255,255,0.07);
  background: rgba(255,255,255,0.035);
  color: rgba(232,228,240,0.8);
  font-family: var(--font-sans);
  font-size: 10.5px;
  font-weight: 500;
  letter-spacing: -0.005em;
  cursor: pointer;
  transition:
    background 140ms ease,
    color 140ms ease,
    border-color 140ms ease,
    transform 140ms ease,
    box-shadow 140ms ease;
}
.sp-btn:hover {
  background: rgba(255,255,255,0.07);
  color: rgba(255,255,255,0.95);
  border-color: rgba(255,255,255,0.12);
}
.sp-btn:active { transform: translateY(0.5px); }

.sp-btn--primary {
  background: linear-gradient(180deg,
    rgba(168,124,226,0.36) 0%,
    rgba(138,98,200,0.24) 100%);
  border-color: rgba(188,150,236,0.38);
  color: rgba(248,238,255,0.96);
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.1),
    inset 0 -1px 0 rgba(0,0,0,0.1),
    0 1px 10px -2px rgba(138,98,200,0.5);
}
.sp-btn--primary:hover {
  background: linear-gradient(180deg,
    rgba(180,134,238,0.46) 0%,
    rgba(152,110,216,0.34) 100%);
  border-color: rgba(208,176,242,0.55);
  box-shadow:
    inset 0 1px 0 rgba(255,255,255,0.14),
    inset 0 -1px 0 rgba(0,0,0,0.1),
    0 2px 14px -2px rgba(148,108,210,0.6);
}

.sp-btn--active {
  background: rgba(232,180,95,0.12);
  border-color: rgba(232,180,95,0.28);
  color: rgba(250,215,160,0.95);
}
.sp-btn--active:hover {
  background: rgba(232,180,95,0.18);
  border-color: rgba(232,180,95,0.4);
  color: rgba(252,225,175,1);
}

.sp-btn--confirm {
  background: rgba(110,200,140,0.14);
  border-color: rgba(120,210,150,0.3);
  color: rgba(190,240,205,0.98);
}

.sp-btn--ghost {
  background: transparent;
  border: 1px solid transparent;
  color: rgba(255,255,255,0.4);
  padding: 4.5px 7px 4.5px 9px;
}
.sp-btn--ghost:hover {
  background: rgba(255,255,255,0.04);
  color: rgba(255,255,255,0.78);
  border-color: transparent;
}

.sp-kbd {
  display: inline-flex;
  align-items: center;
  gap: 1px;
  margin-left: 3px;
  padding: 1px 5px;
  border-radius: 4px;
  background: rgba(0,0,0,0.3);
  border: 1px solid rgba(255,255,255,0.08);
  font-family: var(--font-mono);
  font-size: 9px;
  font-weight: 500;
  color: rgba(255,255,255,0.6);
  letter-spacing: 0;
  line-height: 1;
}
.sp-kbd--primary {
  background: rgba(0,0,0,0.28);
  border-color: rgba(255,255,255,0.14);
  color: rgba(240,228,255,0.85);
}
.sp-kbd--ghost {
  background: transparent;
  border-color: rgba(255,255,255,0.09);
  color: rgba(255,255,255,0.42);
  padding: 1px 4px;
}
.sp-kbd-mod {
  margin-right: 1px;
  font-size: 9.5px;
}
.sp-spacer { flex: 1 1 auto; }

/* Dictation mic ------------------------------------------------ */
.sp-mic {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  height: 24px;
  width: 24px;
  padding: 0;
  margin-left: 2px;
  border-radius: 999px;
  background: rgba(232,180,95,0.08);
  border: 1px solid rgba(232,180,95,0.28);
  color: rgba(244,215,165,0.85);
  cursor: pointer;
  overflow: hidden;
  flex-shrink: 0;
  transition:
    width 240ms cubic-bezier(0.16, 1, 0.3, 1),
    background 180ms ease,
    border-color 180ms ease,
    box-shadow 220ms ease,
    color 140ms ease;
}
.sp-mic:hover:not(:disabled) {
  background: rgba(232,180,95,0.14);
  border-color: rgba(232,180,95,0.42);
  color: rgba(250,225,180,0.98);
}
.sp-mic:active:not(:disabled) { transform: scale(0.94); }
.sp-mic:disabled { cursor: default; }

.sp-mic--recording {
  width: 62px;
  background: rgba(232,180,95,0.18);
  border-color: rgba(240,200,120,0.55);
  box-shadow:
    inset 0 0 0 1px rgba(255,255,255,0.03),
    0 0 12px -2px rgba(232,180,95,0.5);
  animation: sp-mic-breathe 2.2s ease-in-out infinite;
}
.sp-mic--processing {
  background: rgba(232,180,95,0.14);
  border-color: rgba(232,180,95,0.36);
  color: rgba(244,215,165,0.92);
}
.sp-mic-wave {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  padding: 0 7px;
}
.sp-mini-wave {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 1.5px;
  height: 14px;
}
.sp-mini-wave-bar {
  width: 1.75px;
  border-radius: 1px;
  transform-origin: center;
  transition: height 100ms ease-out;
}
.sp-mic-spin { animation: sp-mic-spin 900ms linear infinite; }

@keyframes sp-mic-breathe {
  0%, 100% {
    box-shadow:
      inset 0 0 0 1px rgba(255,255,255,0.03),
      0 0 10px -2px rgba(232,180,95,0.4);
  }
  50% {
    box-shadow:
      inset 0 0 0 1px rgba(255,255,255,0.05),
      0 0 18px -2px rgba(232,180,95,0.65);
  }
}
@keyframes sp-mic-spin {
  from { transform: rotate(0deg); }
  to { transform: rotate(360deg); }
}

/* Keyframes --------------------------------------------------- */
/*
 * Reverse-Dynamic-Island expansion.
 *
 * The clip-path starts as a small rounded rectangle centred along the
 * panel's bottom edge, sized to roughly match the pill below (~56×26
 * with fully-rounded corners).  It then grows outward to the panel's
 * final shape (420×h with 14px top / 4px bottom corners).  Because the
 * clip interpolates both the inset distances AND the corner radii, the
 * visible shape morphs continuously from capsule to rounded rectangle
 * — the eye reads this as the panel flowing out of the pill rather
 * than dropping in from above.
 *
 * Content inside the panel renders at its final size throughout (no
 * scaleY, so text doesn't squish).  A short blur-clear smooths the
 * moment when content first becomes visible, and the opacity fade
 * happens in the first ~35% of the animation so the ramp doesn't feel
 * laggy after the shape has largely formed.
 */
@keyframes sp-in {
  from {
    opacity: 0;
    clip-path: inset(95% 43% 0% 43% round 13px 13px 13px 13px);
    filter: blur(4px);
  }
  35% {
    opacity: 1;
    filter: blur(1.5px);
  }
  to {
    opacity: 1;
    clip-path: inset(0 0 0 0 round 14px 14px 14px 14px);
    filter: blur(0);
  }
}
@keyframes sp-indicator-breathe {
  0%, 100% {
    opacity: 0.78;
    box-shadow:
      0 0 0 1.5px rgba(244,190,110,0.22),
      0 0 6px rgba(244,190,110,0.5);
  }
  50% {
    opacity: 1;
    box-shadow:
      0 0 0 2.5px rgba(244,190,110,0.32),
      0 0 14px rgba(244,190,110,0.78);
  }
}
`;
