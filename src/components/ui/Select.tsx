import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { INPUT_CHROME } from "./Input";

export interface SelectOption<T extends string> {
  value: T;
  label: string;
  /** Muted trailing text on the option row (e.g. a size or a hotkey). */
  hint?: ReactNode;
  disabled?: boolean;
}

interface SelectProps<T extends string> {
  options: SelectOption<T>[];
  /** Controlled value. Omit and pass `defaultValue` for uncontrolled use. */
  value?: T;
  defaultValue?: T;
  onChange?: (value: T) => void;
  placeholder?: string;
  disabled?: boolean;
  /** Applied to the trigger. */
  className?: string;
  /** Applied to the popover. */
  menuClassName?: string;
  id?: string;
  "aria-label"?: string;
}

const TYPEAHEAD_RESET_MS = 700;

/**
 * The app's dropdown. Replaces two styled native `<select>`s (which render
 * Windows' own list, ignoring every token) and two hand-rolled div menus that
 * each carried their own click-outside logic.
 *
 * Focus stays on the trigger and the list is driven by `aria-activedescendant`,
 * so there is no focus to lose on close and the amber focus ring never blinks.
 * The app is a single-window WebView2, so an absolutely-positioned popover with
 * a high z-index is sufficient — no portal needed.
 */
