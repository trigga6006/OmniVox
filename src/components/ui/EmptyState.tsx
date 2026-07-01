import { type ReactNode } from "react";
import { cn } from "@/lib/utils";

interface EmptyStateProps {
  icon?: ReactNode;
  title: string;
  description?: string;
  action?: ReactNode;
  className?: string;
}

export function EmptyState({ icon, title, description, action, className }: EmptyStateProps) {
  return (
    <div
      className={cn(
        "flex flex-col items-center justify-center gap-3 px-6 py-14 text-center",
        className
      )}
    >
      {icon && (
        <div className="flex size-11 items-center justify-center rounded-full bg-surface-2 text-text-muted [&>svg]:size-5">
          {icon}
        </div>
      )}
      <div className="space-y-1">
        <p className="text-sm font-medium text-text-primary">{title}</p>
        {description && (
          <p className="mx-auto max-w-xs text-xs leading-relaxed text-text-muted">{description}</p>
        )}
      </div>
      {action}
    </div>
  );
}
