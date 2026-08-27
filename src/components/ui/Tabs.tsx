import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { cn } from "@/lib/utils";

export interface TabItem<T extends string> {
  value: T;
  label: string;
  icon?: ReactNode;
}

interface TabsProps<T extends string> {
  items: TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
  /** Ties the row to its panel for assistive tech. */
  idPrefix?: string;
}

/**
 * Page-level tab row: a hairline rail with a 2px amber underline that *slides*
 * to the selected tab (transform + width, --dur-2, --ease-out). Segmented is
 * the control-sized sibling; this one is for navigating a page's sections.
 *
 * Pair with `TabPanel` for the asymmetric crossfade the motion standard asks
 * for — out 120ms, in 200ms with a 2px rise.
 */
export function Tabs<T extends string>({
  items,
  value,
  onChange,
  className,
  idPrefix = "tab",
}: TabsProps<T>) {
  const listRef = useRef<HTMLDivElement>(null);
  const tabRefs = useRef(new Map<string, HTMLButtonElement>());
  const [rail, setRail] = useState<{ x: number; w: number } | null>(null);
  const [ready, setReady] = useState(false);

  const itemsKey = items.map((i) => i.value).join(" ");

  useLayoutEffect(() => {
    const list = listRef.current;
    const el = tabRefs.current.get(value);
    if (!list || !el) {
      setRail(null);
      return;
    }
    const measure = () => {
      const x = el.offsetLeft;
      const w = el.offsetWidth;
      setRail((prev) => (prev && prev.x === x && prev.w === w ? prev : { x, w }));
    };
    measure();
    // jsdom has no ResizeObserver — measure once and skip the live tracking.
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(list);
    ro.observe(el);
    return () => ro.disconnect();
  }, [value, itemsKey]);

  useLayoutEffect(() => {
    if (!rail || ready) return;
    const id = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(id);
  }, [rail, ready]);

  const move = (dir: 1 | -1) => {
    const i = items.findIndex((t) => t.value === value);
    const next = items[(i + dir + items.length) % items.length];
    if (!next) return;
    onChange(next.value);
    tabRefs.current.get(next.value)?.focus();
  };

  return (
    <div
      ref={listRef}
      role="tablist"
      className={cn("relative flex items-center gap-1 border-b border-border", className)}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight") {
          e.preventDefault();
          move(1);
        } else if (e.key === "ArrowLeft") {
          e.preventDefault();
          move(-1);
        } else if (e.key === "Home" && items[0]) {
          e.preventDefault();
          onChange(items[0].value);
          tabRefs.current.get(items[0].value)?.focus();
        } else if (e.key === "End" && items.length) {
          e.preventDefault();
          const last = items[items.length - 1];
          onChange(last.value);
          tabRefs.current.get(last.value)?.focus();
        }
      }}
    >
      {rail && (
        <span
          aria-hidden="true"
          className={cn(
            "pointer-events-none absolute bottom-[-1px] left-0 h-[2px] rounded-full bg-amber-500",
            ready && "transition-[transform,width] duration-[var(--dur-2)] ease-out"
          )}
          style={{ transform: `translateX(${rail.x}px)`, width: rail.w }}
        />
      )}

      {items.map((t) => {
        const on = t.value === value;
        return (
          <button
            key={t.value}
            type="button"
            role="tab"
            id={`${idPrefix}-${t.value}`}
            aria-selected={on}
            aria-controls={`${idPrefix}-${t.value}-panel`}
            tabIndex={on ? 0 : -1}
            ref={(node) => {
              if (node) tabRefs.current.set(t.value, node);
              else tabRefs.current.delete(t.value);
            }}
            onClick={() => onChange(t.value)}
            className={cn(
              "pressable inline-flex items-center gap-1.5 rounded-t-[var(--radius-s)]",
              "px-3 pb-2 pt-1.5 text-sm font-medium",
              "transition-[color,transform] duration-[var(--dur-2)] ease-out",
              "[&>svg]:size-[14px]",
              on ? "text-text-primary" : "text-text-muted hover:text-text-secondary"
            )}
          >
            {t.icon}
            {t.label}
          </button>
        );
      })}
    </div>
  );
}

interface TabPanelProps {
  /** The currently selected tab's value — changing it drives the crossfade. */
  tabKey: string;
  children: ReactNode;
  className?: string;
  idPrefix?: string;
}

/**
 * Crossfade wrapper for tab content: the outgoing panel fades out over 120ms,
 * then the incoming one fades in over 200ms with a 2px rise. Asymmetric on
 * purpose — leaving should feel quicker than arriving.
 */
export function TabPanel({ tabKey, children, className, idPrefix = "tab" }: TabPanelProps) {
  const [render, setRender] = useState<{ key: string; node: ReactNode }>({
    key: tabKey,
    node: children,
  });
  const [phase, setPhase] = useState<"in" | "out">("in");
  const latest = useRef(children);
  latest.current = children;

  useEffect(() => {
    if (tabKey === render.key) {
      // Same tab — keep the node fresh without re-running the transition.
      setRender((r) => (r.node === latest.current ? r : { key: r.key, node: latest.current }));
      return;
    }
    setPhase("out");
    const id = window.setTimeout(() => {
      setRender({ key: tabKey, node: latest.current });
    }, 120);
    return () => window.clearTimeout(id);
  }, [tabKey, render.key, children]);

  // The swap lands while still faded out; flip in on the next frame.
  useEffect(() => {
    if (phase !== "out" || render.key !== tabKey) return;
    const id = requestAnimationFrame(() => setPhase("in"));
    return () => cancelAnimationFrame(id);
  }, [phase, render.key, tabKey]);

  return (
    <div
      role="tabpanel"
      id={`${idPrefix}-${render.key}-panel`}
      aria-labelledby={`${idPrefix}-${render.key}`}
      className={cn(
        "transition-[opacity,translate] ease-out",
        phase === "out"
          ? "translate-y-[2px] opacity-0 duration-[var(--dur-1)]"
          : "translate-y-0 opacity-100 duration-[var(--dur-3)]",
        className
      )}
    >
      {render.node}
    </div>
  );
}
