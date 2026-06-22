import { Loader2, Zap, Check, X } from "lucide-react";
import { confirmCommand, cancelCommand } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useCommandStore } from "@/stores/commandStore";
import { PillWaveform } from "./PillWaveform";

/** Command-Mode accent — indigo, deliberately distinct from dictation's red /
 * processing amber / structuring violet so the user always knows "this
 * executes, it won't type." Defined once as an RGB triple so the solid color
 * and the translucent glow can't drift apart. */
const ACCENT_RGB = "129,140,248";
const ACCENT = `rgb(${ACCENT_RGB})`;

/**
 * The Command-Mode pill.  Mutually exclusive with the dictation pill — only
 * rendered while a command capture/result is active.  Display-only except in
 * the `confirm` state, which offers Yes/No for a low-confidence app match.
 */
export function CommandPill({ showContent }: { showContent: boolean }) {
  const state = useCommandStore((s) => s.state);
  const summary = useCommandStore((s) => s.summary);

  const isListening = state === "listening";
  const isRecognizing = state === "recognizing";
  const isConfirm = state === "confirm";
  const isDone = state === "done";
  const isError = state === "error";

  const borderClass = isError
    ? "border-recording-500/40"
    : isDone
      ? "border-success/40"
      : "border-indigo-400/40";

  return (
    <div
      className={cn(
        isConfirm ? "w-[300px] h-[44px]" : "w-[260px] h-[34px]",
        "relative flex items-center overflow-hidden shrink-0 rounded-full border gap-2.5 px-3.5",
        "bg-[var(--color-pill-bg)] transition-[border-color] duration-200 ease-out",
        borderClass
      )}
      style={{
        opacity: showContent ? 1 : 0,
        transition: showContent
          ? "opacity 220ms cubic-bezier(0.4, 0, 0.2, 1) 40ms"
          : "none",
        boxShadow: `0 8px 24px -10px rgba(0,0,0,0.65), 0 0 18px -8px rgba(${ACCENT_RGB},0.45)`,
      }}
    >
      {/* Left glyph — the ⚡ is the command identity. */}
      <div className="shrink-0 flex items-center justify-center min-w-[20px]">
        {isRecognizing ? (
          <Loader2 size={13} className="animate-spin" style={{ color: ACCENT }} strokeWidth={2.5} />
        ) : isDone ? (
          <svg
            width="13"
            height="13"
            viewBox="0 0 16 16"
            className="text-success/85"
            fill="none"
            stroke="currentColor"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <polyline points="3 8.5 6.5 12 13 4" />
          </svg>
        ) : isError ? (
          <span className="text-recording-400/85 text-[12px] font-bold">!</span>
        ) : (
          <Zap size={13} style={{ color: ACCENT }} strokeWidth={2} fill={ACCENT} />
        )}
      </div>

      {/* Center — waveform while listening, otherwise status/summary text. */}
      <div className="flex-1 flex items-center overflow-hidden">
        {isListening && (
          <div className="flex items-center gap-2 w-full overflow-hidden">
            <span
              className="shrink-0 text-[9px] font-semibold uppercase tracking-[0.16em]"
              style={{ color: ACCENT, opacity: 0.85, fontFamily: "var(--font-display)" }}
            >
              Command
            </span>
            <PillWaveform active color={ACCENT} />
          </div>
        )}
        {isRecognizing && (
          <span className="text-[10px] font-medium tracking-wide truncate" style={{ color: ACCENT, opacity: 0.85 }}>
            Recognizing…
          </span>
        )}
        {(isConfirm || isDone || isError) && summary && (
          <span
            className={cn(
              "text-[11px] truncate",
              isError ? "text-recording-300/80" : isDone ? "text-text-secondary/80" : "text-text-primary/90"
            )}
          >
            {summary}
          </span>
        )}
      </div>

      {/* Confirm controls (low-confidence app match). */}
      {isConfirm && (
        <div className="shrink-0 flex items-center gap-2">
          <button
            onMouseDown={(e) => {
              e.stopPropagation();
              e.preventDefault();
              confirmCommand().catch(() => {});
            }}
            title="Yes"
            className="flex items-center justify-center h-7 w-7 rounded-full border border-indigo-400/50 bg-indigo-400/10 text-indigo-200 hover:bg-indigo-400/25 transition-colors"
          >
            <Check size={14} strokeWidth={2.5} />
          </button>
          <button
            onMouseDown={(e) => {
              e.stopPropagation();
              e.preventDefault();
              cancelCommand().catch(() => {});
            }}
            title="Dismiss"
            className="flex items-center justify-center h-7 w-7 rounded-full border border-white/15 text-text-secondary/80 hover:bg-white/10 transition-colors"
          >
            <X size={14} strokeWidth={2.5} />
          </button>
        </div>
      )}

      {/* Listening indicator dot on the right. */}
      {isListening && (
        <div className="shrink-0 w-[16px] flex items-center justify-end">
          <span
            className="relative h-1.5 w-1.5 rounded-full"
            style={{ backgroundColor: ACCENT, boxShadow: `0 0 6px ${ACCENT}` }}
          />
        </div>
      )}
    </div>
  );
}
