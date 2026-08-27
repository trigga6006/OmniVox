import { forwardRef, type InputHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/**
 * The field chrome the rest of the kit copies — Select's trigger and
 * Textarea reuse this exact recipe, so keep the three in step.
 */
export const INPUT_CHROME = cn(
  "w-full rounded-[var(--radius-m)] border border-border-hover bg-surface-2 px-3",
  "text-sm text-text-primary placeholder:text-text-muted",
  "transition-[border-color,box-shadow,background-color] duration-[var(--dur-2)] ease-out",
  "focus:border-amber-500 focus:outline-none focus:ring-[3px] focus:ring-amber-500/[0.15]",
  "disabled:cursor-not-allowed disabled:opacity-45"
);

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, ...props }, ref) {
    return (
      <input
        ref={ref}
        className={cn(INPUT_CHROME, "h-[var(--control-h-m)]", className)}
        {...props}
      />
    );
  }
);
