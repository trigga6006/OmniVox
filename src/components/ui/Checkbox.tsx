import { useId, type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface CheckboxProps {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  /** Optional label; clicking it toggles the box. */
  label?: ReactNode;
  /** Muted second line under the label. */
  hint?: ReactNode;
  className?: string;
  "aria-label"?: string;
}

/**
 * 16px box, 4px radius, amber when checked, tick drawn in over --dur-1.
 * Replaces the three native `accent-amber` OS checkboxes — those render
 * Windows' own control, which ignores every token in the system.
 */
export function Checkbox({
  checked,
  onChange,
  disabled,
  label,
  hint,
  className,
  ...aria
}: CheckboxProps) {
  const labelId = useId();

  const box = (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      aria-labelledby={label ? labelId : undefined}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        // 4px — the spec's `--radius-s − 2`, derived rather than typed so it
        // tracks the radius scale.
        "inline-flex size-4 shrink-0 items-center justify-center border",
        "rounded-[calc(var(--radius-s)-2px)]",
        "transition-[background-color,border-color,scale] duration-[var(--dur-1)] ease-out",
        "active:scale-95",
        "disabled:cursor-not-allowed disabled:opacity-45",
        checked
          ? "border-amber-500 bg-amber-500"
          : "border-border-hover bg-surface-2 hover:border-amber-500/60",
        !label && className
      )}
      {...aria}
    >
      <svg
        viewBox="0 0 16 16"
        className="size-3.5"
        fill="none"
        stroke="var(--primary-foreground)"
        strokeWidth={2.25}
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <path
          d="M3.5 8.4 6.4 11.3 12.5 4.9"
          pathLength={1}
          strokeDasharray={1}
          style={{
            strokeDashoffset: checked ? 0 : 1,
            transition: "stroke-dashoffset var(--dur-1) var(--ease-out)",
          }}
        />
      </svg>
    </button>
  );

  if (!label) return box;

  return (
    <div className={cn("flex items-start gap-2.5", className)}>
      {box}
      <div className="min-w-0">
        <span
          id={labelId}
          onClick={() => !disabled && onChange(!checked)}
          className={cn(
            "block cursor-pointer text-sm leading-tight text-text-primary",
            disabled && "cursor-not-allowed opacity-45"
          )}
        >
          {label}
        </span>
        {hint && <span className="mt-0.5 block text-xs text-text-muted">{hint}</span>}
      </div>
    </div>
  );
}
