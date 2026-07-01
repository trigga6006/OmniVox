import { type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface SegmentedProps<T extends string> {
  options: { value: T; label: string; icon?: ReactNode }[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
}

/** Compact segmented control for 2–4 mutually exclusive choices. */
export function Segmented<T extends string>({
  options,
  value,
  onChange,
  className,
}: SegmentedProps<T>) {
  return (
    <div
      className={cn(
        "inline-flex items-center gap-0.5 rounded-[9px] border border-border bg-surface-2 p-[3px]",
        className
      )}
    >
      {options.map((o) => {
        const on = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            onClick={() => onChange(o.value)}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-md px-3 py-[5px] text-xs font-medium transition-colors",
              "[&>svg]:size-[14px]",
              on
                ? "bg-surface-3 text-text-primary shadow-sm"
                : "text-text-muted hover:text-text-secondary"
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
