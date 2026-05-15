import { useEffect, useState, useCallback, useRef } from "react";
import { StickyNote, Plus, Trash2, ArrowLeft, Check } from "lucide-react";
import {
  listNotes,
  addNote,
  updateNote,
  deleteNote,
  onTranscriptionResult,
  type Note,
} from "@/lib/tauri";

type View = "grid" | "editor";

export function NotesPage() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [loading, setLoading] = useState(true);
  const [view, setView] = useState<View>("grid");
  const [activeNote, setActiveNote] = useState<Note | null>(null);
  const [editTitle, setEditTitle] = useState("");
  const [editContent, setEditContent] = useState("");
  const [saved, setSaved] = useState(false);
  const mountedRef = useRef(true);
  const titleRef = useRef<HTMLInputElement>(null);
  const autoSaveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    listNotes()
      .then((data) => {
        if (mountedRef.current) setNotes(data);
      })
      .catch((e) => console.error("Failed to load notes:", e))
      .finally(() => {
        if (mountedRef.current) setLoading(false);
      });
  }, []);

  useEffect(() => {
    mountedRef.current = true;
    load();
    return () => {
      mountedRef.current = false;
    };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleNew = useCallback(async () => {
    try {
      const note = await addNote("Untitled", "");
      setActiveNote(note);
      setEditTitle(note.title);
      setEditContent(note.content);
      setView("editor");
      setTimeout(() => titleRef.current?.select(), 50);
    } catch (e) {
      console.error("Failed to create note:", e);
    }
  }, []);

  const handleOpen = useCallback((note: Note) => {
    setActiveNote(note);
    setEditTitle(note.title);
    setEditContent(note.content);
    setView("editor");
  }, []);

  const handleSave = useCallback(async () => {
    if (!activeNote) return;
    try {
      await updateNote(activeNote.id, editTitle || "Untitled", editContent);
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
      load();
    } catch (e) {
      console.error("Failed to save note:", e);
    }
  }, [activeNote, editTitle, editContent, load]);

  // Auto-save on content/title changes (debounced 1.5s)
  useEffect(() => {
    if (view !== "editor" || !activeNote) return;
    if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    autoSaveTimer.current = setTimeout(() => {
      handleSave();
    }, 1500);
    return () => {
      if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    };
  }, [editTitle, editContent]); // eslint-disable-line react-hooks/exhaustive-deps

  // When in editor, append transcription results directly into the note
  useEffect(() => {
    if (view !== "editor") return;
    const unlisten = onTranscriptionResult((text) => {
      setEditContent((prev) => {
        const separator = prev.length > 0 && !prev.endsWith("\n") && !prev.endsWith(" ") ? " " : "";
        return prev + separator + text;
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [view]);

  const handleBack = useCallback(async () => {
    if (autoSaveTimer.current) clearTimeout(autoSaveTimer.current);
    if (activeNote) {
      await updateNote(activeNote.id, editTitle || "Untitled", editContent).catch(() => {});
      load();
    }
    setView("grid");
    setActiveNote(null);
  }, [activeNote, editTitle, editContent, load]);

  const handleDelete = useCallback(
    (e: React.MouseEvent, id: string) => {
      e.stopPropagation();
      deleteNote(id)
        .then(() => {
          setNotes((prev) => prev.filter((n) => n.id !== id));
          if (activeNote?.id === id) {
            setView("grid");
            setActiveNote(null);
          }
        })
        .catch((e) => console.error("Failed to delete note:", e));
    },
    [activeNote]
  );

  const formatDate = (iso: string) => {
    const d = new Date(iso);
    const now = new Date();
    const diffMs = now.getTime() - d.getTime();
    const diffMins = Math.floor(diffMs / 60_000);
    if (diffMins < 1) return "Just now";
    if (diffMins < 60) return `${diffMins}m ago`;
    const diffHours = Math.floor(diffMins / 60);
    if (diffHours < 24) return `${diffHours}h ago`;
    const diffDays = Math.floor(diffHours / 24);
    if (diffDays < 7) return `${diffDays}d ago`;
    return d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  };

  /* ────────────────────────────────────────────────────────────
     Editor View
     ──────────────────────────────────────────────────────────── */
  if (view === "editor") {
    return (
      <div className="flex h-full flex-col">
        {/* Toolbar — slim, utility-focused */}
        <div className="flex items-center justify-between border-b border-border/55 px-8 py-3.5">
          <button
            onClick={handleBack}
            className="flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-medium uppercase tracking-[0.12em] text-text-muted transition-colors hover:bg-surface-2/60 hover:text-text-secondary"
          >
            <ArrowLeft size={14} strokeWidth={1.75} />
            Notes
          </button>

          {/* Save indicator */}
          <div className="flex items-center gap-1.5 text-xs text-text-muted">
            {saved ? (
              <>
                <Check size={12} className="text-success" />
                <span className="text-success/90">Saved</span>
              </>
            ) : (
              <span className="opacity-50">Auto-saves</span>
            )}
          </div>
        </div>

        {/* Document canvas */}
        <div className="flex-1 overflow-auto">
          <div className="mx-auto w-full max-w-[640px] px-8 py-12">
            {/* Title */}
            <input
              ref={titleRef}
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              placeholder="Untitled"
              className="w-full border-none bg-transparent font-display text-[2rem] font-semibold leading-tight tracking-[-0.022em] text-text-primary outline-none placeholder:text-text-muted/30"
            />

            {/* Subtle rule */}
            <div className="mb-7 mt-5 h-px w-12 rounded-full bg-amber-400/25" />

            {/* Content */}
            <textarea
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
              placeholder="Start writing…"
              className="min-h-[60vh] w-full resize-none border-none bg-transparent text-[15.5px] leading-[1.75] tracking-[0.005em] text-text-secondary outline-none placeholder:text-text-muted/30"
            />
          </div>
        </div>
      </div>
    );
  }

  /* ────────────────────────────────────────────────────────────
     Grid View
     ──────────────────────────────────────────────────────────── */
  return (
    <div className="flex h-full flex-col px-8 py-8">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            Notes
          </h1>
          <p className="mt-1 text-sm text-text-muted">
            {notes.length > 0
              ? `${notes.length} note${notes.length === 1 ? "" : "s"}`
              : "Your saved notes"}
          </p>
        </div>
        <button
          onClick={handleNew}
          className="flex h-8 items-center gap-1.5 rounded-lg border border-amber-400/30 bg-amber-500/[0.10] px-3 text-xs font-medium uppercase tracking-[0.10em] text-amber-300 transition-colors hover:border-amber-400/55 hover:bg-amber-500/[0.18]"
        >
          <Plus size={13} strokeWidth={2} />
          New
        </button>
      </div>

      {/* Grid */}
      <div className="mt-6 flex-1 overflow-auto pr-1">
        {loading && notes.length === 0 ? (
          <div className="flex h-48 items-center justify-center text-sm text-text-muted">
            Loading…
          </div>
        ) : notes.length === 0 ? (
          /* ── Empty state ── */
          <div className="flex h-64 flex-col items-center justify-center text-center">
            <div className="relative mb-5">
              <div className="absolute inset-0 -m-3 rounded-2xl border border-amber-500/15 bg-amber-500/[0.04]" />
              <StickyNote size={28} strokeWidth={1.5} className="relative text-text-muted/70" />
            </div>
            <p className="text-sm font-medium text-text-secondary">No notes yet</p>
            <p className="mb-4 mt-1 text-xs text-text-muted">
              Create a note to start writing
            </p>
            <button
              onClick={handleNew}
              className="text-xs font-medium tracking-wide text-amber-300 transition-colors hover:text-amber-200"
            >
              + Create note
            </button>
          </div>
        ) : (
          /* ── Note cards ── */
          <div className="grid grid-cols-2 gap-3 lg:grid-cols-3">
            {notes.map((note, i) => (
              <button
                key={note.id}
                onClick={() => handleOpen(note)}
                className="group relative flex flex-col rounded-xl border border-border bg-surface-1/80 text-left shadow-sm transition-all duration-200 hover:-translate-y-px hover:border-border-hover hover:bg-surface-1 hover:shadow-md"
                style={{
                  animation: `fade-in 0.35s cubic-bezier(0.22, 1, 0.36, 1) ${i * 0.03}s both`,
                }}
              >
                {/* Document preview area */}
                <div className="min-h-[104px] flex-1 px-4 pb-3 pt-4">
                  {/* Title */}
                  <h3 className="truncate text-[14px] font-medium leading-snug text-text-primary">
                    {note.title || "Untitled"}
                  </h3>

                  {/* Content preview */}
                  <p className="mt-1.5 line-clamp-4 text-xs leading-relaxed text-text-muted">
                    {note.content || "Empty note"}
                  </p>
                </div>

                {/* Footer — date + actions */}
                <div className="flex items-center justify-between border-t border-border/55 px-4 py-2.5">
                  <span className="text-[10.5px] tabular-nums tracking-wide text-text-muted/70">
                    {formatDate(note.updated_at)}
                  </span>

                  <div
                    role="button"
                    tabIndex={0}
                    onClick={(e) => handleDelete(e, note.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter")
                        handleDelete(e as unknown as React.MouseEvent, note.id);
                    }}
                    className="rounded-md p-1 text-text-muted opacity-0 transition-all hover:bg-recording-500/12 hover:text-recording-400 group-hover:opacity-100"
                  >
                    <Trash2 size={11} />
                  </div>
                </div>
              </button>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
