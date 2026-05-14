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
  Languages,
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
  listVocabularyEntries,
  addVocabularyEntry,
  updateVocabularyEntry,
  deleteVocabularyEntry,
  type DictionaryEntry,
  type Snippet,
  type VocabularyEntry,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

const tabs = ["Vocabulary", "Words", "Snippets"] as const;
type Tab = (typeof tabs)[number];

/* ──────────────────────── Vocabulary Tab ──────────────────────── */

function VocabularyTab() {
  const [entries, setEntries] = useState<VocabularyEntry[]>([]);
  const [loading, setLoading] = useState(true);

  // Inline add
  const [adding, setAdding] = useState(false);
  const [newWord, setNewWord] = useState("");
  const wordRef = useRef<HTMLInputElement>(null);

  // Inline edit
  const [editId, setEditId] = useState<string | null>(null);
  const [editWord, setEditWord] = useState("");

  const load = useCallback(() => {
    setLoading(true);
    listVocabularyEntries()
      .then(setEntries)
      .catch((e) => console.error("Failed to load vocabulary:", e))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (adding) wordRef.current?.focus();
  }, [adding]);

  const handleAdd = useCallback(() => {
    const w = newWord.trim();
    if (!w) return;
    addVocabularyEntry(w)
      .then((entry) => {
        setEntries((prev) => [...prev, entry]);
        setNewWord("");
        setAdding(false);
      })
      .catch((e) => console.error("Failed to add vocabulary entry:", e));
  }, [newWord]);

  const handleUpdate = useCallback(
    (id: string) => {
      const w = editWord.trim();
      if (!w) return;
      updateVocabularyEntry(id, w)
        .then(() => {
          setEntries((prev) =>
            prev.map((e) => (e.id === id ? { ...e, word: w } : e))
          );
          setEditId(null);
        })
        .catch((e) => console.error("Failed to update vocabulary entry:", e));
    },
    [editWord]
  );

  const handleDelete = useCallback((id: string) => {
    deleteVocabularyEntry(id)
      .then(() => setEntries((prev) => prev.filter((e) => e.id !== id)))
      .catch((e) => console.error("Failed to delete vocabulary entry:", e));
  }, []);

  const startEdit = (entry: VocabularyEntry) => {
    setEditId(entry.id);
    setEditWord(entry.word);
  };

  if (!loading && entries.length === 0 && !adding) {
    return (
      <div className="flex flex-1 flex-col items-center justify-center gap-4">
        <div className="relative flex items-center justify-center">
          <div className="absolute h-20 w-20 rounded-full border border-amber-500/15 bg-amber-500/[0.05]" />
          <Languages size={40} strokeWidth={1.5} className="relative text-text-muted" />
        </div>
        <div className="text-center mt-2">
          <p className="text-sm font-medium text-text-secondary">
            No vocabulary words yet
          </p>
          <p className="text-xs text-text-muted mt-1 max-w-xs">
            Add words you commonly use so Whisper recognizes them correctly instead of guessing similar-sounding alternatives
          </p>
        </div>
        <button
          onClick={() => setAdding(true)}
          className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-amber-400/30 bg-amber-500/[0.10] px-4 py-2 text-sm font-medium text-amber-300 transition-colors hover:border-amber-400/50 hover:bg-amber-500/[0.16]"
        >
          <Plus size={14} strokeWidth={2} />
          Add first word
        </button>
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2 flex-1 overflow-y-auto pr-1 mt-4">
      {entries.map((entry) => {
        const isEditing = editId === entry.id;

        if (isEditing) {
          return (
            <div
              key={entry.id}
              className="flex items-center gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm"
            >
              <input
                value={editWord}
                onChange={(e) => setEditWord(e.target.value)}
                className="flex-1 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-text-primary outline-none transition-colors focus:border-amber-400/45 focus:bg-surface-1"
                placeholder="Word or phrase…"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(entry.id)}
                autoFocus
              />
              <button
                onClick={() => handleUpdate(entry.id)}
                className="rounded-md p-1.5 text-success transition-colors hover:bg-surface-3"
              >
                <Check size={14} />
              </button>
              <button
                onClick={() => setEditId(null)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-3"
              >
                <X size={14} />
              </button>
            </div>
          );
        }

        return (
          <div
            key={entry.id}
            className="group flex items-center gap-3 rounded-xl border border-border bg-surface-1/80 px-4 py-3 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
          >
            <span className="text-sm text-text-primary font-medium flex-1 truncate">
              {entry.word}
            </span>
            <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                onClick={() => startEdit(entry)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
              >
                <Pencil size={13} />
              </button>
              <button
                onClick={() => handleDelete(entry.id)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
              >
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        );
      })}

      {/* Inline add row */}
      {adding && (
        <div className="flex items-center gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm">
          <input
            ref={wordRef}
            value={newWord}
            onChange={(e) => setNewWord(e.target.value)}
            className="flex-1 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
            placeholder="Word or phrase…"
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
              setNewWord("");
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
          className="flex items-center gap-2 rounded-xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
        >
          <Plus size={14} strokeWidth={2} />
          Add vocabulary word
        </button>
      )}
    </div>
  );
}

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
          <div className="absolute h-20 w-20 rounded-full border border-amber-500/15 bg-amber-500/[0.05]" />
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
          className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-amber-400/30 bg-amber-500/[0.10] px-4 py-2 text-sm font-medium text-amber-300 transition-colors hover:border-amber-400/50 hover:bg-amber-500/[0.16]"
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
              className="flex items-center gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm"
            >
              <input
                value={editPhrase}
                onChange={(e) => setEditPhrase(e.target.value)}
                className="flex-1 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-text-primary outline-none transition-colors focus:border-amber-400/45 focus:bg-surface-1"
                placeholder="Heard as…"
              />
              <ArrowRight size={14} className="text-text-muted shrink-0" />
              <input
                value={editReplacement}
                onChange={(e) => setEditReplacement(e.target.value)}
                className="flex-1 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-text-primary outline-none transition-colors focus:border-amber-400/45 focus:bg-surface-1"
                placeholder="Replace with…"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(entry.id)}
              />
              <button
                onClick={() => handleUpdate(entry.id)}
                className="rounded-md p-1.5 text-success transition-colors hover:bg-surface-3"
              >
                <Check size={14} />
              </button>
              <button
                onClick={() => setEditId(null)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-3"
              >
                <X size={14} />
              </button>
            </div>
          );
        }

        return (
          <div
            key={entry.id}
            className="group flex items-center gap-3 rounded-xl border border-border bg-surface-1/80 px-4 py-3 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
          >
            <span className="text-sm text-text-secondary font-medium flex-1 truncate">
              {entry.phrase}
            </span>
            <ArrowRight size={12} className="text-text-muted shrink-0" />
            <span className="flex-1 truncate text-sm text-amber-300/90">
              {entry.replacement}
            </span>
            <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
              <button
                onClick={() => startEdit(entry)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
              >
                <Pencil size={13} />
              </button>
              <button
                onClick={() => handleDelete(entry.id)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
              >
                <Trash2 size={13} />
              </button>
            </div>
          </div>
        );
      })}

      {/* Inline add row */}
      {adding && (
        <div className="flex items-center gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm">
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
          className="flex items-center gap-2 rounded-xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
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
          <div className="absolute h-20 w-20 rounded-full border border-amber-500/15 bg-amber-500/[0.05]" />
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
          className="mt-2 inline-flex items-center gap-1.5 rounded-lg border border-amber-400/30 bg-amber-500/[0.10] px-4 py-2 text-sm font-medium text-amber-300 transition-colors hover:border-amber-400/50 hover:bg-amber-500/[0.16]"
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
              className="flex flex-col gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm"
            >
              <div className="flex items-center gap-2">
                <input
                  value={editTrigger}
                  onChange={(e) => setEditTrigger(e.target.value)}
                  className="w-32 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-text-primary outline-none transition-colors focus:border-amber-400/45 focus:bg-surface-1"
                  placeholder="Word…"
                />
                <ArrowRight size={14} className="text-text-muted shrink-0" />
                <input
                  value={editContent}
                  onChange={(e) => setEditContent(e.target.value)}
                  className="flex-1 rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-sm text-text-primary outline-none transition-colors focus:border-amber-400/45 focus:bg-surface-1"
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
                className="rounded-lg border border-border bg-surface-2 px-3 py-1.5 text-xs text-text-secondary outline-none transition-colors focus:border-amber-400/45 focus:bg-surface-1"
                placeholder="Description (optional)"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(snippet.id)}
              />
            </div>
          );
        }

        return (
          <div
            key={snippet.id}
            className="group rounded-xl border border-border bg-surface-1/80 px-4 py-3 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
          >
            <div className="flex items-center gap-3">
              <kbd className="shrink-0 rounded-md border border-amber-400/25 bg-amber-500/[0.08] px-2 py-0.5 font-mono text-xs text-amber-300">
                {snippet.trigger}
              </kbd>
              <ArrowRight size={12} className="text-text-muted shrink-0" />
              <span className="text-sm text-text-primary flex-1 truncate">
                {snippet.content}
              </span>
              <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                <button
                  onClick={() => startEdit(snippet)}
                  className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
                >
                  <Pencil size={13} />
                </button>
                <button
                  onClick={() => handleDelete(snippet.id)}
                  className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
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
        <div className="flex flex-col gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm">
          <div className="flex items-center gap-2">
            <input
              ref={triggerRef}
              value={newTrigger}
              onChange={(e) => setNewTrigger(e.target.value)}
              className="w-32 bg-surface-2 rounded-md px-2.5 py-1.5 text-sm text-text-primary border border-border outline-none focus:border-amber-500/40"
              placeholder="Word…"
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
          className="flex items-center gap-2 rounded-xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
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
  const [activeTab, setActiveTab] = useState<Tab>("Vocabulary");

  return (
    <div className="flex h-full flex-col px-8 py-8">
      {/* Header */}
      <div>
        <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
          Dictionary
        </h1>
        <p className="mt-1 text-sm text-text-muted">
          Custom vocabulary & text snippets
        </p>
      </div>

      {/* Tabs */}
      <div className="mt-6 flex gap-6 border-b border-border/70">
        {tabs.map((tab) => {
          const isActive = activeTab === tab;
          return (
            <button
              key={tab}
              onClick={() => setActiveTab(tab)}
              className={cn(
                "relative pb-3 text-sm font-medium transition-colors",
                isActive
                  ? "text-amber-300"
                  : "text-text-muted hover:text-text-secondary"
              )}
            >
              {tab}
              {isActive && (
                <span className="absolute bottom-[-1px] left-0 right-0 h-[2px] rounded-full bg-amber-400" />
              )}
            </button>
          );
        })}
      </div>

      {/* Tab content */}
      {activeTab === "Vocabulary" && <VocabularyTab />}
      {activeTab === "Words" && <WordsTab />}
      {activeTab === "Snippets" && <SnippetsTab />}
    </div>
  );
}
