import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface SegmentedProps<T extends string> {
  options: { value: T; label: string; icon?: ReactNode }[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
}

/**
 * Compact segmented control for 2–4 mutually exclusive choices.
 *
 * The active state is one absolutely-positioned pill that *slides* to the
 * selected segment (transform + width, --dur-2, --ease-out) instead of a
 * class swap. CSS transitions are interruptible, so rapid clicking retargets
 * mid-flight rather than restarting; the global reduced-motion rule collapses
 * the duration for users who ask for it.
 */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  className,
}: SegmentedProps<T>) {
  const listRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef(new Map<string, HTMLButtonElement>());
  const [pill, setPill] = useState<{ x: number; w: number } | null>(null);
  // Suppress the transition until the first measurement lands, otherwise the
  // pill animates in from x=0/w=0 on mount.
  const [ready, setReady] = useState(false);

  const optionsKey = options.map((o) => o.value).join("\u0000");

  useLayoutEffect(() => {
    const list = listRef.current;
    const el = itemRefs.current.get(value);
    if (!list || !el) {
      setPill(null);
      return;
    }
    const measure = () => {
      const x = el.offsetLeft;
      const w = el.offsetWidth;
      setPill((prev) => (prev && prev.x === x && prev.w === w ? prev : { x, w }));
    };
    measure();
    // jsdom has no ResizeObserver — measure once and skip the live tracking
    // so component tests can render this without a shim.
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(list);
    ro.observe(el);
    return () => ro.disconnect();
  }, [value, optionsKey]);

  useLayoutEffect(() => {
    if (!pill || ready) return;
    const id = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(id);
  }, [pill, ready]);

  return (
    <div
      ref={listRef}
      className={cn(
        "relative inline-flex items-center gap-0.5 rounded-[var(--radius-m)]",
        "border border-border bg-surface-2 p-[3px]",
        className
      )}
    >
      {pill && (
        <span
          aria-hidden="true"
          className={cn(
            "pointer-events-none absolute left-0 top-[3px] bottom-[3px]",
            "rounded-[var(--radius-s)] bg-surface-3 shadow-[var(--shadow-sm)]",
            ready && "transition-[transform,width] duration-[var(--dur-2)] ease-out"
          )}
          style={{ transform: `translateX(${pill.x}px)`, width: pill.w }}
        />
      )}

      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            aria-pressed={on}
            ref={(node) => {
              if (node) itemRefs.current.set(o.value, node);
              else itemRefs.current.delete(o.value);
            }}
            onClick={() => onChange(o.value)}
            className={cn(
              "pressable relative z-10 inline-flex items-center gap-1.5",
              "rounded-[var(--radius-s)] px-3 py-[5px] text-xs font-medium",
              "transition-[color,transform] duration-[var(--dur-2)] ease-out",
              "[&>svg]:size-[14px]",
              on ? "text-text-primary" : "text-text-muted hover:text-text-secondary"
            )}
          >
            {o.icon}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