export function Select<T extends string>({
  options,
  value,
  defaultValue,
  onChange,
  placeholder = "Select…",
  disabled,
  className,
  menuClassName,
  id,
  ...aria
}: SelectProps<T>) {
  const reactId = useId();
  const baseId = id ?? `select-${reactId}`;

  const [internal, setInternal] = useState<T | undefined>(defaultValue);
  const current = value !== undefined ? value : internal;
  const selectedIndex = options.findIndex((o) => o.value === current);
  const selected = selectedIndex >= 0 ? options[selectedIndex] : undefined;

  const [open, setOpen] = useState(false);
  const [shown, setShown] = useState(false);
  const [active, setActive] = useState(0);

  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const typeahead = useRef({ buffer: "", at: 0 });

  const commit = (option: SelectOption<T>) => {
    if (option.disabled) return;
    if (value === undefined) setInternal(option.value);
    onChange?.(option.value);
    close();
  };

  const openMenu = () => {
    if (disabled) return;
    setActive(selectedIndex >= 0 ? selectedIndex : firstEnabled(options));
    setOpen(true);
  };

  const close = () => {
    setOpen(false);
    setShown(false);
  };

  // Mount collapsed, flip on the next frame so the transition runs.
  useLayoutEffect(() => {
    if (!open || shown) return;
    const frame = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(frame);
  }, [open, shown]);

  // Click-outside — one implementation, not one per menu.
  useEffect(() => {
    if (!open) return;
    const onDown = (e: PointerEvent) => {
      if (!rootRef.current?.contains(e.target as Node)) close();
    };
    document.addEventListener("pointerdown", onDown, true);
    return () => document.removeEventListener("pointerdown", onDown, true);
  }, [open]);

  // Keep the active option in view while arrowing through a long list.
  useEffect(() => {
    if (!open) return;
    listRef.current
      ?.querySelector(`[data-index="${active}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [open, active]);

  const step = (from: number, dir: 1 | -1) => {
    for (let i = 1; i <= options.length; i++) {
      const next = (from + dir * i + options.length * i) % options.length;
      if (!options[next].disabled) return next;
    }
    return from;
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (disabled) return;

    if (!open) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp" || e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openMenu();
      }
      return;
    }

    switch (e.key) {
      case "Escape":
        e.preventDefault();
        // Escape closed the MENU — it must not also reach a Modal wrapped
        // around this Select. Modal listens on `window`, so without this the
        // one keypress dismissed both and took the user's unsaved dialog input
        // with it. Only stopped while the menu is open; a closed Select returns
        // above and Modal still handles Escape.
        e.stopPropagation();
        close();
        return;
      case "ArrowDown":
        e.preventDefault();
        setActive((i) => step(i, 1));
        return;
      case "ArrowUp":
        e.preventDefault();
        setActive((i) => step(i, -1));
        return;
      case "Home":
        e.preventDefault();
        setActive(firstEnabled(options));
        return;
      case "End":
        e.preventDefault();
        setActive(step(0, -1));
        return;
      case "Enter":
      case " ":
        e.preventDefault();
        if (options[active]) commit(options[active]);
        return;
      case "Tab":
        close();
        return;
    }

    // Typeahead: jump to the first option starting with what you've typed.
    if (e.key.length === 1 && !e.metaKey && !e.ctrlKey && !e.altKey) {
      const now = Date.now();
      const buffer =
        now - typeahead.current.at > TYPEAHEAD_RESET_MS
          ? e.key.toLowerCase()
          : typeahead.current.buffer + e.key.toLowerCase();
      typeahead.current = { buffer, at: now };
      const hit = options.findIndex(
        (o) => !o.disabled && o.label.toLowerCase().startsWith(buffer)
      );
      if (hit >= 0) setActive(hit);
    }
  };

  return (
    <div ref={rootRef} className="relative">
      <button
        type="button"
        id={baseId}
        role="combobox"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? `${baseId}-list` : undefined}
        aria-activedescendant={open && options[active] ? `${baseId}-opt-${active}` : undefined}
        disabled={disabled}
        onClick={() => (open ? close() : openMenu())}
        onKeyDown={onKeyDown}
        className={cn(
          INPUT_CHROME,
          "flex h-[var(--control-h-m)] items-center justify-between gap-2 text-left",
          open && "border-amber-500",
          className
        )}
        {...aria}
      >
        <span className={cn("truncate", !selected && "text-text-muted")}>
          {selected?.label ?? placeholder}
        </span>
        <ChevronDown
          size={14}
          className={cn(
            "shrink-0 text-text-muted transition-transform duration-[var(--dur-2)] ease-out",
            open && "rotate-180"
          )}
        />
      </button>

      {open && (
        <div
          ref={listRef}
          id={`${baseId}-list`}
          role="listbox"
          aria-labelledby={baseId}
          className={cn(
            "absolute left-0 right-0 top-full z-[110] mt-1 max-h-64 origin-top overflow-y-auto",
            "rounded-[var(--radius-l)] border border-border-hover bg-surface-2 p-1 shadow-[var(--shadow-lg)]",
            "transition-[opacity,scale] duration-[var(--dur-2)] ease-out",
            shown ? "scale-100 opacity-100" : "scale-[0.97] opacity-0",
            menuClassName
          )}
        >
          {options.map((o, i) => {
            const isSelected = o.value === current;
            return (
              <div
                key={o.value}
                id={`${baseId}-opt-${i}`}
                data-index={i}
                role="option"
                aria-selected={isSelected}
                aria-disabled={o.disabled || undefined}
                onPointerEnter={() => !o.disabled && setActive(i)}
                onClick={() => commit(o)}
                className={cn(
                  "flex cursor-pointer items-center gap-2 rounded-[var(--radius-s)] px-2 py-1.5",
                  "text-sm transition-colors duration-[var(--dur-1)] ease-out",
                  o.disabled
                    ? "cursor-not-allowed opacity-45"
                    : i === active
                      ? "bg-surface-3 text-text-primary"
                      : "text-text-secondary"
                )}
              >
                <Check
                  size={13}
                  strokeWidth={2.5}
                  className={cn("shrink-0 text-amber-400", !isSelected && "invisible")}
                />
                <span className="min-w-0 flex-1 truncate">{o.label}</span>
                {o.hint && (
                  <span className="shrink-0 font-mono text-2xs text-text-muted">{o.hint}</span>
                )}
              </div>
            );
          })}

          {options.length === 0 && (
            <p className="px-2 py-3 text-center text-xs text-text-muted">Nothing to choose from.</p>
          )}
        </div>
      )}
    </div>
  );
}

function firstEnabled<T extends string>(options: SelectOption<T>[]) {
  const i = options.findIndex((o) => !o.disabled);
  return i < 0 ? 0 : i;
}
