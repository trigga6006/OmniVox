import { type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface FieldProps {
  label: string;
  hint?: string;
  /** The control (Toggle, Segmented, Input, Button…). */
  children: ReactNode;
  className?: string;
}

/** A labeled settings row: title (+ optional hint) on the left, control on the right. */
export function Field({ label, hint, children, className }: FieldProps) {
  return (
    <div className={cn("flex items-center justify-between gap-4 py-3", className)}>
      <div className="min-w-0">
        <div className="text-[13px] text-text-primary">{label}</div>
        {hint && <div className="mt-0.5 text-[11.5px] leading-snug text-text-muted">{hint}</div>}
      </div>
      <div className="shrink-0">{children}</div>
    </div>
  );
}
