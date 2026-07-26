import { StickyNote } from "lucide-react";

/**
 * The proximity "bud" — a small circle that springs out to the right of the
 * idle pill when the cursor comes near it. Its scratchpad glyph fades in on
 * hover; clicking opens the scratchpad window. Rendered as a SIBLING of the
 * pill button (the button is `overflow-hidden`, which would clip an outward
 * child) and absolutely positioned so revealing it never reflows the pill.
 */
export function ScratchpadBud({ onOpen }: { onOpen: () => void }) {
  return (
    <button className="scratchpad-bud" onClick={onOpen} aria-label="Open scratchpad">
      <StickyNote size={9} strokeWidth={2.25} className="scratchpad-bud-icon" />
    </button>
  );
}
