import { cn } from "@/lib/utils";

/** The app's canonical loading spinner — a bordered, amber-topped circle.
 *  Replaces the markup that was copy-pasted across App/Analytics and the bare
 *  "Loading…" text on Notes/Context-Modes. */
export function Spinner({ className }: { className?: string }) {
  return (
    <div
      className={cn(
        "h-4 w-4 animate-spin rounded-full border-2 border-text-muted/25 border-t-amber-400",
        className
      )}
    />
  );
}

/** Centered spinner for page/section loading fallbacks. */
export function LoadingState({ className }: { className?: string }) {
  return (
    <div className={cn("flex items-center justify-center py-10", className)}>
      <Spinner />
    </div>
  );
}
