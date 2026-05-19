import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  Upload,
  FileAudio,
  Copy,
  Check,
  StickyNote,
  Trash2,
  Plus,
  X,
  AlertCircle,
  Loader2,
} from "lucide-react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";

import { useImportStore } from "@/stores/importStore";
import { useRecordingStore } from "@/stores/recordingStore";
import { useToastStore } from "@/stores/toastStore";
import { addNote, listNotes, updateNote, type Note } from "@/lib/tauri";
import { useAppStore } from "@/stores/appStore";
import { cn } from "@/lib/utils";
import { formatDuration } from "@/lib/utils";

const ACCEPTED_EXTENSIONS = ["wav", "mp3", "m4a", "flac", "ogg", "oga", "aac"];

function isSupported(path: string): boolean {
  const lower = path.toLowerCase();
  return ACCEPTED_EXTENSIONS.some((ext) => lower.endsWith(`.${ext}`));
}

function basename(path: string): string {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx >= 0 ? path.slice(idx + 1) : path;
}

function stripExt(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

export function ImportsPage() {
  const state = useImportStore((s) => s.state);
  const startImport = useImportStore((s) => s.startImport);
  const cancelImport = useImportStore((s) => s.cancelImport);
  const reset = useImportStore((s) => s.reset);
  const updateTranscript = useImportStore((s) => s.updateTranscript);

  const recordingStatus = useRecordingStore((s) => s.status);
  const isRecording =
    recordingStatus === "recording" || recordingStatus === "processing";

  const addToast = useToastStore((s) => s.addToast);

  // Drag/drop via Tauri webview API (HTML5 File doesn't expose local paths).
  useEffect(() => {
    let dispose: (() => void) | null = null;
    let cancelled = false;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const paths = event.payload.paths ?? [];
        if (paths.length === 0) return;
        const [first, ...rest] = paths;
        if (rest.length > 0) {
          addToast({
            message: `Importing ${basename(first)}; ignoring ${rest.length} other file${rest.length === 1 ? "" : "s"}.`,
            level: "info",
          });
        }
        if (!isSupported(first)) {
          addToast({
            message: `Unsupported file type. Try WAV, MP3, M4A, FLAC, OGG, or AAC.`,
            level: "error",
          });
          return;
        }
        if (isRecording) {
          addToast({
            message: "Stop recording before importing.",
            level: "error",
          });
          return;
        }
        startImport(first);
      })
      .then((unlisten) => {
        if (cancelled) {
          unlisten();
        } else {
          dispose = unlisten;
        }
      })
      .catch((e) => console.error("onDragDropEvent:", e));
    return () => {
      cancelled = true;
      if (dispose) dispose();
    };
  }, [addToast, startImport, isRecording]);

  const handleBrowse = useCallback(async () => {
    try {
      const picked = await openDialog({
        multiple: false,
        directory: false,
        filters: [
          {
            name: "Audio",
            extensions: ACCEPTED_EXTENSIONS,
          },
        ],
      });
      if (typeof picked === "string") {
        if (isRecording) {
          addToast({ message: "Stop recording before importing.", level: "error" });
          return;
        }
        startImport(picked);
      }
    } catch (e) {
      console.error("dialog open failed:", e);
    }
  }, [addToast, startImport, isRecording]);

  return (
    <div className="flex h-full flex-col px-8 py-8">
      <header>
        <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
          Imports
        </h1>
        <p className="mt-1 text-sm text-text-muted">
          Drop an audio file to transcribe it locally
        </p>
      </header>

      <div className="mt-8 flex flex-1 items-start justify-center">
        {state.kind === "idle" && (
          <IdleView onBrowse={handleBrowse} disabled={isRecording} />
        )}
        {state.kind === "decoding" && (
          <ProgressView
            filename={state.filename}
            phase="Decoding"
            percent={state.percent}
            durationMs={null}
            onCancel={cancelImport}
          />
        )}
        {state.kind === "transcribing" && (
          <ProgressView
            filename={state.filename}
            phase="Transcribing"
            percent={state.percent}
            durationMs={state.durationMs}
            onCancel={cancelImport}
          />
        )}
        {state.kind === "cancelling" && (
          <CancellingView filename={state.filename} />
        )}
        {state.kind === "complete" && (
          <CompleteView
            filename={state.filename}
            durationMs={state.durationMs}
            transcript={state.transcript}
            onUpdate={updateTranscript}
            onDiscard={reset}
            onNewImport={() => {
              reset();
              handleBrowse();
            }}
          />
        )}
        {state.kind === "error" && (
          <ErrorView
            filename={state.filename}
            message={state.message}
            onRetry={() => {
              reset();
              handleBrowse();
            }}
            onDismiss={reset}
          />
        )}
      </div>
    </div>
  );
}

