import { type HTMLAttributes } from "react";
import { cn } from "@/lib/utils";

/**
 * Surface card. Carries `data-no-pulse` so the click-pulse never fires on top
 * of it — the pulse stays on page dead space. Compose with CardHeader / CardTitle
 * / CardBody, or just drop children in and pass your own padding.
 */
export function Card({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      data-no-pulse
      className={cn(
        "rounded-2xl border border-border bg-surface-1",
        className
      )}
      {...props}
    />
  );
}

export function CardHeader({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return (
    <div
      className={cn("flex items-center justify-between gap-3 px-5 pt-4 pb-3", className)}
      {...props}
    />
  );
}

export function CardTitle({ className, ...props }: HTMLAttributes<HTMLHeadingElement>) {
  return (
    <h3 className={cn("text-sm font-semibold text-text-primary", className)} {...props} />
  );
}

export function CardBody({ className, ...props }: HTMLAttributes<HTMLDivElement>) {
  return <div className={cn("px-5 pb-5", className)} {...props} />;
}
