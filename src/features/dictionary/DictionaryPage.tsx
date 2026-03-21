import { useState, useEffect, useCallback, useRef } from "react";
import {
  BookOpen,
  Plus,
  Trash2,
  Pencil,
  Check,
  X,
  ArrowRight,
  FileText,
} from "lucide-react";
import {
  listDictionaryEntries,
  addDictionaryEntry,
  updateDictionaryEntry,
  deleteDictionaryEntry,
  listSnippets,
  addSnippet,
  updateSnippet,
  deleteSnippet,
  type DictionaryEntry,
  type Snippet,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

const tabs = ["Words", "Snippets"] as const;
type Tab = (typeof tabs)[number];

/* ────────────────────────── Words Tab ────────────────────────── */

function WordsTab() {
  const [entries, setEntries] = useState<DictionaryEntry[]>([]);
  const [loading, setLoading] = useState(true);

  // Inline add
  const [adding, setAdding] = useState(false);
  const [newPhrase, setNewPhrase] = useState("");
  const [newReplacement, setNewReplacement] = useState("");
  const phraseRef = useRef<HTMLInputElement>(null);

  // Inline edit
  const [editId, setEditId] = useState<string | null>(null);
  const [editPhrase, setEditPhrase] = useState("");
  const [editReplacement, setEditReplacement] = useState("");

  const load = useCallback(() => {
    setLoading(true);
    listDictionaryEntries()
      .then(setEntries)
      .catch((e) => console.error("Failed to load dictionary:", e))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  // Focus the phrase input when add mode opens
  useEffect(() => {
    if (adding) phraseRef.current?.focus();
  }, [adding]);

  const handleAdd = useCallback(() => {
    const p = newPhrase.trim();
    const r = newReplacement.trim();
    if (!p || !r) return;
    addDictionaryEntry(p, r)
      .then((entry) => {
        setEntries((prev) => [...prev, entry]);
        setNewPhrase("");
        setNewReplacement("");
        setAdding(false);
      })
      .catch((e) => console.error("Failed to add entry:", e));
  }, [newPhrase, newReplacement]);

  const handleUpdate = useCallback(
    (id: string) => {
      const p = editPhrase.trim();
      const r = editReplacement.trim();
      if (!p || !r) return;
      updateDictionaryEntry(id, p, r)
        .then(() => {
          setEntries((prev) =>
            prev.map((e) =>
              e.id === id ? { ...e, phrase: p, replacement: r } : e
            )
          );
          setEditId(null);
        })
        .catch((e) => console.error("Failed to update entry:", e));
    },
    [editPhrase, editReplacement]
  );

  const handleDelete = useCallback((id: string) => {
    deleteDictionaryEntry(id)
      .then(() => setEntries((prev) => prev.filter((e) => e.id !== id)))
      .catch((e) => console.error("Failed to delete entry:", e));
  }, []);

  const startEdit = (entry: DictionaryEntry) => {
    setEditId(entry.id);
    setEditPhrase(entry.phrase);
    setEditReplacement(entry.replacement);
  };

  if (!loading && entries.length === 0 && !adding) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4">
        <div className="relative flex items-center justify-center">
          <div className="absolute h-20 w-20 rounded-full bg-amber-500/5 border border-amber-700/20" />
          <BookOpen size={40} strokeWidth={1.5} className="relative text-text-muted" />
        </div>
        <div className="text-center mt-2">
          <p className="text-sm font-medium text-text-secondary">
            No word replacements yet
          </p>
          <p className="text-xs text-text-muted mt-1 max-w-xs">
            Add words that Whisper commonly mis-transcribes and map them to the correct spelling
          </p>
        </div>
        <button
          onClick={() => setAdding(true)}
          className="mt-2 inline-flex items-center gap-1.5 rounded-lg bg-amber-500/10 border border-amber-500/25 px-4 py-2 text-sm font-medium text-amber-400 hover:bg-amber-500/15 transition-colors"
        >
          <Plus size={14} strokeWidth={2} />
          Add first word
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2 flex-1 overflow-y-auto pr-1 mt-4">
      {/* Existing entries */}
      {entries.map((entry) => {
        const isEditing = editId === entry.id;

        if (isEditing) {
          return (
            <div
              key={entry.id}
              className="bg-surface-1 rounded-lg border border-amber-500/30 p-3 flex items-center gap-2"
            >
              <input
                value={editPhrase}
                onChange={(e) => setEditPhrase(e.target.value)}
                className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
                placeholder="Heard as…"
              />
              <ArrowRight size={14} className="text-text-muted shrink-0" />
              <input
                value={editReplacement}
                onChange={(e) => setEditReplacement(e.target.value)}
                className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
                placeholder="Replace with…"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(entry.id)}
              />
              <button
                onClick={() => handleUpdate(entry.id)}
                className="p-1.5 rounded-md hover:bg-surface-3 text-green-400 transition-colors"
              >
                <Check size={14} />
              </button>
              <button
                onClick={() => setEditId(null)}
                className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted transition-colors"
              >
                <X size={14} />
              </button>
            </div>
          );
        }

        return (
          <div
            key={entry.id}
            className="group bg-surface-1 rounded-lg border border-border hover:border-border-hover p-3 flex items-center gap-3 transition-colors"
          >
            <span className="text-sm text-text-secondary font-medium flex-1 truncate">
              {entry.phrase}
            </span>
            <ArrowRight size={12} className="text-text-muted shrink-0" />
            <span className="text-sm text-amber-400/80 flex-1 truncate">
              {entry.replacement}
            </span>
            <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                onClick={() => startEdit(entry)}
                className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted hover:text-text-secondary transition-colors"
              >
                <Pencil size={13} />
              </button>
              <button
                onClick={() => handleDelete(entry.id)}
                className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted hover:text-recording-400 transition-colors"
              >
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        );
      })}

      {/* Inline add row */}
      {adding && (
        <div className="bg-surface-1 rounded-lg border border-amber-500/30 p-3 flex items-center gap-2">
          <input
            ref={phraseRef}
            value={newPhrase}
            onChange={(e) => setNewPhrase(e.target.value)}
            className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
            placeholder="Heard as…"
          />
          <ArrowRight size={14} className="text-text-muted shrink-0" />
          <input
            value={newReplacement}
            onChange={(e) => setNewReplacement(e.target.value)}
            className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
            placeholder="Replace with…"
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          />
          <button
            onClick={handleAdd}
            className="p-1.5 rounded-md hover:bg-surface-3 text-green-400 transition-colors"
          >
            <Check size={14} />
          </button>
          <button
            onClick={() => {
              setAdding(false);
              setNewPhrase("");
              setNewReplacement("");
            }}
            className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted transition-colors"
          >
            <X size={14} />
          </button>
        </div>
      )}

      {/* Add button */}
      {!adding && (
        <button
          onClick={() => setAdding(true)}
          className="flex items-center gap-2 rounded-lg border border-dashed border-border hover:border-amber-500/30 p-3 text-sm text-text-muted hover:text-amber-400 transition-colors"
        >
          <Plus size={14} strokeWidth={2} />
          Add word replacement
        </button>
      )}
    </div>
  );
}

/* ────────────────────────── Snippets Tab ─────────────────────── */

function SnippetsTab() {
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [loading, setLoading] = useState(true);

  // Inline add
  const [adding, setAdding] = useState(false);
  const [newTrigger, setNewTrigger] = useState("");
  const [newContent, setNewContent] = useState("");
  const [newDesc, setNewDesc] = useState("");
  const triggerRef = useRef<HTMLInputElement>(null);

  // Inline edit
  const [editId, setEditId] = useState<string | null>(null);
  const [editTrigger, setEditTrigger] = useState("");
  const [editContent, setEditContent] = useState("");
  const [editDesc, setEditDesc] = useState("");

  const load = useCallback(() => {
    setLoading(true);
    listSnippets()
      .then(setSnippets)
      .catch((e) => console.error("Failed to load snippets:", e))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (adding) triggerRef.current?.focus();
  }, [adding]);

  const handleAdd = useCallback(() => {
    const t = newTrigger.trim();
    const c = newContent.trim();
    if (!t || !c) return;
    addSnippet(t, c, newDesc.trim() || undefined)
      .then((snippet) => {
        setSnippets((prev) => [...prev, snippet]);
        setNewTrigger("");
        setNewContent("");
        setNewDesc("");
        setAdding(false);
      })
      .catch((e) => console.error("Failed to add snippet:", e));
  }, [newTrigger, newContent, newDesc]);

  const handleUpdate = useCallback(
    (id: string) => {
      const t = editTrigger.trim();
      const c = editContent.trim();
      if (!t || !c) return;
      updateSnippet(id, t, c, editDesc.trim() || undefined)
        .then(() => {
          setSnippets((prev) =>
            prev.map((s) =>
              s.id === id
                ? {
                    ...s,
                    trigger: t,
                    content: c,
                    description: editDesc.trim() || null,
                  }
                : s
            )
          );
          setEditId(null);
        })
        .catch((e) => console.error("Failed to update snippet:", e));
    },
    [editTrigger, editContent, editDesc]
  );

  const handleDelete = useCallback((id: string) => {
    deleteSnippet(id)
      .then(() => setSnippets((prev) => prev.filter((s) => s.id !== id)))
      .catch((e) => console.error("Failed to delete snippet:", e));
  }, []);

  const startEdit = (snippet: Snippet) => {
    setEditId(snippet.id);
    setEditTrigger(snippet.trigger);
    setEditContent(snippet.content);
    setEditDesc(snippet.description ?? "");
  };

  if (!loading && snippets.length === 0 && !adding) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4">
        <div className="relative flex items-center justify-center">
          <div className="absolute h-20 w-20 rounded-full bg-amber-500/5 border border-amber-700/20" />
          <FileText size={40} strokeWidth={1.5} className="relative text-text-muted" />
        </div>
        <div className="text-center mt-2">
          <p className="text-sm font-medium text-text-secondary">
            No snippets yet
          </p>
          <p className="text-xs text-text-muted mt-1 max-w-xs">
            Create text shortcuts — say a trigger word and it expands to longer text
          </p>
        </div>
        <button
          onClick={() => setAdding(true)}
          className="mt-2 inline-flex items-center gap-1.5 rounded-lg bg-amber-500/10 border border-amber-500/25 px-4 py-2 text-sm font-medium text-amber-400 hover:bg-amber-500/15 transition-colors"
        >
          <Plus size={14} strokeWidth={2} />
          Add first snippet
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2 flex-1 overflow-y-auto pr-1 mt-4">
      {snippets.map((snippet) => {
        const isEditing = editId === snippet.id;

        if (isEditing) {
          return (
            <div
              key={snippet.id}
              className="bg-surface-1 rounded-lg border border-amber-500/30 p-3 flex flex-col gap-2"
            >
              <div className="flex items-center gap-2">
                <input
                  value={editTrigger}
                  onChange={(e) => setEditTrigger(e.target.value)}
                  className="w-32 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
                  placeholder="Trigger…"
                />
                <ArrowRight size={14} className="text-text-muted shrink-0" />
                <input
                  value={editContent}
                  onChange={(e) => setEditContent(e.target.value)}
                  className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
                  placeholder="Expands to…"
                />
                <button
                  onClick={() => handleUpdate(snippet.id)}
                  className="p-1.5 rounded-md hover:bg-surface-3 text-green-400 transition-colors"
                >
                  <Check size={14} />
                </button>
                <button
                  onClick={() => setEditId(null)}
                  className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted transition-colors"
                >
                  <X size={14} />
                </button>
              </div>
              <input
                value={editDesc}
                onChange={(e) => setEditDesc(e.target.value)}
                className="bg-surface-2 rounded-md px-2.5 py-1.5 text-xs text-text-muted border border-border outline-none focus:border-amber-500/40"
                placeholder="Description (optional)"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(snippet.id)}
              />
            </div>
          );
        }

        return (
          <div
            key={snippet.id}
            className="group bg-surface-1 rounded-lg border border-border hover:border-border-hover p-3 transition-colors"
          >
            <div className="flex items-center gap-3">
              <kbd className="bg-surface-3 rounded-md px-2 py-0.5 font-mono text-xs text-amber-400 border border-border shrink-0">
                {snippet.trigger}
              </kbd>
              <ArrowRight size={12} className="text-text-muted shrink-0" />
              <span className="text-sm text-text-primary flex-1 truncate">
                {snippet.content}
              </span>
              <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                <button
                  onClick={() => startEdit(snippet)}
                  className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted hover:text-text-secondary transition-colors"
                >
                  <Pencil size={13} />
                </button>
                <button
                  onClick={() => handleDelete(snippet.id)}
                  className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted hover:text-recording-400 transition-colors"
                >
                  <Trash2 size={13} />
                </button>
              </div>
            </div>
            {snippet.description && (
              <p className="text-xs text-text-muted mt-1.5 ml-0.5">
                {snippet.description}
              </p>
            )}
          </div>
        );
      })}

      {/* Inline add */}
      {adding && (
        <div className="bg-surface-1 rounded-lg border border-amber-500/30 p-3 flex flex-col gap-2">
          <div className="flex items-center gap-2">
            <input
              ref={triggerRef}
              value={newTrigger}
              onChange={(e) => setNewTrigger(e.target.value)}
              className="w-32 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
              placeholder="Trigger…"
            />
            <ArrowRight size={14} className="text-text-muted shrink-0" />
            <input
              value={newContent}
              onChange={(e) => setNewContent(e.target.value)}
              className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
              placeholder="Expands to…"
            />
            <button
              onClick={handleAdd}
              className="p-1.5 rounded-md hover:bg-surface-3 text-green-400 transition-colors"
            >
              <Check size={14} />
            </button>
            <button
              onClick={() => {
                setAdding(false);
                setNewTrigger("");
                setNewContent("");
                setNewDesc("");
              }}
              className="p-1.5 rounded-md hover:bg-surface-3 text-text-muted transition-colors"
            >
              <X size={14} />
            </button>
          </div>
          <input
            value={newDesc}
            onChange={(e) => setNewDesc(e.target.value)}
            className="bg-surface-2 rounded-md px-2.5 py-1.5 text-xs text-text-muted border border-border outline-none focus:border-amber-500/40"
            placeholder="Description (optional)"
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          />
        </div>
      )}

      {!adding && (
        <button
          onClick={() => setAdding(true)}
          className="flex items-center gap-2 rounded-lg border border-dashed border-border hover:border-amber-500/30 p-3 text-sm text-text-muted hover:text-amber-400 transition-colors"
        >
          <Plus size={14} strokeWidth={2} />
          Add snippet
        </button>
      )}
    </div>
  );
}

/* ────────────────────────── Main Page ────────────────────────── */

export function DictionaryPage() {
  const [activeTab, setActiveTab] = useState<Tab>("Words");

  return (
    <div className="flex h-full flex-col p-6">
      {/* Header */}
      <div>
        <h1 className="font-display text-2xl text-text-primary">Dictionary</h1>
        <p className="text-sm text-text-muted mt-1">
          Custom vocabulary & text snippets
        </p>
      </div>

      {/* Tabs */}
      <div className="mt-5 flex gap-6 border-b border-border">
        {tabs.map((tab) => {
          const isActive = activeTab === tab;
          return (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={cn(
                "relative pb-2.5 text-sm font-medium transition-colors",
                isActive
                  ? "text-amber-400"
                  : "text-text-muted hover:text-text-secondary"
              )}
            >
              {tab}
              {isActive && (
                <span className="absolute bottom-0 left-0 right-0 h-[2px] rounded-full bg-amber-500" />
              )}
            </button>
          );
        })}
      </div>

      {/* Tab content */}
      {activeTab === "Words" ? <WordsTab /> : <SnippetsTab />}
    </div>
  );
}
