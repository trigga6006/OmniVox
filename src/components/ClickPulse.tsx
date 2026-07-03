import { useEffect, useRef } from "react";

/**
 * ClickPulse — the interactive "artist's touch" that makes the paper feel alive.
 * The base UI stays completely matte/normal; this is a full-window,
 * pointer-events-none canvas on top that does nothing until you click. On each
 * click a SMALL, contained patch of electric static flickers right at the
 * pointer — a bright contact flash, a crackle of multicolored dither, then a
 * residue that runs through the spot and cools away (~600ms). No expanding ring;
 * the patch is an irregular blob (noise-warped) and the texture is true
 * per-frame static (antsy), never an ordered sweep.
 *
 * Two looks share one envelope (ignite → crackle → residue):
 *   · "dither" — fine ordered-dither pixels + electric arc filaments
 *   · "ascii"  — authentic Memselon AsciiMatrix ANSI field: a rectangular glyph
 *               grid keyed to the 70-char density ramp, scrambling as it crackles
 *               (default). Mirrors the /launch ASCII plates' rendering technique.
 * Cycle live with Ctrl/Cmd+Alt+P (persists). Mirrors design-lab/click-pulse-lab.html.
 *
 * Mounted only in the main window. rAF runs only while a pulse is alive.
 */

type Mode = "dither" | "ascii" | "off";
type Pulse = { x: number; y: number; t0: number; r: number };

const DUR = 600; // ms — ignite + cool
const DOT = 2; // dither cell (px)
const R_MIN = 22; // contained radius (px)
const R_VAR = 12;
const MAX_PULSES = 40; // trail cap while click-dragging
const EMIT_GAP = 12; // px the cursor must travel before the drag spawns the next burst
const STORE_KEY = "omnivox.clickPulse.v2"; // bumped: ANSI is now the default look

const BAYER4 = [
  [0, 8, 2, 10],
  [12, 4, 14, 6],
  [3, 11, 1, 9],
  [15, 7, 13, 5],
];

// Graphite-system palette, split by "temperature".
const HOT = ["#f4f4f5", "#fde9c2"]; // white / pale-gold — hottest sparks
const WARM = ["#f59e0b", "#fbbf24"]; // amber / gold — body
const EMBER = ["#d97f08", "#b45309"]; // amber-600 / deep — cooling
const COLD = ["#60a5fa", "#93c5fd"]; // blue / light-blue — rare electric fleck

// Authentic Memselon AsciiMatrix glyph ramp — light→dark, sparse→dense. Mapping
// brightness to character DENSITY this way is the signature of the real ANSI
// field (same ramp the /launch ASCII plates use).
const RAMP =
  " .'`,^:;Il!i><~+_-?][}{1)(|/tfjrxnuvczXYUJCLQ0OZmwqpdbkhao*#MW&8%B@$";
const A_CH = 11; // ascii cell height (px)
const A_CW = 6; // ascii cell width — ~0.55 of height → the real rectangular cell
const A_FONT = 10; // glyph size (~0.9 × cell height, matching AsciiMatrix fontScale)

// Density-by-intensity: t∈0..1 → ramp glyph (0 = sparse/empty, 1 = densest).
const rampGlyph = (t: number) =>
  RAMP[Math.max(0, Math.min(RAMP.length - 1, (t * RAMP.length) | 0))];

// Controls + opt-out marker. The pulse never renders on top of these.
const INTERACTIVE =
  'button, a, input, textarea, select, label, summary, [role="button"], [role="switch"], [role="tab"], [role="menuitem"], [contenteditable="true"], [data-no-pulse]';

