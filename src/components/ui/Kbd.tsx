import { type HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

export function Kbd({ className, ...props }: HTMLAttributes<HTMLElement>) {
  return (
    <kbd
      className={cn(
        "inline-flex items-center rounded-md border border-b-2 border-border-hover bg-surface-2",
        "px-1.5 py-0.5 font-mono text-[11px] font-medium text-text-secondary",
        className
      )}
      {...props}
    />
  );
}
