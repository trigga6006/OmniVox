import { useEffect, useRef } from "react";
import { buildRgbRamp, type RgbRampEntry } from "@/lib/colorRamp";
import type { MeetingRuntimeState } from "@/lib/tauri";
import { cn } from "@/lib/utils";

type OrbStatus = MeetingRuntimeState["status"];

/**
 * Amber ramp, dimmest → brightest. Read from tokens.css instead of retyped:
 * all three triples were already exact copies of amber-700 / -500 / -300, so
 * this is a 1:1 swap that now follows a retuned accent. ImageData needs raw
 * channels, hence the RGB variant.
 */
const RAMP_TOKENS: readonly RgbRampEntry[] = [
  ["--color-amber-700", [180, 83, 9]],
  ["--color-amber-500", [245, 158, 11]],
  ["--color-amber-300", [252, 195, 77]],
];

/** Ordered 4×4 Bayer matrix, flattened and normalized to 0..1 thresholds. */
const BAYER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map(
  (v) => (v + 0.5) / 16
);

/** Frame budget — the orb runs for the whole meeting, so it is capped at ~30fps. */
const FRAME_MS = 33;
/** Breath: one slow inhale/exhale every ~4.2s. */
const BREATH_RATE = 1.5;
/** mic/system levels peak well under 1.0 — lift them into a usable 0..1 range. */
const LEVEL_GAIN = 7;

interface DitherOrbProps {
  status: OrbStatus;
  /** Combined capture level, 0..~0.15 in practice. */
  level: number;
  /** Rendered CSS size, px. */
  size?: number;
  /** Buffer resolution — the orb is drawn at cells×cells then scaled up. */
  cells?: number;
  className?: string;
}

/**
 * Bayer-dithered recording orb — a low-res radial glow thresholded through a
 * 4×4 ordered-dither matrix into three amber steps, then scaled up with
 * smoothing off so the pixels stay chunky. The core breathes on a slow sine and
 * swells with live capture level; "paused" freezes and dims it, "transcribing"
 * adds a slow vertical shimmer.
 *
 * Cheap by construction: one reused ImageData, `cells²` (~576) pixels per frame
 * at a 30fps cap, paused while the document is hidden, and a single static
 * frame under prefers-reduced-motion.
 */
export function DitherOrb({
  status,
  level,
  size = 40,
  cells = 20,
  className,
}: DitherOrbProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const statusRef = useRef(status);
  const levelRef = useRef(level);

  useEffect(() => {
    statusRef.current = status;
  }, [status]);
  useEffect(() => {
    levelRef.current = level;
  }, [level]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    // Low-res buffer we dither into, then blit up at nearest-neighbour.
    const buffer = document.createElement("canvas");
    buffer.width = cells;
    buffer.height = cells;
    const bctx = buffer.getContext("2d");
    if (!bctx) return;
    const image = bctx.createImageData(cells, cells);
    const pixels = image.data;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = Math.round(size * dpr);
    canvas.height = Math.round(size * dpr);
    ctx.imageSmoothingEnabled = false;

    // Built once per mount; `buildRgbRamp` caches per theme.
    const ramp = buildRgbRamp(RAMP_TOKENS);

    const radius = cells / 2;
    let smoothed = 0;
    let clock = 0;
    let last = 0;
    let lastFrame = 0;
    let raf = 0;

    const draw = () => {
      const state = statusRef.current;
      const paused = state === "paused";
      const idle = state === "idle";
      const energy = Math.min(1, Math.max(0, levelRef.current) * LEVEL_GAIN);
      smoothed += (energy - smoothed) * 0.14;

      const breath = 0.5 + 0.5 * Math.sin(clock * BREATH_RATE);
      // Core radius in cell units: idle sits small and still, recording breathes
      // and swells with the room.
      const core = idle
        ? radius * 0.34
        : radius * (0.44 + 0.1 * breath + 0.24 * smoothed);
      // Brightness ceiling — paused reads as embers, idle as a dim standby dot.
      const gain = paused ? 0.58 : idle ? 0.42 : 0.86 + 0.14 * breath;
      const shimmer = state === "transcribing" ? 0.16 : 0;

      for (let y = 0; y < cells; y++) {
        const dy = (y + 0.5 - radius) / core;
        const wave = shimmer * Math.sin((y / cells) * 7 - clock * 2.4);
        for (let x = 0; x < cells; x++) {
          const dx = (x + 0.5 - radius) / core;
          const distance = Math.sqrt(dx * dx + dy * dy);
          const falloff = 1 - distance;
          const p = (y * cells + x) * 4;
          if (falloff <= 0) {
            pixels[p + 3] = 0;
            continue;
          }
          // Gamma tightens the core so the dithered bands read as rings.
          const value = Math.min(
            1,
            Math.pow(falloff, 1.35) * gain + wave * falloff
          );
          // Ordered dither: quantize into 3 bands, nudging up when the
          // remainder beats this pixel's Bayer threshold.
          const scaled = value * ramp.length;
          const band = Math.floor(scaled);
          const step =
            band + (scaled - band > BAYER[(y & 3) * 4 + (x & 3)] ? 1 : 0);
          const index = Math.min(ramp.length, step) - 1;
          if (index < 0) {
            pixels[p + 3] = 0;
            continue;
          }
          const [r, g, b] = ramp[index];
          pixels[p] = r;
          pixels[p + 1] = g;
          pixels[p + 2] = b;
          pixels[p + 3] = 255;
        }
      }

      bctx.putImageData(image, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.drawImage(buffer, 0, 0, canvas.width, canvas.height);
    };

    const reduce =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    if (reduce) {
      draw();
      return;
    }

    const loop = (now: number) => {
      raf = requestAnimationFrame(loop);
      if (now - lastFrame < FRAME_MS) return;
      lastFrame = now;
      const delta = last ? Math.min(0.1, (now - last) / 1000) : 0.033;
      last = now;
      // Freezing the clock while paused freezes the whole orb.
      if (statusRef.current !== "paused") clock += delta;
      draw();
    };

    const start = () => {
      if (raf) return;
      last = 0;
      raf = requestAnimationFrame(loop);
    };
    const stop = () => {
      cancelAnimationFrame(raf);
      raf = 0;
    };
    const onVisibility = () => (document.hidden ? stop() : start());

    start();
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      stop();
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [size, cells]);

  return (
    <canvas
      ref={canvasRef}
      aria-hidden="true"
      className={cn("pointer-events-none block", className)}
      style={{ width: size, height: size, imageRendering: "pixelated" }}
    />
  );
}
