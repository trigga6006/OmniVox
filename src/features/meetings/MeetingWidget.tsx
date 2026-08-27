import { useEffect, useState } from "react";
import { Pause, Play, Square, X } from "lucide-react";
import {
  dismissMeetingWidget,
  meetingPause,
  meetingResume,
  meetingStop,
  openMeetingDrawer,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { DitherOrb } from "./DitherOrb";
import { MeetingChannelMeter } from "./MeetingWaveform";
import { formatMeetingDuration, useMeetingRuntime } from "./useMeetingRuntime";

/**
 * The docked meeting widget — a 66×132 card inside a fixed 78×142 window
 * (the extra margin is the shadow's room). The dithered orb is both the
 * recording indicator and the button that opens the notes drawer; transport
 * controls slide up on hover over the timer row.
 */
export function MeetingWidget() {
  const { runtime } = useMeetingRuntime();
  const [hovered, setHovered] = useState(false);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.documentElement.dataset.theme = "dark";
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    document.body.style.overflow = "hidden";
  }, []);

  const active = runtime.status === "recording";
  const paused = runtime.status === "paused";
  const processing = runtime.status === "transcribing";

  return (
    <div className="flex h-screen w-screen items-center justify-center">
      <div
        data-tauri-drag-region
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className={cn(
          "relative flex h-[132px] w-[66px] cursor-move flex-col items-center overflow-hidden",
          "rounded-[22px] border bg-surface-1/95 shadow-[var(--shadow-lg)] backdrop-blur-xl",
          "transition-colors duration-200",
          active ? "border-border-active" : "border-border"
        )}
      >
        <button
          onMouseDown={(event) => event.stopPropagation()}
          onClick={() => dismissMeetingWidget().catch(() => {})}
          className={cn(
            "absolute right-[5px] top-[5px] z-10 flex h-[18px] w-[18px] items-center justify-center",
            "rounded-full text-text-muted transition-[opacity,color,background-color] duration-150",
            "hover:bg-surface-3 hover:text-text-primary",
            hovered ? "opacity-100" : "opacity-40"
          )}
          aria-label="Dismiss meeting widget"
        >
          <X size={11} />
        </button>

        <div className="flex flex-1 flex-col items-center justify-center gap-[7px] pt-1">
          <button
            onMouseDown={(event) => event.stopPropagation()}
            onClick={() =>
              (runtime.meeting_id
                ? openMeetingDrawer(runtime.meeting_id)
                : dismissMeetingWidget()
              ).catch(() => {})
            }
            className={cn(
              "flex h-11 w-11 cursor-pointer items-center justify-center rounded-full",
              "transition-colors duration-200 hover:bg-surface-3/60"
            )}
            aria-label={
              runtime.meeting_id
                ? "Open meeting notes"
                : "Dismiss inactive meeting widget"
            }
          >
            <DitherOrb
              status={runtime.status}
              level={Math.max(runtime.mic_level, runtime.system_level)}
            />
          </button>

          <MeetingChannelMeter
            micLevel={runtime.mic_level}
            systemLevel={runtime.system_level}
            micDetected={runtime.mic_signal_detected}
            systemDetected={runtime.system_signal_detected}
            active={active}
          />

          <span
            title={runtime.capture_warning ?? undefined}
            className={cn(
              "font-mono text-xs font-medium tabular-nums",
              paused ? "text-text-muted" : "text-text-secondary"
            )}
          >
            {processing ? "sync" : formatMeetingDuration(runtime.elapsed_ms)}
          </span>
        </div>

        {/* Resting affordance for the hidden transport bar. */}
        <span
          aria-hidden="true"
          className={cn(
            "absolute bottom-[6px] left-1/2 h-[2px] w-5 -translate-x-1/2 rounded-full",
            "bg-text-muted/50 transition-opacity duration-200",
            hovered ? "opacity-0" : "opacity-100"
          )}
        />

        <div
          onMouseDown={(event) => event.stopPropagation()}
          className={cn(
            "absolute inset-x-0 bottom-0 flex h-10 items-center justify-center gap-1.5",
            "border-t border-border bg-surface-2/95 backdrop-blur-xl",
            "transition-transform duration-200 ease-out-quint",
            hovered ? "translate-y-0" : "translate-y-full"
          )}
        >
          {!processing && (
            <button
              onClick={() => (paused ? meetingResume() : meetingPause()).catch(() => {})}
              className={cn(
                "flex h-7 w-7 cursor-pointer items-center justify-center rounded-full",
                "text-text-secondary transition-colors duration-150",
                "hover:bg-surface-4 hover:text-text-primary"
              )}
              aria-label={paused ? "Resume meeting" : "Pause meeting"}
            >
              {paused ? <Play size={12} fill="currentColor" /> : <Pause size={12} fill="currentColor" />}
            </button>
          )}
          {!processing && (
            <button
              onClick={() => meetingStop().catch(() => {})}
              className={cn(
                "flex h-7 w-7 cursor-pointer items-center justify-center rounded-full",
                "bg-recording-500/15 text-recording-400 transition-colors duration-150",
                "hover:bg-recording-500 hover:text-white"
              )}
              aria-label="End meeting"
            >
              <Square size={10} fill="currentColor" />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
