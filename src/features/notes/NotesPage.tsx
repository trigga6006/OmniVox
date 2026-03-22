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
        <div className="flex items-center justify-between px-6 py-3 border-b border-border/50">
          <button
            onClick={handleBack}
            className="flex items-center gap-1.5 text-xs tracking-wide uppercase text-text-muted hover:text-text-secondary transition-colors"
          >
            <ArrowLeft size={14} strokeWidth={1.5} />
            Notes
          </button>

          {/* Save indicator */}
          <div className="flex items-center gap-1.5 text-xs text-text-muted">
            {saved ? (
              <>
                <Check size={12} className="text-emerald-400" />
                <span className="text-emerald-400/80">Saved</span>
              </>
            ) : (
              <span className="opacity-40">Auto-saves</span>
            )}
          </div>
        </div>

        {/* Document canvas */}
        <div className="flex-1 overflow-auto">
          <div className="mx-auto w-full max-w-[640px] px-6 py-10">
            {/* Title */}
            <input
              ref={titleRef}
              value={editTitle}
              onChange={(e) => setEditTitle(e.target.value)}
              placeholder="Untitled"
              className="w-full bg-transparent font-display font-semibold text-3xl text-text-primary placeholder:text-text-muted/25 outline-none border-none leading-tight"
            />

            {/* Subtle rule */}
            <div className="mt-4 mb-6 h-px w-12 bg-amber-500/20 rounded-full" />

            {/* Content */}
            <textarea
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
              placeholder="Start writing…"
              className="w-full min-h-[60vh] bg-transparent text-[15px] text-text-secondary/90 placeholder:text-text-muted/20 outline-none border-none resize-none leading-[1.8] tracking-[0.005em]"
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
    <div className="flex h-full flex-col p-6">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="font-display font-semibold text-2xl text-text-primary">Notes</h1>
          <p className="text-sm text-text-muted mt-1">
            {notes.length > 0
              ? `${notes.length} note${notes.length === 1 ? "" : "s"}`
              : "Your saved notes"}
          </p>
        </div>
        <button
          onClick={handleNew}
          className="flex items-center gap-2 h-8 px-3 rounded-lg bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 text-xs font-medium tracking-wide uppercase transition-colors"
        >
          <Plus size={13} strokeWidth={2} />
          New
        </button>
      </div>

      {/* Grid */}
      <div className="mt-6 flex-1 overflow-auto pr-1">
        {loading && notes.length === 0 ? (
          <div className="flex items-center justify-center h-48 text-text-muted text-sm">
            Loading…
          </div>
        ) : notes.length === 0 ? (
          /* ── Empty state ── */
          <div className="flex flex-col items-center justify-center h-64 text-center">
            <div className="relative mb-5">
              <div className="absolute inset-0 -m-3 rounded-2xl bg-amber-500/[0.03] border border-amber-500/10" />
              <StickyNote size={28} strokeWidth={1.25} className="relative text-text-muted/60" />
            </div>
            <p className="text-sm text-text-secondary">No notes yet</p>
            <p className="text-xs text-text-muted mt-1 mb-4">
              Create a note to start writing
            </p>
            <button
              onClick={handleNew}
              className="text-xs text-amber-400 hover:text-amber-300 transition-colors tracking-wide"
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
                className="group relative flex flex-col text-left rounded-xl border border-border bg-surface-1 hover:bg-surface-2 hover:border-border-hover transition-all duration-200"
                style={{
                  animation: `fade-in 0.3s ease-out ${i * 0.04}s both`,
                }}
              >
                {/* Document preview area */}
                <div className="px-4 pt-4 pb-3 flex-1 min-h-[100px]">
                  {/* Title */}
                  <h3 className="text-sm font-medium text-text-primary truncate leading-snug">
                    {note.title || "Untitled"}
                  </h3>

                  {/* Content preview */}
                  <p className="mt-2 text-xs text-text-muted/70 line-clamp-4 leading-relaxed">
                    {note.content || "Empty note"}
                  </p>
                </div>

                {/* Footer — date + actions */}
                <div className="flex items-center justify-between px-4 py-2.5 border-t border-border/50">
                  <span className="text-[10px] text-text-muted/50 tracking-wide">
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
                    className="p-1 rounded opacity-0 group-hover:opacity-100 hover:bg-surface-3 text-text-muted hover:text-recording-400 transition-all"
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
