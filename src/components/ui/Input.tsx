import { forwardRef, type InputHTMLAttributes } from "react";
import { cn } from "@/lib/utils";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(
  function Input({ className, ...props }, ref) {
    return (
      <input
        ref={ref}
        className={cn(
          "h-9 w-full rounded-[9px] border border-border-hover bg-surface-2 px-3",
          "text-sm text-text-primary placeholder:text-text-muted transition-colors",
          "focus:border-amber-500 focus:outline-none focus:ring-[3px] focus:ring-amber-500/[0.15]",
          "disabled:pointer-events-none disabled:opacity-50",
          className
        )}
        {...props}
      />
    );
  }
);