/* ── Idle / drop zone ──────────────────────────────────────────────── */

function IdleView({
  onBrowse,
  disabled,
}: {
  onBrowse: () => void;
  disabled: boolean;
}) {
  return (
    <div className="flex w-full max-w-xl flex-col items-center">
      <div
        className={cn(
          "flex w-full flex-col items-center rounded-2xl border-2 border-dashed border-amber-400/25 bg-surface-1/60 px-10 py-14 text-center transition-colors",
          "hover:border-amber-400/50 hover:bg-surface-1/80",
          disabled && "cursor-not-allowed opacity-50 hover:border-amber-400/25 hover:bg-surface-1/60"
        )}
      >
        <div className="relative mb-5">
          <div className="absolute inset-0 -m-3 rounded-2xl border border-amber-500/15 bg-amber-500/[0.06]" />
          <Upload size={28} strokeWidth={1.5} className="relative text-amber-300" />
        </div>
        <p className="text-sm font-medium text-text-primary">
          Drop an audio file here
        </p>
        <p className="mt-1 text-xs text-text-muted">
          or{" "}
          <button
            onClick={onBrowse}
            disabled={disabled}
            className="text-amber-300 underline-offset-2 hover:underline disabled:no-underline"
          >
            browse files
          </button>
        </p>
        <p className="mt-6 text-[11px] text-text-muted/70">
          WAV · MP3 · M4A · FLAC · OGG · AAC · up to 500 MB
        </p>
        {disabled && (
          <p className="mt-3 text-[11px] text-recording-300/70">
            Stop the active recording first.
          </p>
        )}
      </div>
    </div>
  );
}

/* ── Decoding / transcribing ───────────────────────────────────────── */

function ProgressView({
  filename,
  phase,
  percent,
  durationMs,
  onCancel,
}: {
  filename: string;
  phase: "Decoding" | "Transcribing";
  percent: number;
  durationMs: number | null;
  onCancel: () => void;
}) {
  return (
    <div className="flex w-full max-w-xl flex-col rounded-2xl border border-border bg-surface-1/80 px-7 py-7">
      <div className="flex items-center gap-3">
        <FileAudio
          size={22}
          strokeWidth={1.5}
          className="shrink-0 text-amber-300"
        />
        <div className="flex-1 truncate">
          <p className="truncate text-sm font-medium text-text-primary">
            {filename}
          </p>
          <p className="mt-0.5 text-[11px] uppercase tracking-[0.12em] text-text-muted">
            {phase}
            {durationMs ? ` · ${formatDuration(durationMs)}` : ""}
          </p>
        </div>
        <button
          onClick={onCancel}
          className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
          title="Cancel"
        >
          <X size={16} />
        </button>
      </div>

      <div className="mt-6 h-1.5 w-full overflow-hidden rounded-full bg-surface-2">
        <div
          className="h-full rounded-full bg-amber-400/60 transition-[width] duration-300 ease-out"
          style={{ width: `${Math.min(Math.max(percent, 0), 100)}%` }}
        />
      </div>
      <div className="mt-1.5 flex justify-end text-[11px] tabular-nums text-text-muted">
        {Math.round(percent)}%
      </div>
    </div>
  );
}

/* ── Cancelling ────────────────────────────────────────────────────── */

/**
 * After the user clicks Cancel, Whisper's abort callback can take
 * 1–10 s to actually unwind (it's polled between decode segments).
 * We sit on this view until the backend emits `import-cancelled` —
 * that's the only signal that the gate + active_import are fully
 * released and the user can safely start a recording or new import.
 */
function CancellingView({ filename }: { filename: string }) {
  return (
    <div className="flex w-full max-w-xl flex-col items-center rounded-2xl border border-border bg-surface-1/80 px-7 py-9 text-center">
      <Loader2
        size={26}
        strokeWidth={1.5}
        className="animate-spin text-text-muted"
      />
      <p className="mt-4 text-sm font-medium text-text-primary">
        Cancelling…
      </p>
      <p className="mt-1 truncate text-xs text-text-muted">{filename}</p>
      <p className="mt-3 text-[11px] text-text-muted/70">
        Waiting for the transcription engine to release.
      </p>
    </div>
  );
}

/* ── Complete ──────────────────────────────────────────────────────── */

