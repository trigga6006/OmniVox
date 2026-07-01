import { cn } from "@/lib/utils";

interface ToggleProps {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  /** Structured Mode reads violet; everything else amber. */
  accent?: "amber" | "violet";
  "aria-label"?: string;
}

export function Toggle({ checked, onChange, disabled, accent = "amber", ...aria }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative h-[22px] w-[38px] shrink-0 rounded-full border transition-colors duration-200",
        "disabled:pointer-events-none disabled:opacity-50",
        checked
          ? accent === "violet"
            ? "border-transparent bg-violet-500"
            : "border-transparent bg-amber-500"
          : "border-border-hover bg-surface-3"
      )}
      {...aria}
    >
      <span
        className={cn(
          "absolute left-[2px] top-[2px] size-[16px] rounded-full bg-white shadow-sm",
          "transition-transform duration-200 ease-[cubic-bezier(0.4,0,0.2,1)]",
          checked && "translate-x-[16px]"
        )}
      />
    </button>
  );
}
