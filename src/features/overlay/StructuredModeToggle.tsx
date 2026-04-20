import { cn } from "@/lib/utils";

interface Props {
  active: boolean;
  onToggle: () => void;
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
export function StructuredModeToggle({ active, onToggle }: Props) {
  return (
    <button
      onMouseDown={(e) => {
        e.stopPropagation();
        e.preventDefault();
        onToggle();
      }}
      aria-pressed={active}
      aria-label={
        active ? "Structured Mode: on" : "Structured Mode: off"
      }
      title={
        active
          ? "Structured Mode is ON — dictation runs through the LLM"
          : "Structured Mode is OFF — click to enable"
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

const styles = `
.ley-line {
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
    rgba(40,36,32,0.78) 0%,
    rgba(28,26,24,0.82) 100%);
  border: 1px solid rgba(232,180,95,0.16);
  box-shadow:
    inset 0 1px 0 rgba(255,235,200,0.045),
    0 2px 6px -2px rgba(0,0,0,0.45);

  transition:
    background 320ms cubic-bezier(0.4, 0, 0.2, 1),
    border-color 320ms cubic-bezier(0.4, 0, 0.2, 1),
    box-shadow 320ms cubic-bezier(0.4, 0, 0.2, 1),
    transform 180ms cubic-bezier(0.34, 1.56, 0.64, 1);
}
.ley-line:hover {
  border-color: rgba(232,180,95,0.3);
  box-shadow:
    inset 0 1px 0 rgba(255,235,200,0.08),
    0 3px 10px -3px rgba(0,0,0,0.55);
}
.ley-line:active { transform: scale(0.96); }

/* --- ON STATE ------------------------------------------------- */

.ley-line--on {
  background: linear-gradient(180deg,
    rgba(172,128,230,0.34) 0%,
    rgba(140,100,202,0.22) 52%,
    rgba(118,82,184,0.28) 100%);
  border-color: rgba(210,178,246,0.55);
  /* Static glow — deliberately no breathing animation.  The button
     already reads as "on" from the violet fill, border, and dot; a
     pulsing halo was distracting in the broader UI context. */
  box-shadow:
    inset 0 1px 0 rgba(255,245,255,0.16),
    inset 0 -1px 0 rgba(40,20,80,0.3),
    0 0 12px -3px rgba(188,150,236,0.42),
    0 2px 10px -2px rgba(0,0,0,0.5);
}
.ley-line--on:hover {
  background: linear-gradient(180deg,
    rgba(184,138,240,0.44) 0%,
    rgba(152,108,216,0.3) 52%,
    rgba(128,90,198,0.36) 100%);
  border-color: rgba(224,196,252,0.68);
  box-shadow:
    inset 0 1px 0 rgba(255,245,255,0.2),
    inset 0 -1px 0 rgba(40,20,80,0.3),
    0 0 16px -2px rgba(200,168,240,0.55),
    0 2px 10px -2px rgba(0,0,0,0.5);
}

/* --- GLYPH ---------------------------------------------------- */

.ley-line-glyph {
  display: inline-flex;
  z-index: 2;
  transition: opacity 280ms ease, filter 280ms ease;
}
.ley-line-glyph svg {
  fill: rgba(255,235,200,0.32);
  transition: fill 320ms cubic-bezier(0.4, 0, 0.2, 1);
}
.ley-line:hover .ley-line-glyph svg {
  fill: rgba(255,240,210,0.6);
}
.ley-line--on .ley-line-glyph svg {
  fill: rgba(245,232,255,0.96);
  filter: drop-shadow(0 0 4px rgba(200,168,240,0.55));
}

/* --- SPINE ---------------------------------------------------- */

.ley-line-spine {
  position: relative;
  width: 1px;
  flex: 1 1 auto;
  margin: 2px 0;
  background: linear-gradient(180deg,
    rgba(232,180,95,0) 0%,
    rgba(232,180,95,0.12) 35%,
    rgba(232,180,95,0.12) 65%,
    rgba(232,180,95,0) 100%);
  z-index: 1;
  overflow: hidden;
  transition: background 320ms cubic-bezier(0.4, 0, 0.2, 1);
}
.ley-line--on .ley-line-spine {
  background: linear-gradient(180deg,
    rgba(200,168,240,0) 0%,
    rgba(218,192,248,0.55) 25%,
    rgba(232,208,255,0.7) 50%,
    rgba(218,192,248,0.55) 75%,
    rgba(200,168,240,0) 100%);
}

/* --- STATE DOT ----------------------------------------------- */

.ley-line-dot {
  display: inline-block;
  width: 4px;
  height: 4px;
  border-radius: 50%;
  background: rgba(255,235,200,0.18);
  z-index: 2;
  transition:
    background 280ms ease,
    box-shadow 280ms ease;
}
.ley-line--on .ley-line-dot {
  /* Static lit dot — no breathing.  The soft ring + halo is enough to
     read as "on" at a glance without any motion. */
  background: rgb(210,178,246);
  box-shadow:
    0 0 0 1.5px rgba(188,150,236,0.24),
    0 0 6px rgba(200,168,240,0.55);
}
`;
