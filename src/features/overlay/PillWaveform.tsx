import { useRecordingStore } from "@/stores/recordingStore";

const BAR_COUNT = 12;
const BAR_WIDTH = 2;
const BAR_GAP = 2;
const MIN_HEIGHT = 2;
const MAX_HEIGHT = 18;
// Below this audio level the mic is effectively silent — show a gently
// shimmering resting baseline instead of 12 dead-flat 2px dots.
const IDLE_LEVEL = 0.04;
const IDLE_HEIGHT = 6;

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
  color?: string;
}

// The waveform is only ever rendered while capture is live (both call sites),
// so there's no inactive branch — bars react to the audio level, and fall back
// to a shimmering baseline during near-silence.
export function PillWaveform({ color }: PillWaveformProps) {
  const audioLevel = useRecordingStore((s) => s.audioLevel);
  const idle = audioLevel < IDLE_LEVEL;

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
        const level = Math.min(1, audioLevel * weight + phase * audioLevel * 0.5);
        const height = MIN_HEIGHT + level * (MAX_HEIGHT - MIN_HEIGHT);

        return (
          <div
            key={i}
            className="rounded-full"
            style={{
              width: `${BAR_WIDTH}px`,
              height: idle ? `${IDLE_HEIGHT}px` : `${height}px`,
              backgroundColor: color ?? "rgb(245,158,11)",
              opacity: 0.8,
              transformOrigin: "center",
              // Near-silence: a per-bar staggered shimmer (reuses the idle-wave
              // keyframe) so the waveform breathes instead of flat-lining. Once
              // audio rises above IDLE_LEVEL, snap to the audio-driven height.
              animation: idle
                ? `idle-wave 1.5s ease-in-out ${i * 0.09}s infinite`
                : "none",
              transition: "height 100ms ease-out, background-color 300ms ease",
            }}
          />
        );
      })}
    </div>
  );
}