function CompleteView({
  filename,
  durationMs,
  transcript,
  onUpdate,
  onDiscard,
  onNewImport,
}: {
  filename: string;
  durationMs: number;
  transcript: string;
  onUpdate: (text: string) => void;
  onDiscard: () => void;
  onNewImport: () => void;
}) {
  const [copied, setCopied] = useState(false);
  const [savePickerOpen, setSavePickerOpen] = useState(false);

  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(transcript).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    });
  }, [transcript]);

  const wordCount = useMemo(
    () => transcript.trim().split(/\s+/).filter(Boolean).length,
    [transcript]
  );

  return (
    <div className="flex w-full max-w-2xl flex-col rounded-2xl border border-border bg-surface-1/80 px-7 py-7">
      <div className="flex items-center gap-3">
        <FileAudio size={20} strokeWidth={1.5} className="shrink-0 text-amber-300" />
        <div className="flex-1 truncate">
          <p className="truncate text-sm font-medium text-text-primary">
            {filename}
          </p>
          <p className="mt-0.5 text-[11px] tabular-nums text-text-muted">
            {formatDuration(durationMs)} · {wordCount.toLocaleString()} words
          </p>
        </div>
      </div>

      <textarea
        value={transcript}
        onChange={(e) => onUpdate(e.target.value)}
        spellCheck={false}
        className="mt-5 h-[42vh] w-full resize-none rounded-xl border border-border/55 bg-surface-0/70 px-4 py-3 text-sm leading-[1.65] text-text-primary outline-none transition-colors focus:border-amber-400/40"
      />

      <div className="mt-4 flex flex-wrap items-center justify-end gap-2">
        <button
          onClick={handleCopy}
          className="flex items-center gap-1.5 rounded-lg border border-border bg-surface-2/70 px-3 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:border-border-hover hover:bg-surface-2"
        >
          {copied ? <Check size={12} className="text-success" /> : <Copy size={12} />}
          {copied ? "Copied" : "Copy"}
        </button>
        <button
          onClick={() => setSavePickerOpen(true)}
          className="flex items-center gap-1.5 rounded-lg border border-amber-400/30 bg-amber-500/[0.10] px-3 py-1.5 text-xs font-medium text-amber-300 transition-colors hover:border-amber-400/55 hover:bg-amber-500/[0.18]"
        >
          <StickyNote size={12} />
          Save to Note
        </button>
        <button
          onClick={onNewImport}
          className="rounded-lg border border-border bg-surface-2/70 px-3 py-1.5 text-xs font-medium text-text-secondary transition-colors hover:border-border-hover hover:bg-surface-2"
        >
          New import
        </button>
        <button
          onClick={onDiscard}
          className="flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-xs font-medium text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
        >
          <Trash2 size={12} />
          Discard
        </button>
      </div>

      {savePickerOpen && (
        <SaveToNotePopover
          filename={filename}
          transcript={transcript}
          onClose={() => setSavePickerOpen(false)}
          onSaved={onDiscard}
        />
      )}
    </div>
  );
}

/* ── Save to note picker ───────────────────────────────────────────── */

