import { useEffect, useState } from "react";
import { X } from "lucide-react";
import {
  dismissMeetingSuggestion,
  meetingStart,
  onMeetingSuggestion,
  type MeetingSuggestionPayload,
} from "@/lib/tauri";
import { Logo } from "@/components/Logo";
import { Badge, Button } from "@/components/ui";

/**
 * Call-detected banner — a 372×124 top-right window. The card leaves an 8px
 * margin for its own shadow (the Tauri window draws none).
 */
export function MeetingSuggestion() {
  const [suggestion, setSuggestion] = useState<MeetingSuggestionPayload | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    document.documentElement.style.background = "transparent";
    document.documentElement.dataset.theme = "dark";
    document.body.style.background = "transparent";
    document.body.style.margin = "0";
    document.body.style.overflow = "hidden";
    const unlisten = onMeetingSuggestion(setSuggestion);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  if (!suggestion) return <div className="h-screen w-screen" />;

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      await meetingStart(suggestion.suggested_title, suggestion.app);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-screen w-screen items-center justify-center p-2">
      <div className="relative flex h-full w-full flex-col justify-between rounded-2xl border border-border bg-surface-1/95 px-3.5 py-3 text-text-primary shadow-[var(--shadow-lg)] backdrop-blur-xl animate-scale-in">
        <button
          onClick={() => dismissMeetingSuggestion()}
          className="absolute right-2 top-2 flex h-6 w-6 items-center justify-center rounded-lg text-text-muted transition-colors duration-150 hover:bg-surface-3 hover:text-text-primary"
          aria-label="Dismiss"
        >
          <X size={13} />
        </button>

        <div className="flex min-w-0 items-start gap-2.5 pr-7">
          <span className="mt-px flex h-6 w-6 shrink-0 items-center justify-center rounded-lg bg-amber-500/[0.12]">
            <Logo size={15} />
          </span>
          <div className="min-w-0 flex-1">
            <h2 className="truncate text-sm font-semibold leading-snug">
              {suggestion.suggested_title}
            </h2>
            <p className="truncate text-xs leading-snug text-text-secondary">
              Noticed a call in {suggestion.app}
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2">
          {error ? (
            <p role="alert" className="min-w-0 flex-1 truncate text-xs text-recording-400">
              {error}
            </p>
          ) : (
            <Badge tone="green">Local capture</Badge>
          )}
          <div className="ml-auto flex shrink-0 items-center gap-1.5">
            <Button variant="ghost" size="sm" onClick={() => dismissMeetingSuggestion()}>
              Not now
            </Button>
            <Button variant="primary" size="sm" loading={busy} onClick={start}>
              {busy ? "Starting…" : "Start recording"}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
