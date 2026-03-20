import { useMemo } from "react";
import { useRecordingStore } from "@/stores/recordingStore";

/** Number of bars in the visualizer */
const BAR_COUNT = 5;

/** Each bar's base amplitude multiplier — center bar peaks highest */
const BAR_WEIGHTS = [0.6, 0.85, 1, 0.85, 0.6];

/** Staggered animation delays (ms) for the bounce animation */
const BAR_DELAYS = [0, 120, 60, 180, 100];

/** Min/max height for bars in px */
const MIN_HEIGHT = 8;
const MAX_HEIGHT = 36;

export function AudioVisualizer() {
  const audioLevel = useRecordingStore((s) => s.audioLevel);

  // Derive per-bar heights from the audio level (0-1 range)
  const barHeights = useMemo(() => {
    // Clamp audio level to 0-1
    const level = Math.max(0, Math.min(1, audioLevel));

    return BAR_WEIGHTS.map((weight) => {
      const scaled = level * weight;
      return MIN_HEIGHT + scaled * (MAX_HEIGHT - MIN_HEIGHT);
    });
  }, [audioLevel]);

  return (
    <div
      className="flex items-end justify-center gap-[6px]"
      style={{ width: 60, height: MAX_HEIGHT + 4 }}
      role="img"
      aria-label="Audio level visualization"
    >
      {Array.from({ length: BAR_COUNT }, (_, i) => (
        <span
          key={i}
          className="w-[3px] rounded-full bg-gradient-to-t from-amber-500 to-amber-400 origin-bottom"
          style={{
            height: `${barHeights[i]}px`,
            animation: `bar-bounce 0.8s ease-in-out ${BAR_DELAYS[i]}ms infinite`,
            transition: "height 120ms ease-out",
          }}
        />
      ))}
    </div>
  );
}
