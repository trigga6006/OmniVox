import { Loader2, Sparkles } from "lucide-react";
import { formatDuration } from "@/lib/utils";
import { PillWaveform } from "./PillWaveform";

interface PillContentProps {
  showContent: boolean;
  isRecording: boolean;
  isProcessing: boolean;
  isStructuring: boolean;
  /** True while the LLM is still loading (llm-status event) — the
   *  structuring stall is a model lazy-load, so label it honestly. */
  llmLoading: boolean;
  isSuccess: boolean;
  isError: boolean;
  duration: number;
  previewText: string | null;
  flashText: string | null;
  modeColor: string;
}

// The active-state inner content of the pill button: waveform / live preview
// / duration on the left, state text in the center, record dot on the right.
// FloatingPill mounts this only when the pill is not idle and the mode
// selector is closed.
export function PillContent({
  showContent,
  isRecording,
  isProcessing,
  isStructuring,
  llmLoading,
  isSuccess,
  isError,
  duration,
  previewText,
  flashText,
  modeColor,
}: PillContentProps) {
  return (
    <div
      className="flex items-center w-full h-full gap-2"
      style={{
        opacity: showContent ? 1 : 0,
        // Asymmetric transition — key polish fix.
        // Before: `opacity 0.2s ease` applied in both directions,
        // which meant the 80 ms hide window (set before resize)
        // cut off the fade-out at ~60 % opacity, then React flipped
        // showContent back to true and the fade reversed.  User
        // perception: "content dims and brightens for no reason"
        // = the one-frame flicker.
        // Now: hide is instant (transition: "none" when going
        // false), so no partial fade is ever visible.  Show uses a
        // 40 ms delay to give WebView2 a margin beyond the 80 ms
        // resize window before the pixels arrive, then fades in
        // cleanly over 220 ms.
        transition: showContent
          ? "opacity 220ms cubic-bezier(0.4, 0, 0.2, 1) 40ms"
          : "none",
      }}
    >
      {isProcessing && (
        <div
          className="absolute inset-0 overflow-hidden pointer-events-none"
          aria-hidden="true"
        >
          <div
            className="absolute inset-0 -translate-x-full"
            style={{
              background:
                "linear-gradient(90deg, transparent 0%, rgba(245,158,11,0.06) 50%, transparent 100%)",
              animation: "shimmer 2s ease-in-out infinite",
            }}
          />
        </div>
      )}

      {/* Left: timer / spinner / icon */}
      <div className="shrink-0 flex items-center justify-center min-w-[28px]">
        {isRecording && (
          <span className="font-mono text-[11px] tabular-nums text-recording-300/80 tracking-wide">
            {formatDuration(duration)}
          </span>
        )}
        {isStructuring && (
          <span className="relative flex items-center justify-center">
            <Sparkles
              size={12}
              className="relative text-amber-300"
              strokeWidth={2.5}
              style={{
                animation: "structuring-spark 2.2s ease-in-out infinite",
              }}
            />
          </span>
        )}
        {isSuccess && (
          <svg
            width="12" height="12" viewBox="0 0 16 16"
            className="text-success/80" fill="none" stroke="currentColor"
            strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"
          >
            <polyline points="3 8.5 6.5 12 13 4" />
          </svg>
        )}
        {isError && (
          <span className="text-recording-400/80 text-[11px] font-semibold">!</span>
        )}
      </div>

      {/* Center: waveform / preview text / status text */}
      <div className="flex-1 flex items-center justify-center overflow-hidden">
        {isRecording && previewText && (
          // Right-anchored teleprompter: newest words pinned to the right,
          // older ones clipped off the left as speech streams in.
          <div className="flex w-full justify-end overflow-hidden">
            <span
              className="whitespace-nowrap text-[10px] font-normal tracking-tight"
              style={{ color: modeColor, opacity: 0.7 }}
            >
              {previewText}
            </span>
          </div>
        )}
        {isRecording && !previewText && <PillWaveform active color={modeColor} />}
        {isProcessing && (
          <Loader2 size={13} className="text-amber-400/70 animate-spin" strokeWidth={2.5} />
        )}
        {isStructuring && (
          <span
            className="text-[10px] font-medium tracking-[0.14em] uppercase truncate"
            style={{
              fontFamily: "var(--font-display)",
              background:
                "linear-gradient(90deg, rgba(245,158,11,0.45) 0%, rgba(252,195,77,0.95) 50%, rgba(245,158,11,0.45) 100%)",
              backgroundSize: "220% 100%",
              WebkitBackgroundClip: "text",
              WebkitTextFillColor: "transparent",
              animation: "structuring-shimmer 2.4s linear infinite",
            }}
          >
            {llmLoading ? "Loading model" : "Structuring"}
            <span
              aria-hidden="true"
              style={{
                display: "inline-block",
                width: "1.2em",
                textAlign: "left",
                marginLeft: "1px",
              }}
            >
              <span
                style={{
                  animation: "structuring-dot 1.4s ease-in-out infinite",
                  animationDelay: "0s",
                }}
              >
                ·
              </span>
              <span
                style={{
                  animation: "structuring-dot 1.4s ease-in-out infinite",
                  animationDelay: "0.2s",
                }}
              >
                ·
              </span>
              <span
                style={{
                  animation: "structuring-dot 1.4s ease-in-out infinite",
                  animationDelay: "0.4s",
                }}
              >
                ·
              </span>
            </span>
          </span>
        )}
        {isSuccess && flashText && (
          <span className="text-[10px] text-text-secondary/70 truncate">
            {flashText}
          </span>
        )}
        {isError && (
          <span className="text-[10px] text-recording-300/70 truncate">
            Error
          </span>
        )}
      </div>

      {/* Right: record dot */}
      <div className="shrink-0 w-[16px] flex items-center justify-end">
        {isRecording && (
          <div className="relative flex items-center justify-center">
            <span
              className="absolute h-3.5 w-3.5 rounded-full bg-recording-500/15"
              style={{ animation: "recording-pulse 2s ease-in-out infinite" }}
            />
            <span className="relative h-1.5 w-1.5 rounded-full bg-recording-500" />
          </div>
        )}
        {isStructuring && (
          <div className="relative flex items-center justify-center">
            <span
              className="relative h-1.5 w-1.5 rounded-full"
              style={{
                backgroundColor: "rgb(245,158,11)",
                animation: "structuring-pulse 2s ease-in-out infinite",
              }}
            />
          </div>
        )}
        {isSuccess && (
          <div className="h-1.5 w-1.5 rounded-full bg-success/40" />
        )}
      </div>
    </div>
  );
}
