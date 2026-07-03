import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { cn } from "@/lib/utils";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-amber-500 text-[#1a1206] font-semibold hover:bg-amber-400",
  secondary:
    "bg-surface-2 text-text-primary border border-border-hover hover:bg-surface-3",
  ghost: "bg-transparent text-text-secondary hover:bg-surface-2 hover:text-text-primary",
  danger: "bg-error text-white font-medium hover:brightness-110",
};

const SIZES: Record<Size, string> = {
  sm: "h-8 px-3 text-[13px] gap-1.5 rounded-[9px]",
  md: "h-9 px-4 text-sm gap-2 rounded-[10px]",
};

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  size?: Size;
  loading?: boolean;
  /** Leading icon (a lucide element). Hidden while loading. */
  icon?: ReactNode;
}

/**
 * The one button. Variants map to the graphite·amber roles: primary = amber,
 * danger = red, secondary/ghost = neutral. Use this instead of inline buttons.
 */
export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", loading, icon, className, children, disabled, ...props },
  ref
) {
  return (
    <button
      ref={ref}
      disabled={disabled || loading}
      className={cn(
        "inline-flex select-none items-center justify-center whitespace-nowrap font-medium",
        "transition-[background-color,border-color,transform,opacity] duration-150",
        "active:scale-[0.985] disabled:pointer-events-none disabled:opacity-50",
        "[&>svg]:size-[15px] [&>svg]:shrink-0",
        SIZES[size],
        VARIANTS[variant],
        className
      )}
      {...props}
    >
      {loading ? <Loader2 className="animate-spin" /> : icon}
      {children}
    </button>
  );
});
