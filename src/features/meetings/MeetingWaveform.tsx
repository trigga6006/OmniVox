import { useMemo } from "react";
import { cn } from "@/lib/utils";

export function MeetingWaveform({
  level,
  active,
  compact = false,
  className,
}: {
  level: number;
  active: boolean;
  compact?: boolean;
  className?: string;
}) {
  const bars = useMemo(() => [0.52, 0.9, 0.68, 1, 0.58], []);
  return (
    <div className={cn("flex items-center justify-center gap-[2px]", className)} aria-hidden="true">
      {bars.map((weight, index) => {
        const height = active ? Math.max(3, Math.min(compact ? 13 : 18, 3 + level * 120 * weight)) : 3;
        return (
          <span
            key={index}
            className="w-[2px] rounded-full bg-current transition-[height,opacity] duration-100"
            style={{ height, opacity: active ? 0.58 + weight * 0.4 : 0.28 }}
          />
        );
      })}
    </div>
  );
}

/**
 * Two-channel capture meter for the meeting widget — mic on top, system below.
 * A meeting recorder's first question is "is it hearing both sides?", so each
 * channel gets its own bar and a channel that is live but silent turns red.
 */
export function MeetingChannelMeter({
  micLevel,
  systemLevel,
  micDetected,
  systemDetected,
  active,
}: {
  micLevel: number;
  systemLevel: number;
  micDetected: boolean;
  systemDetected: boolean;
  active: boolean;
}) {
  return (
    <div className="flex flex-col gap-[3px]" aria-hidden="true">
      {[
        { level: micLevel, detected: micDetected },
        { level: systemLevel, detected: systemDetected },
      ].map((channel, index) => (
        <span
          key={index}
          className={cn(
            "block h-[3px] w-[26px] overflow-hidden rounded-full bg-surface-3",
            !active && "opacity-50"
          )}
        >
          <span
            className={cn(
              "block h-full rounded-full transition-[width,background-color] duration-150",
              active && !channel.detected ? "bg-recording-500/70" : "bg-amber-500"
            )}
            style={{
              width: active
                ? `${Math.max(channel.detected ? 12 : 8, Math.min(100, channel.level * 700))}%`
                : "0%",
            }}
          />
        </span>
      ))}
    </div>
  );
}