function SaveToNotePopover({
  filename,
  transcript,
  onClose,
  onSaved,
}: {
  filename: string;
  transcript: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [notes, setNotes] = useState<Note[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState("");
  const [saving, setSaving] = useState(false);
  const addToast = useToastStore((s) => s.addToast);
  const setPage = useAppStore((s) => s.setPage);
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    listNotes()
      .then((data) => setNotes(data))
      .catch((e) => console.error("listNotes:", e))
      .finally(() => setLoading(false));
  }, []);

  // Click outside to close.
  useEffect(() => {
    const onClick = (e: MouseEvent) => {
      if (!containerRef.current?.contains(e.target as Node)) onClose();
    };
    // Defer so the opening click doesn't immediately close.
    const id = setTimeout(() => document.addEventListener("mousedown", onClick), 50);
    return () => {
      clearTimeout(id);
      document.removeEventListener("mousedown", onClick);
    };
  }, [onClose]);

  const filtered = useMemo(() => {
    if (!filter.trim()) return notes;
    const q = filter.toLowerCase();
    return notes.filter((n) =>
      n.title.toLowerCase().includes(q) || n.content.toLowerCase().includes(q)
    );
  }, [filter, notes]);

  const handleNew = useCallback(async () => {
    setSaving(true);
    try {
      const title = stripExt(filename);
      await addNote(title, transcript);
      addToast({ message: `Created note “${title}”`, level: "info" });
      onSaved();
      setPage("notes");
    } catch (e) {
      console.error("addNote:", e);
      addToast({ message: "Failed to create note", level: "error" });
    } finally {
      setSaving(false);
    }
  }, [filename, transcript, addToast, onSaved, setPage]);

  const handleAppend = useCallback(
    async (note: Note) => {
      setSaving(true);
      try {
        const heading = `## ${filename}`;
        const sep = note.content.length === 0 ? "" : "\n\n";
        const merged = `${note.content}${sep}${heading}\n\n${transcript}`;
        await updateNote(note.id, note.title, merged);
        addToast({
          message: `Appended to “${note.title}”`,
          level: "info",
        });
        onSaved();
        setPage("notes");
      } catch (e) {
        console.error("updateNote:", e);
        addToast({ message: "Failed to update note", level: "error" });
      } finally {
        setSaving(false);
      }
    },
    [filename, transcript, addToast, onSaved, setPage]
  );

  return (
    <div className="fixed inset-0 z-40 flex items-end justify-end bg-black/30 sm:items-center sm:justify-center">
      <div
        ref={containerRef}
        className="w-full max-w-sm rounded-2xl border border-border bg-surface-1 p-4 shadow-2xl"
      >
        <div className="flex items-center justify-between">
          <p className="text-sm font-medium text-text-primary">Save to note</p>
          <button
            onClick={onClose}
            className="rounded-md p-1 text-text-muted hover:bg-surface-2 hover:text-text-secondary"
          >
            <X size={14} />
          </button>
        </div>

        <button
          onClick={handleNew}
          disabled={saving}
          className="mt-3 flex w-full items-center gap-2 rounded-lg border border-amber-400/35 bg-amber-500/[0.10] px-3 py-2 text-left text-sm text-amber-300 transition-colors hover:border-amber-400/55 hover:bg-amber-500/[0.18] disabled:opacity-60"
        >
          <Plus size={14} />
          <span className="flex-1 truncate">
            New note from “{stripExt(filename)}”
          </span>
        </button>

        <div className="mt-3">
          <input
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            placeholder="Search existing notes…"
            className="w-full rounded-lg border border-border bg-surface-2/60 px-3 py-1.5 text-xs text-text-primary outline-none transition-colors focus:border-amber-400/40"
          />
        </div>

        <div className="mt-2 max-h-64 overflow-y-auto pr-0.5">
          {loading ? (
            <div className="py-6 text-center text-xs text-text-muted">Loading…</div>
          ) : filtered.length === 0 ? (
            <div className="py-6 text-center text-xs text-text-muted">
              {notes.length === 0
                ? "No existing notes — create a new one above"
                : "No notes match your filter"}
            </div>
          ) : (
            <ul className="flex flex-col gap-1">
              {filtered.map((note) => (
                <li key={note.id}>
                  <button
                    onClick={() => handleAppend(note)}
                    disabled={saving}
                    className="flex w-full flex-col rounded-lg border border-transparent px-3 py-2 text-left transition-colors hover:border-border hover:bg-surface-2/70 disabled:opacity-60"
                  >
                    <span className="truncate text-sm text-text-primary">
                      {note.title || "Untitled"}
                    </span>
                    <span className="truncate text-[11px] text-text-muted">
                      {note.content ? note.content.slice(0, 90) : "Empty note"}
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </div>
    </div>
  );
}

/* ── Error ─────────────────────────────────────────────────────────── */

function ErrorView({
  filename,
  message,
  onRetry,
  onDismiss,
}: {
  filename: string | null;
  message: string;
  onRetry: () => void;
  onDismiss: () => void;
}) {
  return (
    <div className="flex w-full max-w-xl flex-col rounded-2xl border border-recording-500/35 bg-recording-500/[0.05] px-7 py-6">
      <div className="flex items-start gap-3">
        <AlertCircle
          size={20}
          strokeWidth={1.75}
          className="mt-0.5 shrink-0 text-recording-400"
        />
        <div className="flex-1">
          <p className="text-sm font-medium text-text-primary">
            Couldn't import {filename ?? "audio"}
          </p>
          <p className="mt-1 text-xs text-text-muted">{message}</p>
        </div>
      </div>
      <div className="mt-5 flex justify-end gap-2">
        <button
          onClick={onDismiss}
          className="rounded-lg px-3 py-1.5 text-xs font-medium text-text-muted hover:text-text-secondary"
        >
          Dismiss
        </button>
        <button
          onClick={onRetry}
          className="rounded-lg border border-amber-400/35 bg-amber-500/[0.12] px-3 py-1.5 text-xs font-medium text-amber-300 hover:border-amber-400/60 hover:bg-amber-500/[0.20]"
        >
          Pick another file
        </button>
      </div>
    </div>
  );
}
