import { useEffect, useRef } from "react";

// Command accent palette — muted → bright amber (aligned with the app accent,
// kept low-key so the field reads as background texture, not a light show).
const AMBER = ["#b45309", "#d97f08", "#f59e0b"];

// Authentic Memselon AsciiMatrix glyph ramp (mirrors ClickPulse) — brightness →
// character density. Rendering the field off this ramp is what makes it read as
// a real ANSI surface rather than a dot grid.
const RAMP =
  " .'`,^:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";
const A_CH = 9; // ascii cell height (px)
const A_CW = 5; // ascii cell width — ~0.55 of height → the real rectangular cell
const A_FONT = 8; // glyph size (~0.9 × cell height)
const TAU = Math.PI * 2;

// Density-by-intensity: t∈0..1 → ramp glyph (0 = sparse/empty, 1 = densest).
const rampGlyph = (t: number) =>
  RAMP[Math.max(0, Math.min(RAMP.length - 1, (t * RAMP.length) | 0))];

// Stable per-cell hash (0..1) — gives every dot a fixed phase + speed so they
// shimmer independently (deterministic, so the pattern itself never moves).
const hash = (a: number, b: number) => {
  const s = Math.sin(a * 12.9898 + b * 78.233) * 43758.5453;
  return s - Math.floor(s);
};

/**
 * Ambient ANSI field for the Command pill — a fixed rectangular grid of amber
 * ASCII glyphs (same Memselon AsciiMatrix technique as the main-app touch
 * effect, in its quiet always-on "glitch" form). Each cell has a stable density
 * baseline and gently shimmers on its own phase + speed, with a sparse,
 * throttled scramble flickering glyphs through the ramp — so the whole field
 * reads as a quietly-alive terminal surface rather than a moving wave.
 * Deliberately faint background texture: decorative (pointer-events-none),
 * clipped by the pill's rounded overflow, behind the content. Honors
 * prefers-reduced-motion (renders one static frame).
 */
export function CommandDither({ active }: { active: boolean }) {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    if (!active) return;
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const reduce =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    let raf = 0;
    let t = 0;
    let last = 0;

    const size = () => {
      const r = canvas.getBoundingClientRect();
      canvas.width = Math.max(1, Math.round(r.width * dpr));
      canvas.height = Math.max(1, Math.round(r.height * dpr));
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    size();

    const draw = (now: number) => {
      const W = canvas.width / dpr;
      const H = canvas.height / dpr;
      ctx.clearRect(0, 0, W, H);
      // Advance by real elapsed time so the shimmer runs at the same pace even
      // if the always-on-top overlay renders below 60fps.
      const dt = last ? Math.min(0.05, (now - last) / 1000) : 0.016;
      last = now;
      t += dt;

      ctx.font = `${A_FONT}px "Geist Mono", "JetBrains Mono", ui-monospace, monospace`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      const cols = Math.ceil(W / A_CW);
      const rows = Math.ceil(H / A_CH);
      // Throttled glitch quantum (~7×/sec) — matches AsciiMatrix's deliberately
      // low glitch refresh and keeps the scramble allocation-free + deterministic.
      const tick = Math.floor(t * 7);
      for (let ix = 0; ix < cols; ix++) {
        for (let iy = 0; iy < rows; iy++) {
          // Stable per-cell density baseline → a fixed field of varied glyphs,
          // not a uniform wall. Each cell shimmers on its own phase + speed.
          const base = 0.2 + hash(ix * 1.3, iy * 0.7) * 0.6;
          const phase = hash(ix, iy) * TAU;
          const rate = 1.0 + hash(iy, ix) * 1.6; // 1.0–2.6 rad/s
          const osc = 0.5 + 0.5 * Math.sin(t * rate + phase);
          let di = base * (0.35 + 0.65 * osc * osc); // bias dim → brights twinkle
          // Sparse, throttled scramble: the ANSI glitch churn — a few cells flare
          // to a brighter, denser glyph each quantum.
          if (hash(ix + tick * 0.137, iy - tick * 0.091) < 0.07) {
            di = Math.max(di, 0.45 + hash(iy + tick, ix) * 0.4);
          }
          const glyph = rampGlyph(di);
          if (glyph === " ") continue;
          ctx.fillStyle = AMBER[di > 0.6 ? 2 : di > 0.28 ? 1 : 0];
          ctx.globalAlpha = 0.06 + di * 0.4; // faint base, gentle peak
          ctx.fillText(glyph, ix * A_CW + A_CW / 2, iy * A_CH + A_CH / 2);
        }
      }
      ctx.globalAlpha = 1;
    };

    if (reduce) {
      draw(0);
      return;
    }

    const loop = (now: number) => {
      draw(now);
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    const onResize = () => size();
    window.addEventListener("resize", onResize);
    return () => {
      cancelAnimationFrame(raf);
      window.removeEventListener("resize", onResize);
    };
  }, [active]);

  if (!active) return null;
  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      className="pointer-events-none absolute inset-0 h-full w-full"
      style={{ opacity: 0.38 }}
    />
  );
}