// The pulse is for the *paper* — it fires only on page dead space, never over a
// control or a raised surface (card / pill / panel). Walk from the click target
// up to the page root; bail if we cross anything interactive or with its own
// background fill. Auto-adapts to new components without per-component wiring.
function isDeadSpace(target: EventTarget | null): boolean {
  if (!(target instanceof Element)) return false;
  const root = target.closest("[data-pulse-root]");
  if (!root) return false;
  let el: Element | null = target;
  while (el && el !== root) {
    if (el.matches(INTERACTIVE)) return false;
    const bg = window.getComputedStyle(el).backgroundColor;
    if (bg && bg !== "rgba(0, 0, 0, 0)" && bg !== "transparent") return false;
    el = el.parentElement;
  }
  return true;
}

const clamp01 = (v: number) => (v < 0 ? 0 : v > 1 ? 1 : v);

// Stable per-cell hash + coarse bilinear value-noise → low-freq blobs (warps the
// patch into an irregular shape so it never reads as a clean circle/ring).
function hash2(x: number, y: number) {
  const s = Math.sin(x * 127.1 + y * 311.7) * 43758.5453;
  return s - Math.floor(s);
}
function vnoise(x: number, y: number) {
  const g = 16; // coarser -> larger lobes -> less circular outline
  const xi = Math.floor(x / g);
  const yi = Math.floor(y / g);
  const xf = x / g - xi;
  const yf = y / g - yi;
  const u = xf * xf * (3 - 2 * xf);
  const v = yf * yf * (3 - 2 * yf);
  const a = hash2(xi, yi);
  const b = hash2(xi + 1, yi);
  const c = hash2(xi, yi + 1);
  const d = hash2(xi + 1, yi + 1);
  return a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v;
}

type Rnd = () => number;

function renderDither(ctx: CanvasRenderingContext2D, p: Pulse, ageMs: number, rnd: Rnd) {
  const a = ageMs / DUR;
  if (a >= 1) return;
  const ignite = clamp01(1 - ageMs / 110);
  const crackle = Math.pow(clamp01(1 - ageMs / 330), 0.85);
  const residue = Math.pow(clamp01(1 - a), 1.5);
  const r = p.r;
  const x0 = Math.floor((p.x - r) / DOT) * DOT;
  const y0 = Math.floor((p.y - r) / DOT) * DOT;

  for (let x = x0; x <= p.x + r; x += DOT) {
    for (let y = y0; y <= p.y + r; y += DOT) {
      const dx = x - p.x;
      const dy = y - p.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const n = vnoise(x, y);
      // Stronger, lower-freq warp -> lobed/irregular outline, not a disc.
      const distW = dist * (0.58 + n * 0.86);
      if (distW > r) continue;
      const falloff = 1 - distW / r;
      const th = (BAYER4[(y / DOT) & 3][(x / DOT) & 3] + 0.5) / 16;

      let alpha = 0;
      let pal = WARM;

      // RESIDUE — ordered dither, structured but per-frame flickering so the spot
      // reads as live static running through it (not a fixed halftone decal).
      const resI = falloff * residue * (0.6 + n * 0.7);
      if (resI > th && rnd() < 0.45 + resI * 0.4) {
        alpha = (0.14 + resI * 0.4) * (0.65 + rnd() * 0.35);
        pal = falloff > 0.62 ? WARM : EMBER;
      }
      // CRACKLE — random sparks, densest at the core.
      const crkI = falloff * falloff * crackle;
      if (rnd() < crkI * 0.46) {
        alpha = Math.max(alpha, 0.34 + rnd() * 0.4);
        const roll = rnd();
        pal = roll < 0.2 ? HOT : roll < 0.28 ? EMBER : WARM;
        if (rnd() < 0.07) pal = COLD;
      }
      // IGNITE — contact flash at the very core.
      if (ignite > 0 && distW < r * 0.5 && rnd() < ignite * (0.2 + falloff * 0.55)) {
        alpha = Math.max(alpha, 0.5 + ignite * 0.38);
        pal = HOT;
      }
      if (alpha <= 0.01) continue;
      ctx.fillStyle = pal[(rnd() * pal.length) | 0];
      ctx.globalAlpha = Math.min(1, alpha);
      ctx.fillRect(x, y, DOT, DOT);
    }
  }

  // ARC FILAMENTS — a few short jagged sparks for the electric "snap".
  const arcs = Math.round(crackle * 2.4);
  for (let i = 0; i < arcs; i++) {
    let ax = p.x;
    let ay = p.y;
    let ang = rnd() * Math.PI * 2;
    const steps = 3 + ((rnd() * 4) | 0);
    for (let s = 0; s < steps; s++) {
      ang += (rnd() - 0.5) * 1.5;
      const len = DOT * (1.5 + rnd() * 2);
      ax += Math.cos(ang) * len;
      ay += Math.sin(ang) * len;
      if ((ax - p.x) ** 2 + (ay - p.y) ** 2 > r * r) break;
      ctx.fillStyle = rnd() < 0.5 ? HOT[0] : HOT[1];
      ctx.globalAlpha = Math.min(1, (0.4 + rnd() * 0.45) * crackle);
      ctx.fillRect(ax, ay, DOT, DOT);
    }
  }
  ctx.globalAlpha = 1;
}

