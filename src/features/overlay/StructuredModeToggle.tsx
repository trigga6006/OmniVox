import { cn } from "@/lib/utils";

interface Props {
  active: boolean;
  onToggle: () => void;
  /**
   * Right-click handler — opens the Voice-Command popup that gates
   * Structured Mode behind the "Voxify" trigger word.  Mirrors the
   * Ship button's right-click popup pattern.
   */
  onContextMenu?: (e: React.MouseEvent) => void;
}

/**
 * "The Ley Line" — the Structured Mode toggle.
 *
 * A vertical capsule button that sits to the left of the pill-overlay menu,
 * symmetric with (but deliberately distinct from) the round quick-toggles on
 * the right side.  Visually represents the conduit that raw speech flows
 * through to become a structured prompt.
 *
 * Design intent:
 *   - Off state reads as a latent rune: mostly transparent, a thin amber
 *     hairline, faint glyph.  Present but dormant.
 *   - On state reads as awakened: violet gradient body, glowing border,
 *     outer halo, and — most importantly — a bright "energy pulse"
 *     travelling continuously up the central spine.  The animated spine is
 *     the focal detail that separates this from a generic toggle.
 *   - The glyph is a custom three-bar hierarchy mark (decreasing bar
 *     lengths) that evokes "structured indentation" without leaning on a
 *     stock icon.
 *
 * The press itself kicks off the menu-wide transition (amber → violet or
 * reverse) — that choreography lives in `ModeSelector`, not here.
 */
export function StructuredModeToggle({
  active,
  onToggle,
  onContextMenu,
}: Props) {
  return (
    <button
      onMouseDown={(e) => {
        e.stopPropagation();
        e.preventDefault();
        // Left-click toggles; right-click opens the voice-command popup.
        if (e.button === 0) onToggle();
      }}
      onContextMenu={(e) => {
        e.preventDefault();
        e.stopPropagation();
        onContextMenu?.(e);
      }}
      aria-pressed={active}
      aria-label={
        active ? "Structured Mode: on" : "Structured Mode: off"
      }
      title={
        active
          ? "Structured Mode is ON — dictation runs through the LLM (right-click for options)"
          : "Structured Mode is OFF — click to enable (right-click for options)"
      }
      className={cn("ley-line", active && "ley-line--on")}
    >
      {/* Top glyph: hierarchy mark (three stacked bars of descending length) */}
      <span className="ley-line-glyph" aria-hidden="true">
        <svg viewBox="0 0 18 16" width="14" height="12">
          <rect x="0" y="1" width="18" height="1.6" rx="0.8" />
          <rect x="0" y="7.2" width="13" height="1.6" rx="0.8" />
          <rect x="0" y="13.4" width="8" height="1.6" rx="0.8" />
        </svg>
      </span>

      {/* The spine — a thin violet line down the middle.  When active, it
          reads as a quietly-lit conduit; no traveling animation (too busy
          for the context — the button lives alongside several other UI
          elements so it needs to hold presence without pulling focus). */}
      <span className="ley-line-spine" aria-hidden="true" />

      {/* Bottom state dot */}
      <span className="ley-line-dot" aria-hidden="true" />

      <style>{styles}</style>
    </button>
  );
}

/*
 * Palette port. This block used to hand-type 37 rgba() literals in two
 * off-brand families: a warm salmon/brown for the off state (a survivor of the
 * pre-graphite palette — the comments already called it "amber" while the value
 * was #e8957e) and a dusty mauve for the on state (rgb(161,118,142), which the
 * spec calls out by name). Both now read from tokens, so Structured Mode has
 * exactly one violet and the hairline is actually amber.
 *
 * Every duration, easing curve and transform below is unchanged.
 */
