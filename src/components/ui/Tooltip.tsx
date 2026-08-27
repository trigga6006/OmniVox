import { useEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";

type Side = "top" | "bottom" | "left" | "right";

interface TooltipProps {
  /** What the tooltip says. Keep it to a phrase. */
  content: ReactNode;
  children: ReactNode;
  side?: Side;
  /** Hotkeys, model ids and other machine text set in Geist Mono. */
  mono?: boolean;
  /** Open delay in ms. Ignored when chaining off a just-closed tooltip. */
  delay?: number;
  disabled?: boolean;
  /** Applied to the inline-flex wrapper around the trigger. */
  className?: string;
}

/**
 * The one tooltip. Replaces native `title=`, FloatingPill's `data-tip` CSS and
 * Analytics' bespoke BarTooltip.
 *
 * Perceived speed is the whole point: the first tooltip in a toolbar waits
 * 350ms, but if another one closed within the last 300ms the next opens with
 * zero delay and no entrance animation. Scanning a row of icon buttons then
 * feels like reading labels rather than waiting on six separate timers.
 */

/** Module-level so every Tooltip instance shares one "was one just open?" clock. */
let lastClosedAt = 0;
const CHAIN_WINDOW_MS = 300;

const SIDE_POSITION: Record<Side, string> = {
  top: "bottom-full left-1/2 -translate-x-1/2 mb-1.5 origin-bottom",
  bottom: "top-full left-1/2 -translate-x-1/2 mt-1.5 origin-top",
  left: "right-full top-1/2 -translate-y-1/2 mr-1.5 origin-right",
  right: "left-full top-1/2 -translate-y-1/2 ml-1.5 origin-left",
};

export function Tooltip({
  content,
  children,
  side = "top",
  mono,
  delay = 350,
  disabled,
  className,
}: TooltipProps) {
  const [open, setOpen] = useState(false);
  const [shown, setShown] = useState(false);
  const [instant, setInstant] = useState(false);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => () => window.clearTimeout(timer.current), []);

  // Mount at scale(.97)/opacity-0, then flip on the next frame so the CSS
  // transition actually runs. Chained tooltips skip this entirely.
  useEffect(() => {
    if (!open || shown) return;
    const id = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(id);
  }, [open, shown]);

  const show = (pointerType?: string) => {
    // Pointer-only: a touch "hover" would strand the tooltip on screen.
    if (disabled || pointerType === "touch" || pointerType === "pen") return;
    window.clearTimeout(timer.current);
    if (Date.now() - lastClosedAt < CHAIN_WINDOW_MS) {
      setInstant(true);
      setOpen(true);
      setShown(true);
      return;
    }
    setInstant(false);
    timer.current = window.setTimeout(() => setOpen(true), delay);
  };

  const hide = () => {
    window.clearTimeout(timer.current);
    if (open) lastClosedAt = Date.now();
    setOpen(false);
    setShown(false);
  };

  return (
    <span
      className={cn("relative inline-flex", className)}
      onPointerEnter={(e) => show(e.pointerType)}
      onPointerLeave={hide}
      onPointerDown={hide}
      onFocus={() => show("mouse")}
      onBlur={hide}
      onKeyDown={(e) => {
        if (e.key === "Escape") hide();
      }}
    >
      {children}
      {open && (
        <span
          role="tooltip"
          className={cn(
            "pointer-events-none absolute z-[120] whitespace-nowrap",
            "rounded-[var(--radius-s)] border border-border-hover bg-surface-3 px-2 py-1",
            "text-xs leading-none text-text-primary shadow-[var(--shadow-md)]",
            mono && "font-mono tracking-tight",
            SIDE_POSITION[side],
            // Tailwind v4 emits `scale:`, not `transform:` — name it explicitly.
            !instant && "transition-[opacity,scale] duration-[125ms] ease-out",
            shown ? "scale-100 opacity-100" : "scale-[0.97] opacity-0"
          )}
        >
          {content}
        </span>
      )}
    </span>
  );
}
