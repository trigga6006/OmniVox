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
import "./StructuredPanel.css";

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

  // Escape hatch for "I don't like what the LLM did to my dictation."
  // Pastes the pre-structuring ASR text (the same string shown in the
  // Raw Transcript drawer), so the user never loses what they said.
  // Reuses the same paste_structured_output command — the Rust side
  // just invokes OutputRouter.send() on whatever string we hand it.
  const handlePasteRaw = async () => {
    try {
      await pasteStructuredOutput(payload.raw_transcript);
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

      {/* Actions — labels + kbd hints collapse to icon-only while the
          user is dictating into the textarea, freeing room for the mic
          button's recording waveform without reshuffling the whole
          footer.  ESC / ⌘↵ keep working regardless of visible hints. */}
      <div className={cn("sp-actions", isDictating && "sp-actions--dictating")}>
        <button
          className="sp-btn sp-btn--primary"
          onClick={handlePaste}
          title="Paste structured output into active app"
        >
          <ClipboardPaste size={11} strokeWidth={2.2} />
          <span className="sp-btn-label">Paste</span>
          <kbd className="sp-kbd sp-kbd--primary">
            <span className="sp-kbd-mod">⌘</span>↵
          </kbd>
        </button>
        <button
          className="sp-btn sp-btn--raw"
          onClick={handlePasteRaw}
          title="Paste raw transcript (your words, unstructured)"
        >
          <FileText size={11} strokeWidth={2.2} />
          <span className="sp-btn-label">Raw</span>
        </button>
        <button
          className={cn("sp-btn", justCopied && "sp-btn--confirm")}
          onClick={handleCopy}
          title="Copy Markdown to clipboard"
        >
          <Copy size={11} strokeWidth={2.2} />
          <span className="sp-btn-label">{justCopied ? "Copied" : "Copy"}</span>
        </button>
        <button
          className={cn("sp-btn", isEditing && "sp-btn--active")}
          onClick={() => setIsEditing((e) => !e)}
          title={isEditing ? "Finish editing" : "Edit before paste"}
        >
          <Pencil size={11} strokeWidth={2.2} />
          <span className="sp-btn-label">{isEditing ? "Done" : "Edit"}</span>
        </button>
        <div className="sp-spacer" />
        <button
          className="sp-btn sp-btn--ghost"
          onClick={onClose}
          title="Dismiss panel (Esc)"
        >
          <span className="sp-btn-label">Dismiss</span>
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
            <>
              <span className="sp-mic-wave">
                <MiniWaveform color="rgba(248,200,130,0.95)" />
              </span>
              {/* Full-bar label — only visible when the mic has
                  expanded to fill the action row (see
                  `.sp-actions--dictating .sp-mic` rules below).  In the
                  compact variant the label stays at max-width:0 so it
                  collapses cleanly. */}
              <span className="sp-mic-label">Listening · click to stop</span>
            </>
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
