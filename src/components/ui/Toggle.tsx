import { cn } from "@/lib/utils";

interface ToggleProps {
  checked: boolean;
  onChange: (value: boolean) => void;
  disabled?: boolean;
  /** Structured Mode reads violet; AI Transcript Cleanup reads teal; everything else amber. */
  accent?: "amber" | "violet" | "teal";
  "aria-label"?: string;
}

const ON_TRACK: Record<NonNullable<ToggleProps["accent"]>, string> = {
  amber: "border-transparent bg-amber-500",
  violet: "border-transparent bg-violet-500",
  teal: "border-transparent bg-teal-500",
};

/**
 * One object, one motion: the track colour and the knob travel share the
 * same duration and curve (--dur-2 / --ease-out) so the switch reads as a
 * single moving thing instead of a knob sliding over a snapping background.
 */
export function Toggle({ checked, onChange, disabled, accent = "amber", ...aria }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "pressable relative h-[22px] w-[38px] shrink-0 rounded-full border",
        "transition-[background-color,border-color,transform,opacity]",
        "duration-[var(--dur-2)] ease-out",
        "disabled:cursor-not-allowed disabled:opacity-45",
        checked ? ON_TRACK[accent] : "border-border-hover bg-surface-3"
      )}
      {...aria}
    >
      <span
        className={cn(
          "absolute left-[2px] top-[2px] size-[16px] rounded-full bg-white shadow-[var(--shadow-sm)]",
          "transition-transform duration-[var(--dur-2)] ease-out",
          checked && "translate-x-[16px]"
        )}
      />
    </button>
  );
}
