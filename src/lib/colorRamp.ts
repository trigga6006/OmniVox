/**
 * Canvas colour, read from the design tokens instead of retyped.
 *
 * Three canvases (ClickPulse, CommandDither, DitherOrb) each carried their own
 * hardcoded amber ramp in JS. They were theme-blind by construction — a
 * `<canvas>` cannot use `var(--color-amber-500)` in a `fillStyle` — so retuning
 * the accent in `tokens.css` left them behind.
 *
 * These helpers resolve a token to a concrete colour once, through the browser,
 * so `color-mix()`, `oklch()`, and plain hex all work the same. Results are
 * cached per theme, so a light/dark flip produces a fresh ramp on the next read
 * rather than a stale one; within a theme every call after the first is a Map
 * lookup.
 *
 * Callers build their ramp at mount (in the effect that owns the canvas). That
 * makes a ramp static for the life of the mount — long enough for a 600ms click
 * pulse, and re-run whenever the effect's own deps change.
 */

export type Rgb = readonly [number, number, number];

/** `[cssVariableName, fallbackLiteral]` — the fallback is the value the token
 *  is expected to carry, kept beside it so a drift is visible in review and so
 *  jsdom (which computes nothing) still renders. */
export type RampEntry = readonly [varName: string, fallback: string];
export type RgbRampEntry = readonly [varName: string, fallback: Rgb];

const cache = new Map<string, string>();

/** Light and dark resolve the same token differently; key the cache on it. */
function themeKey() {
  if (typeof document === "undefined") return "none";
  return document.documentElement.dataset.theme ?? "system";
}

/**
 * Push a colour through the browser's own parser so every token form —
 * `#f59e0b`, `rgb(…)`, `oklch(…)`, `color-mix(…)` — comes back as `rgb(r, g, b)`.
 */
function resolve(value: string): string | null {
  if (typeof document === "undefined" || !document.body) return null;
  const probe = document.createElement("span");
  probe.style.cssText = "position:absolute;visibility:hidden;pointer-events:none";
  probe.style.color = value;
  document.body.appendChild(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  // jsdom hands back the input verbatim; a real engine always normalises.
  return computed.startsWith("rgb") ? computed : null;
}

/**
 * One token as a canvas-ready colour string.
 *
 * @param varName  CSS custom property, with or without the leading `--`.
 * @param fallback Used when there is no DOM, or the token does not resolve.
 */
export function readCssColor(varName: string, fallback: string): string {
  const name = varName.startsWith("--") ? varName : `--${varName}`;
  const key = `${themeKey()}|${name}`;
  const hit = cache.get(key);
  if (hit !== undefined) return hit;

  const declared =
    typeof document === "undefined"
      ? ""
      : getComputedStyle(document.documentElement).getPropertyValue(name).trim();

  const value = resolve(declared || fallback) ?? fallback;
  cache.set(key, value);
  return value;
}

/** One token as an `[r, g, b]` triple, for per-pixel `ImageData` work. */
export function readCssRgb(varName: string, fallback: Rgb): Rgb {
  const parsed = /(-?[\d.]+)\D+(-?[\d.]+)\D+(-?[\d.]+)/.exec(
    readCssColor(varName, `rgb(${fallback[0]}, ${fallback[1]}, ${fallback[2]})`)
  );
  if (!parsed) return fallback;
  return [Number(parsed[1]), Number(parsed[2]), Number(parsed[3])] as const;
}

/** A ramp of canvas-ready colour strings, dimmest→brightest by convention. */
export function buildRamp(entries: readonly RampEntry[]): string[] {
  return entries.map(([name, fallback]) => readCssColor(name, fallback));
}

/** A ramp of `[r, g, b]` triples. */
export function buildRgbRamp(entries: readonly RgbRampEntry[]): Rgb[] {
  return entries.map(([name, fallback]) => readCssRgb(name, fallback));
}