function renderAscii(ctx: CanvasRenderingContext2D, p: Pulse, ageMs: number, rnd: Rnd) {
  const a = ageMs / DUR;
  if (a >= 1) return;
  const ignite = clamp01(1 - ageMs / 110);
  const crackle = Math.pow(clamp01(1 - ageMs / 330), 0.85);
  const residue = Math.pow(clamp01(1 - a), 1.5);
  const r = p.r + 4;
  ctx.font = `${A_FONT}px "Geist Mono", "JetBrains Mono", ui-monospace, monospace`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  // Rectangular ANSI cell grid (cw < ch) — the real AsciiMatrix proportion.
  const cx0 = Math.floor((p.x - r) / A_CW) * A_CW;
  const cy0 = Math.floor((p.y - r) / A_CH) * A_CH;

  for (let x = cx0; x <= p.x + r; x += A_CW) {
    for (let y = cy0; y <= p.y + r; y += A_CH) {
      const dx = x - p.x;
      const dy = y - p.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const n = vnoise(x * 1.7, y * 1.7);
      const distW = dist * (0.72 + n * 0.52);
      if (distW > r) continue;
      const falloff = 1 - distW / r;

      let inten = falloff * residue * (0.7 + n * 0.6);
      let pal = falloff > 0.6 ? WARM : EMBER;
      let scramble = false; // crackling cells churn through random ramp glyphs
      const crkI = falloff * falloff * crackle;
      if (rnd() < crkI * 0.5) {
        inten = Math.max(inten, 0.6 + rnd() * 0.4);
        const roll = rnd();
        pal = roll < 0.25 ? HOT : roll < 0.32 ? EMBER : WARM;
        if (rnd() < 0.08) pal = COLD;
        scramble = true;
      }
      if (ignite > 0 && distW < r * 0.5 && rnd() < ignite * (0.3 + falloff * 0.5)) {
        inten = Math.max(inten, 0.9);
        pal = HOT;
      }
      if (inten <= 0.06) continue;
      if (rnd() > 0.5 + inten * 0.45) continue; // per-frame visibility flicker

      // Genuine AsciiMatrix glyph: random scramble while crackling (the live
      // glitch), otherwise density keyed to intensity off the 70-char ramp.
      const glyph = scramble
        ? RAMP[(rnd() * RAMP.length) | 0]
        : rampGlyph(inten * (0.7 + rnd() * 0.55));
      if (glyph === " ") continue;
      ctx.fillStyle = pal[(rnd() * pal.length) | 0];
      ctx.globalAlpha = Math.min(1, 0.35 + inten * 0.6 * (0.7 + rnd() * 0.3));
      ctx.fillText(glyph, x + A_CW / 2, y + A_CH / 2);
    }
  }
  ctx.globalAlpha = 1;
}