const styles = `
.ley-line {
  /* Token palette for this control — declared on the root element so every
     descendant rule below inherits it. */
  --ll-surface-top: var(--color-surface-2);
  --ll-surface-bottom: var(--color-surface-1);
  --ll-accent: var(--color-amber-500);
  --ll-glint: var(--color-cream);
  --ll-ink: var(--color-text-primary);
  --ll-violet: var(--color-violet-400);
  --ll-violet-deep: var(--color-violet-500);
  --ll-violet-deepest: var(--color-violet-600);
  --ll-violet-light: var(--color-violet-300);
  --ll-violet-lightest: var(--color-violet-200);

  /* Shape — a vertical capsule, deliberately taller than a quick-toggle
     circle so it reads as a flagship control rather than a setting. */
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: space-between;
  width: 28px;
  height: 64px;
  padding: 7px 0 8px;
  border-radius: 14px;
  cursor: pointer;
  overflow: hidden;
  isolation: isolate;

  /* Off state — latent rune: mostly dark with an amber hairline.  Thin
     inner highlight hints at depth without pulling focus. */
  background: linear-gradient(180deg,
    color-mix(in srgb, var(--ll-surface-top) 78%, transparent) 0%,
    color-mix(in srgb, var(--ll-surface-bottom) 82%, transparent) 100%);
  border: 1px solid color-mix(in srgb, var(--ll-accent) 16%, transparent);
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, var(--ll-glint) 4.5%, transparent),
    0 2px 6px -2px rgba(0,0,0,0.45);

  transition:
    background 320ms cubic-bezier(0.4, 0, 0.2, 1),
    border-color 320ms cubic-bezier(0.4, 0, 0.2, 1),
    box-shadow 320ms cubic-bezier(0.4, 0, 0.2, 1),
    transform 180ms cubic-bezier(0.34, 1.56, 0.64, 1);
}
.ley-line:hover {
  border-color: color-mix(in srgb, var(--ll-accent) 30%, transparent);
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, var(--ll-glint) 8%, transparent),
    0 3px 10px -3px rgba(0,0,0,0.55);
}
.ley-line:active { transform: scale(0.96); }

/* --- ON STATE ------------------------------------------------- */

.ley-line--on {
  background: linear-gradient(180deg,
    color-mix(in srgb, var(--ll-violet) 34%, transparent) 0%,
    color-mix(in srgb, var(--ll-violet-deep) 22%, transparent) 52%,
    color-mix(in srgb, var(--ll-violet-deepest) 28%, transparent) 100%);
  border-color: color-mix(in srgb, var(--ll-violet) 55%, transparent);
  /* Static glow — deliberately no breathing animation.  The button
     already reads as "on" from the violet fill, border, and dot; a
     pulsing halo was distracting in the broader UI context. */
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, var(--ll-glint) 16%, transparent),
    inset 0 -1px 0 rgba(0,0,0,0.3),
    0 2px 10px -2px rgba(0,0,0,0.5);
}
.ley-line--on:hover {
  background: linear-gradient(180deg,
    color-mix(in srgb, var(--ll-violet-light) 44%, transparent) 0%,
    color-mix(in srgb, var(--ll-violet) 30%, transparent) 52%,
    color-mix(in srgb, var(--ll-violet-deep) 36%, transparent) 100%);
  border-color: color-mix(in srgb, var(--ll-violet-light) 68%, transparent);
  box-shadow:
    inset 0 1px 0 color-mix(in srgb, var(--ll-glint) 20%, transparent),
    inset 0 -1px 0 rgba(0,0,0,0.3),
    0 2px 10px -2px rgba(0,0,0,0.5);
}

/* --- GLYPH ---------------------------------------------------- */

.ley-line-glyph {
  display: inline-flex;
  z-index: 2;
  transition: opacity 280ms ease, filter 280ms ease;
}
.ley-line-glyph svg {
  fill: color-mix(in srgb, var(--ll-ink) 32%, transparent);
  transition: fill 320ms cubic-bezier(0.4, 0, 0.2, 1);
}
.ley-line:hover .ley-line-glyph svg {
  fill: color-mix(in srgb, var(--ll-ink) 60%, transparent);
}
.ley-line--on .ley-line-glyph svg {
  fill: color-mix(in srgb, var(--ll-ink) 96%, transparent);
}

/* --- SPINE ---------------------------------------------------- */

.ley-line-spine {
  position: relative;
  width: 1px;
  flex: 1 1 auto;
  margin: 2px 0;
  background: linear-gradient(180deg,
    transparent 0%,
    color-mix(in srgb, var(--ll-accent) 12%, transparent) 35%,
    color-mix(in srgb, var(--ll-accent) 12%, transparent) 65%,
    transparent 100%);
  z-index: 1;
  overflow: hidden;
  transition: background 320ms cubic-bezier(0.4, 0, 0.2, 1);
}
.ley-line--on .ley-line-spine {
  background: linear-gradient(180deg,
    transparent 0%,
    color-mix(in srgb, var(--ll-violet-light) 55%, transparent) 25%,
    color-mix(in srgb, var(--ll-violet-lightest) 70%, transparent) 50%,
    color-mix(in srgb, var(--ll-violet-light) 55%, transparent) 75%,
    transparent 100%);
}

/* --- STATE DOT ----------------------------------------------- */

.ley-line-dot {
  display: inline-block;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--ll-ink) 18%, transparent);
  z-index: 2;
  transition:
    background 280ms ease,
    box-shadow 280ms ease;
}
.ley-line--on .ley-line-dot {
  /* Static lit dot — no breathing.  The soft ring + halo is enough to
     read as "on" at a glance without any motion. */
  background: var(--ll-violet);
  box-shadow:
    0 0 0 1.5px color-mix(in srgb, var(--ll-violet) 24%, transparent);
}
`;
