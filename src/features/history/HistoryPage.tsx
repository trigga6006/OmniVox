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

export function HistoryPage() {
  const [records, setRecords] = useState<TranscriptionRecord[]>([]);
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const mountedRef = useRef(true);

  const load = useCallback(
    (searchQuery?: string) => {
      setLoading(true);
      const q = searchQuery ?? query;
      const request = q.trim()
        ? searchHistory(q.trim())
        : recentHistory(100);

      request
        .then((data) => {
          if (mountedRef.current) {
            setRecords(data);
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
    [query]
  );

  // Load on mount
  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  // Reload when search query changes (debounced)
  useEffect(() => {
    if (!mountedRef.current) return;
    const timer = setTimeout(() => load(query), query ? 250 : 0);
    return () => clearTimeout(timer);
  }, [query]); // eslint-disable-line react-hooks/exhaustive-deps

  // Auto-reload when a new transcription arrives while on this page
  useEffect(() => {
    const unlisten = onTranscriptionResult(() => {
      // Small delay for DB write to complete
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
    <div className="flex h-full flex-col p-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display font-semibold text-2xl text-text-primary">History</h1>
          <p className="text-sm text-text-muted mt-1">Transcription archive</p>
        </div>
        <button
          onClick={() => load()}
          className="p-2 rounded-lg hover:bg-surface-2 text-text-muted hover:text-text-secondary transition-colors"
          title="Refresh"
        >
          <RefreshCw size={16} strokeWidth={1.75} />
        </button>
      </div>

      {/* Search */}
      <div className="mt-4">
        <div className="flex items-center gap-2 bg-surface-1 rounded-lg px-3 py-2 border border-border focus-within:border-amber-500/40 transition-colors">
          <Search size={14} strokeWidth={1.75} className="text-text-muted shrink-0" />
          <input
            type="text"
            placeholder="Search transcriptions…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="flex-1 bg-transparent text-sm text-text-primary placeholder:text-text-muted outline-none"
          />
        </div>
      </div>

      {/* List */}
      {!loading && records.length === 0 ? (
        <div className="flex flex-1 flex-col items-center justify-center gap-4">
          <div className="relative flex items-center justify-center">
            <div className="absolute h-20 w-20 rounded-full bg-amber-500/5 border border-amber-700/20" />
            <Clock size={40} strokeWidth={1.5} className="relative text-text-muted" />
          </div>
          <div className="text-center mt-2">
            <p className="text-sm font-medium text-text-secondary">
              {query ? "No matching transcriptions" : "No transcriptions yet"}
            </p>
            <p className="text-xs text-text-muted mt-1">
              {query
                ? "Try a different search term"
                : "Your dictation history will appear here"}
            </p>
          </div>
        </div>
      ) : (
        <div className="mt-4 flex flex-col gap-2 overflow-y-auto flex-1 pr-1">
          {records.map((rec) => (
            <div
              key={rec.id}
              className="group bg-surface-1 rounded-lg border border-border hover:border-border-hover p-4 transition-colors"
            >
              <div className="flex items-start justify-between gap-3">
                <p className="text-sm text-text-primary leading-relaxed select-text flex-1">
                  {rec.text}
                </p>

                <div className="flex items-center gap-1 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={() => handleCopy(rec.text, rec.id)}
                    className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted hover:text-text-secondary transition-colors"
                    title="Copy"
                  >
                    {copiedId === rec.id ? (
                      <Check size={14} className="text-green-400" />
                    ) : (
                      <Copy size={14} />
                    )}
                  </button>
                  <button
                    onClick={() => handleDelete(rec.id)}
                    className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted hover:text-recording-400 transition-colors"
                    title="Delete"
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>

              <div className="flex items-center gap-3 mt-2 text-xs text-text-muted">
                <span>{formatDate(rec.created_at)}</span>
                <span className="opacity-40">·</span>
                <span>{formatDuration(rec.duration_ms)}</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
