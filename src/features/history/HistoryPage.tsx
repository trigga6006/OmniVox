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
import { Button, Card, Input, EmptyState } from "@/components/ui";

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

  // Refresh the currently visible window (all loaded pages, current query)
  // when a new transcription lands.  The backend saves the row before
  // emitting the event, so no settle timer is needed — and the ref avoids
  // the stale closure that used to reset an active search/pagination.
  const refresh = useCallback(() => {
    const q = query.trim();
    const limit = Math.max(records.length + 1, PAGE_SIZE);
    const request = q ? searchHistory(q, limit, 0) : recentHistory(limit, 0);
    request
      .then((data) => {
        if (mountedRef.current) {
          setRecords(data);
          setHasMore(data.length >= limit);
        }
      })
      .catch((e) => console.error("Failed to refresh history:", e));
  }, [query, records.length]);

  const refreshRef = useRef(refresh);
  useEffect(() => {
    refreshRef.current = refresh;
  }, [refresh]);

  useEffect(() => {
    const unlisten = onTranscriptionResult(() => refreshRef.current());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

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
    <div className="flex h-full flex-col px-8 pt-6 pb-8">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            History
          </h1>
          <p className="mt-1 text-sm text-text-muted">Transcription archive</p>
        </div>
        <Button
          variant="ghost"
          size="sm"
          icon={<RefreshCw />}
          onClick={() => load()}
          aria-label="Refresh"
          title="Refresh"
        />
      </div>

      {/* Search */}
      <div className="relative mt-4">
        <Search
          size={15}
          strokeWidth={1.75}
          className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text-muted"
        />
        <Input
          type="text"
          placeholder="Search transcriptions…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="pl-9"
        />
      </div>

      {/* List */}
      {!loading && records.length === 0 ? (
        <div className="flex flex-1 items-center justify-center">
          <EmptyState
            icon={<Clock />}
            title={query ? "No matching transcriptions" : "No transcriptions yet"}
            description={
              query
                ? "Try a different search term"
                : "Your dictation history will appear here"
            }
          />
        </div>
      ) : (
        <div className="mt-5 flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
          {records.map((rec) => (
            <Card
              key={rec.id}
              className="group px-4 py-3.5 transition-colors hover:border-border-hover"
            >
              <div className="flex items-start justify-between gap-3">
                <p className="flex-1 select-text text-[14.5px] leading-[1.55] text-text-primary">
                  {rec.text}
                </p>

                <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                  <Button
                    variant="ghost"
                    size="sm"
                    icon={
                      copiedId === rec.id ? (
                        <Check className="text-success" />
                      ) : (
                        <Copy />
                      )
                    }
                    onClick={() => handleCopy(rec.text, rec.id)}
                    aria-label="Copy"
                    title="Copy"
                  />
                  <Button
                    variant="danger"
                    size="sm"
                    icon={<Trash2 />}
                    onClick={() => handleDelete(rec.id)}
                    aria-label="Delete"
                    title="Delete"
                  />
                </div>
              </div>

              <div className="mt-2 flex items-center gap-2.5 text-[11px] tabular-nums text-text-muted">
                <span>{formatDate(rec.created_at)}</span>
                <span className="opacity-40">·</span>
                <span>{formatDuration(rec.duration_ms)}</span>
              </div>
            </Card>
          ))}
          {hasMore && records.length > 0 && (
            <Button
              variant="secondary"
              loading={loading}
              onClick={() => load(undefined, true)}
              disabled={loading}
              className="mt-1 self-center"
            >
              {loading ? "Loading…" : "Load more"}
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
