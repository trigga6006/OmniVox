import { useEffect, useState, useCallback, useRef } from "react";
import { Clock, Trash2, Copy, Check, Search, RefreshCw } from "lucide-react";
import {
  recentHistory,
  searchHistory,
  deleteHistoryRecord,
  onTranscriptionResult,
  type TranscriptionRecord,
} from "@/lib/tauri";
import { formatDuration } from "@/lib/utils";

const PAGE_SIZE = 50;

export function HistoryPage() {
  const [records, setRecords] = useState<TranscriptionRecord[]>([]);
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [hasMore, setHasMore] = useState(true);
  const mountedRef = useRef(true);

  const load = useCallback(
    (searchQuery?: string, append = false) => {
      setLoading(true);
      const q = searchQuery ?? query;
      const offset = append ? records.length : 0;
      const request = q.trim()
        ? searchHistory(q.trim(), PAGE_SIZE, offset)
        : recentHistory(PAGE_SIZE, offset);

      request
        .then((data) => {
          if (mountedRef.current) {
            setRecords((prev) => (append ? [...prev, ...data] : data));
            setHasMore(data.length >= PAGE_SIZE);
          }
        })
        .catch((e) => {
          console.error("Failed to load history:", e);
        })
        .finally(() => {
          if (mountedRef.current) {
            setLoading(false);
          }
        });
    },
    [query, records.length]
  );

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    if (!mountedRef.current) return;
    setHasMore(true);
    const timer = setTimeout(() => load(query, false), query ? 250 : 0);
    return () => clearTimeout(timer);
  }, [query]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    const unlisten = onTranscriptionResult(() => {
      setTimeout(() => load(), 300);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleCopy = useCallback((text: string, id: string) => {
    navigator.clipboard.writeText(text).then(() => {
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 1500);
    });
  }, []);

  const handleDelete = useCallback((id: string) => {
    deleteHistoryRecord(id)
      .then(() => setRecords((prev) => prev.filter((r) => r.id !== id)))
      .catch((e) => console.error("Failed to delete:", e));
  }, []);

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffMins = Math.floor(diffMs / 60_000);

    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  };

  return (
    <div className="flex h-full flex-col px-8 py-8">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            History
          </h1>
          <p className="mt-1 text-sm text-text-muted">Transcription archive</p>
        </div>
        <button
          onClick={() => load()}
          className="rounded-lg p-2 text-text-muted transition-colors hover:bg-surface-2/70 hover:text-text-secondary"
          title="Refresh"
        >
          <RefreshCw size={16} strokeWidth={1.75} />
        </button>
      </div>

      {/* Search */}
      <div className="mt-5">
        <div className="flex items-center gap-2 rounded-xl border border-border bg-surface-1/70 px-3.5 py-2.5 transition-colors focus-within:border-amber-400/40 focus-within:bg-surface-1">
          <Search size={14} strokeWidth={1.75} className="shrink-0 text-text-muted" />
          <input
            type="text"
            placeholder="Search transcriptions…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="flex-1 bg-transparent text-sm text-text-primary outline-none placeholder:text-text-muted"
          />
        </div>
      </div>

      {/* List */}
      {!loading && records.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-4">
          <div className="relative flex items-center justify-center">
            <div className="absolute h-20 w-20 rounded-full border border-amber-500/15 bg-amber-500/[0.05]" />
            <Clock size={36} strokeWidth={1.5} className="relative text-text-muted" />
          </div>
          <div className="mt-2 text-center">
            <p className="text-sm font-medium text-text-secondary">
              {query ? "No matching transcriptions" : "No transcriptions yet"}
            </p>
            <p className="mt-1 text-xs text-text-muted">
              {query
                ? "Try a different search term"
                : "Your dictation history will appear here"}
            </p>
          </div>
        </div>
      ) : (
        <div className="mt-5 flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
          {records.map((rec) => (
            <div
              key={rec.id}
              className="group rounded-xl border border-border bg-surface-1/80 px-4 py-3.5 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
            >
              <div className="flex items-start justify-between gap-3">
                <p className="flex-1 select-text text-[14.5px] leading-[1.55] text-text-primary">
                  {rec.text}
                </p>

                <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                  <button
                    onClick={() => handleCopy(rec.text, rec.id)}
                    className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
                    title="Copy"
                  >
                    {copiedId === rec.id ? (
                      <Check size={14} className="text-success" />
                    ) : (
                      <Copy size={14} />
                    )}
                  </button>
                  <button
                    onClick={() => handleDelete(rec.id)}
                    className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
                    title="Delete"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>

              <div className="mt-2 flex items-center gap-2.5 text-[11px] tabular-nums text-text-muted">
                <span>{formatDate(rec.created_at)}</span>
                <span className="opacity-40">·</span>
                <span>{formatDuration(rec.duration_ms)}</span>
              </div>
            </div>
          ))}
          {hasMore && records.length > 0 && (
            <button
              onClick={() => load(undefined, true)}
              disabled={loading}
              className="py-3 text-xs text-text-muted transition-colors hover:text-text-secondary disabled:opacity-50"
            >
              {loading ? "Loading…" : "Load more"}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
