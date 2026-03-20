import { useRecordingStore } from "@/stores/recordingStore";

/**
 * 16-bar audio waveform visualizer — the heartbeat of the floating pill.
 *
 * Bell-curve weighted: center bars peak higher, edges taper off.
 * Idle: flat 3px bars in muted color. Recording: amber bars that dance
 * with your voice via audio level from the recording store.
 */

const BAR_COUNT = 16;
const BAR_WIDTH = 3;
const BAR_GAP = 3;
const MIN_HEIGHT = 3;
const MAX_HEIGHT = 28;

// Bell-curve weights — center bars respond more, edges stay lower
const WEIGHTS = [
  0.25, 0.35, 0.48, 0.6, 0.72, 0.84, 0.92, 1.0,
  1.0, 0.92, 0.84, 0.72, 0.6, 0.48, 0.35, 0.25,
];

// Per-bar phase offsets for organic, non-uniform motion
const PHASE_OFFSETS = [
  0, 0.15, 0.05, 0.22, 0.1, 0.28, 0.08, 0.18,
  0.12, 0.25, 0.06, 0.2, 0.14, 0.03, 0.24, 0.09,
];

interface PillWaveformProps {
  active: boolean;
}

export function PillWaveform({ active }: PillWaveformProps) {
  const audioLevel = useRecordingStore((s) => s.audioLevel);

  return (
    <div
      className="flex items-center justify-center"
      style={{
        gap: `${BAR_GAP}px`,
        height: `${MAX_HEIGHT}px`,
      }}
    >
      {Array.from({ length: BAR_COUNT }, (_, i) => {
        const weight = WEIGHTS[i];
        const phase = PHASE_OFFSETS[i];

        // Combine audio level with per-bar weight and phase for organic motion
        const level = active
          ? Math.min(1, audioLevel * weight + phase * audioLevel * 0.5)
          : 0;

        const height = MIN_HEIGHT + level * (MAX_HEIGHT - MIN_HEIGHT);

        return (
          <div
            key={i}
            style={{
              width: `${BAR_WIDTH}px`,
              height: `${height}px`,
              transition: active
                ? "height 100ms ease-out, background-color 300ms ease"
                : "height 400ms ease-out, background-color 300ms ease",
            }}
            className={
              active
                ? "rounded-full bg-amber-400"
                : "rounded-full bg-surface-4"
            }
          />
        );
      })}
    </div>
  );
}