const RENDER: Record<Exclude<Mode, "off">, typeof renderDither> = {
  dither: renderDither,
  ascii: renderAscii,
};

function loadMode(): Mode {
  try {
    const m = localStorage.getItem(STORE_KEY);
    if (m === "ascii" || m === "off" || m === "dither") return m;
  } catch {
    /* ignore */
  }
  return "ascii";
}

export function ClickPulse() {
  const ref = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const reduce =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (reduce) return;

    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    let W = 0;
    let H = 0;
    let pulses: Pulse[] = [];
    let raf = 0;
    let running = false;
    let mode = loadMode();

    const resize = () => {
      W = window.innerWidth;
      H = window.innerHeight;
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    };
    resize();

    const frame = (now: number) => {
      ctx.clearRect(0, 0, W, H);
      let alive = false;
      const render = mode === "off" ? null : RENDER[mode];
      for (const p of pulses) {
        const age = now - p.t0;
        if (age >= DUR) continue;
        alive = true;
        if (render) render(ctx, p, age, Math.random);
      }
      pulses = pulses.filter((p) => now - p.t0 < DUR);
      if (alive) {
        raf = requestAnimationFrame(frame);
      } else {
        running = false;
      }
    };

    // Click-hold-drag: a press spawns a burst, and while the button stays down
    // the field keeps spawning bursts that follow the cursor (a comet trail that
    // cools behind it). Releasing stops new bursts; the live ones finish their
    // ~600ms and fade. A plain click is just a down+up with no travel = one burst.
    let dragging = false;
    let lastX = 0;
    let lastY = 0;

    const emit = (x: number, y: number) => {
      pulses.push({ x, y, t0: performance.now(), r: R_MIN + Math.random() * R_VAR });
      if (pulses.length > MAX_PULSES) pulses.shift();
      if (!running) {
        running = true;
        raf = requestAnimationFrame(frame);
      }
    };

    const onDown = (e: PointerEvent) => {
      if (mode === "off" || e.button !== 0) return;
      if (!isDeadSpace(e.target)) return;
      dragging = true;
      lastX = e.clientX;
      lastY = e.clientY;
      emit(e.clientX, e.clientY);
    };

    const onMove = (e: PointerEvent) => {
      if (!dragging || mode === "off") return;
      const dx = e.clientX - lastX;
      const dy = e.clientY - lastY;
      if (dx * dx + dy * dy < EMIT_GAP * EMIT_GAP) return;
      lastX = e.clientX;
      lastY = e.clientY;
      // Stay on the "paper" — never trail across controls or raised surfaces.
      if (isDeadSpace(e.target)) emit(e.clientX, e.clientY);
    };

    const endDrag = () => {
      dragging = false;
    };

    // Ctrl/Cmd+Alt+P cycles dither → ascii → off (persisted) so both looks can
    // be tried live without a rebuild.
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.altKey && e.code === "KeyP") {
        e.preventDefault();
        mode = mode === "dither" ? "ascii" : mode === "ascii" ? "off" : "dither";
        try {
          localStorage.setItem(STORE_KEY, mode);
        } catch {
          /* ignore */
        }
        // eslint-disable-next-line no-console
        console.info(`[click-pulse] mode → ${mode}`);
      }
    };

    window.addEventListener("resize", resize);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("pointerup", endDrag);
    window.addEventListener("pointercancel", endDrag);
    window.addEventListener("blur", endDrag);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("resize", resize);
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("pointermove", onMove, true);
      window.removeEventListener("pointerup", endDrag);
      window.removeEventListener("pointercancel", endDrag);
      window.removeEventListener("blur", endDrag);
      window.removeEventListener("keydown", onKey);
      cancelAnimationFrame(raf);
    };
  }, []);

  return (
    <canvas
      ref={ref}
      aria-hidden="true"
      className="pointer-events-none fixed inset-0 z-40"
    />
  );
}
