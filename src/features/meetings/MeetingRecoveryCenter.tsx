import { AlertTriangle, CheckCircle2, ExternalLink, Loader2, RotateCcw, ShieldCheck } from "lucide-react";
import { Badge, Button } from "@/components/ui";
import type { MeetingRecoveryItem } from "@/lib/tauri";
import { cn } from "@/lib/utils";

export interface RecoveryPresentation {
  kind: "working" | "retry" | "review";
  title: string;
  detail: string;
}

export function recoveryPresentation(item: MeetingRecoveryItem): RecoveryPresentation {
  const inFlight = item.pending_chunks + item.processing_chunks;
  if (item.status === "transcribing" && inFlight > 0) {
    const finished = item.completed_chunks + item.failed_chunks;
    return {
      kind: "working",
      title: "Recovering transcript automatically",
      detail: `${finished} chunk${finished === 1 ? "" : "s"} finished · ${inFlight} remaining`,
    };
  }
  if (item.recoverable_failed_chunks > 0) {
    return {
      kind: "retry",
      title: "Captured audio needs another transcription attempt",
      detail: `${item.recoverable_failed_chunks} recoverable chunk${item.recoverable_failed_chunks === 1 ? "" : "s"} remain safely stored on this device.`,
    };
  }
  if (item.status === "interrupted") {
    return {
      kind: "review",
      title: item.has_summary ? "A notes update was interrupted" : "Meeting processing was interrupted",
      detail: item.has_summary
        ? "Existing notes are intact. Review the meeting before choosing whether to regenerate and spend again."
        : "Recovered transcript text is available. Review it before explicitly generating cloud notes.",
    };
  }
  return {
    kind: "review",
    title: "Meeting needs review",
    detail: item.error ?? "Open the meeting to review its capture and processing state.",
  };
}

export function MeetingRecoveryCenter({
  items,
  busyId,
  onRetry,
  onRetryAll,
  onOpenMeeting,
}: {
  items: MeetingRecoveryItem[];
  busyId: string | null;
  onRetry: (item: MeetingRecoveryItem) => void;
  onRetryAll: () => void;
  onOpenMeeting: (meetingId: string) => void;
}) {
  const retryable = items.filter((item) => item.recoverable_failed_chunks > 0);
  const working = items.filter((item) => recoveryPresentation(item).kind === "working");

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 items-center gap-2 border-b border-border px-6 py-3">
        <ShieldCheck size={15} className="shrink-0 text-amber-400" />
        <h2 className="shrink-0 text-sm font-semibold tracking-[-0.01em] text-text-primary">Meeting recovery</h2>
        <p className="min-w-0 flex-1 truncate text-xs text-text-muted">
          Sealed local audio resumes automatically. Paid AI work always waits for your explicit retry.
        </p>
        {working.length > 0 && (
          <span className="flex shrink-0 items-center gap-1.5 text-xs text-violet-300">
            <Loader2 size={11} className="animate-spin" />
            {working.length} recovering
          </span>
        )}
        {retryable.length > 1 && (
          <Button size="sm" variant="primary" disabled={busyId != null} onClick={onRetryAll} icon={<RotateCcw />}>
            Retry all local audio
          </Button>
        )}
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {items.length === 0 ? (
          <div className="flex min-h-72 flex-col items-center justify-center text-center">
            <div className="flex h-11 w-11 items-center justify-center rounded-full bg-success/[0.12] text-success">
              <CheckCircle2 size={19} />
            </div>
            <h3 className="mt-4 text-sm font-semibold text-text-primary">No recovery work</h3>
            <p className="mt-1 max-w-sm text-xs leading-relaxed text-text-muted">
              All captured meetings have finished local processing or are ready for ordinary use.
            </p>
          </div>
        ) : (
          <div className="mx-auto max-w-3xl space-y-2">
            {items.map((item) => {
              const presentation = recoveryPresentation(item);
              const retrying = busyId === item.meeting_id;
              return (
                <article
                  key={item.meeting_id}
                  className={cn(
                    "rounded-[var(--radius-l)] border p-4",
                    presentation.kind === "working"
                      ? "border-violet-500/15 bg-violet-500/[0.04]"
                      : presentation.kind === "retry"
                        ? "border-amber-500/20 bg-amber-500/[0.045]"
                        : "border-border bg-surface-2/45"
                  )}
                >
                  <div className="flex items-start gap-3">
                    <div
                      className={cn(
                        "mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-[var(--radius-m)]",
                        presentation.kind === "working"
                          ? "bg-violet-500/[0.12] text-violet-300"
                          : presentation.kind === "retry"
                            ? "bg-amber-500/[0.12] text-amber-400"
                            : "bg-recording-500/[0.12] text-recording-400"
                      )}
                    >
                      {presentation.kind === "working"
                        ? <Loader2 size={14} className="animate-spin" />
                        : <AlertTriangle size={14} />}
                    </div>

                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <h3 className="truncate text-sm font-medium text-text-primary">{item.title}</h3>
                        <Badge
                          tone={
                            presentation.kind === "working"
                              ? "violet"
                              : presentation.kind === "retry" ? "amber" : "neutral"
                          }
                        >
                          {item.status.replace("_", " ")}
                        </Badge>
                      </div>
                      <p className="mt-1.5 text-xs font-medium text-text-secondary">{presentation.title}</p>
                      <p className="mt-1 select-text text-xs leading-relaxed text-text-muted">{presentation.detail}</p>
                      {item.error && presentation.kind !== "review" && (
                        <p className="mt-2 line-clamp-2 select-text text-xs leading-relaxed text-amber-300/80">
                          {item.error}
                        </p>
                      )}
                    </div>

                    <div className="flex shrink-0 flex-col gap-1.5">
                      {presentation.kind === "retry" && (
                        <Button
                          size="sm"
                          variant="primary"
                          disabled={busyId != null}
                          onClick={() => onRetry(item)}
                          icon={retrying ? <Loader2 className="animate-spin" /> : <RotateCcw />}
                        >
                          {retrying ? "Retrying" : `Retry ${item.recoverable_failed_chunks}`}
                        </Button>
                      )}
                      <Button
                        size="sm"
                        variant="secondary"
                        onClick={() => onOpenMeeting(item.meeting_id)}
                        icon={<ExternalLink />}
                      >
                        Review meeting
                      </Button>
                    </div>
                  </div>
                </article>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
