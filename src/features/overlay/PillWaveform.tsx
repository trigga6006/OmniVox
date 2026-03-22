import { useRecordingStore } from "@/stores/recordingStore";

const BAR_COUNT = 12;
const BAR_WIDTH = 2;
const BAR_GAP = 2;
const MIN_HEIGHT = 2;
const MAX_HEIGHT = 18;

// Bell-curve weights — center bars respond more, edges stay lower
const WEIGHTS = [
  0.3, 0.4, 0.55, 0.7, 0.85, 1.0,
  1.0, 0.85, 0.7, 0.55, 0.4, 0.3,
];

// Per-bar phase offsets for organic motion
const PHASE_OFFSETS = [
  0, 0.15, 0.05, 0.22, 0.1, 0.18,
  0.12, 0.25, 0.06, 0.2, 0.03, 0.09,
];

interface PillWaveformProps {
  active: boolean;
  color?: string;
}

export function PillWaveform({ active, color }: PillWaveformProps) {
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
        const level = active
          ? Math.min(1, audioLevel * weight + phase * audioLevel * 0.5)
          : 0;
        const height = MIN_HEIGHT + level * (MAX_HEIGHT - MIN_HEIGHT);

        return (
          <div
            key={i}
            className="rounded-full"
            style={{
              width: `${BAR_WIDTH}px`,
              height: `${height}px`,
              backgroundColor: active
                ? (color ?? "rgb(251,191,36)")
                : "rgba(255,255,255,0.12)",
              opacity: active ? 0.8 : 1,
              transition: active
                ? "height 100ms ease-out, background-color 300ms ease"
                : "height 400ms ease-out, background-color 300ms ease",
            }}
          />
        );
      })}
    </div>
  );
}
