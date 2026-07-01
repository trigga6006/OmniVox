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
import { Button, Card, Input, Kbd, Segmented, EmptyState } from "@/components/ui";

const tabs = ["Vocabulary", "Words", "Snippets"] as const;
type Tab = (typeof tabs)[number];

const eyebrow =
  "font-mono text-[10px] font-semibold uppercase tracking-[0.2em] text-text-muted";

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
      <Card className="mt-4 flex flex-1 flex-col p-2">
        <EmptyState
          className="flex-1"
          icon={<Languages strokeWidth={1.5} />}
          title="No vocabulary words yet"
          description="Add words you commonly use so Whisper recognizes them correctly instead of guessing similar-sounding alternatives."
          action={
            <Button
              variant="primary"
              size="sm"
              icon={<Plus />}
              onClick={() => setAdding(true)}
            >
              Add first word
            </Button>
          }
        />
      </Card>
    );
  }

  return (
    <div className="mt-4 flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
      {entries.map((entry) => {
        const isEditing = editId === entry.id;

        if (isEditing) {
          return (
            <Card
              key={entry.id}
              className="flex items-center gap-2 border-amber-400/40 p-3 shadow-sm"
            >
              <Input
                value={editWord}
                onChange={(e) => setEditWord(e.target.value)}
                placeholder="Word or phrase…"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(entry.id)}
                autoFocus
              />
              <Button
                variant="ghost"
                size="sm"
                className="px-2 text-success"
                aria-label="Save"
                onClick={() => handleUpdate(entry.id)}
              >
                <Check />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="px-2"
                aria-label="Cancel"
                onClick={() => setEditId(null)}
              >
                <X />
              </Button>
            </Card>
          );
        }

        return (
          <Card
            key={entry.id}
            className="group flex items-center gap-3 bg-surface-1/80 px-4 py-3 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
          >
            <span className="flex-1 truncate text-sm font-medium text-text-primary">
              {entry.word}
            </span>
            <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
              <Button
                variant="ghost"
                size="sm"
                className="px-2"
                aria-label="Edit"
                onClick={() => startEdit(entry)}
              >
                <Pencil />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="px-2 text-text-muted hover:bg-recording-500/10 hover:text-recording-400"
                aria-label="Delete"
                onClick={() => handleDelete(entry.id)}
              >
                <Trash2 />
              </Button>
            </div>
          </Card>
        );
      })}

      {/* Inline add row */}
      {adding && (
        <Card className="flex items-center gap-2 border-amber-400/40 p-3 shadow-sm">
          <Input
            ref={wordRef}
            value={newWord}
            onChange={(e) => setNewWord(e.target.value)}
            placeholder="Word or phrase…"
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          />
          <Button
            variant="ghost"
            size="sm"
            className="px-2 text-success"
            aria-label="Save"
            onClick={handleAdd}
          >
            <Check />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="px-2"
            aria-label="Cancel"
            onClick={() => {
              setAdding(false);
              setNewWord("");
            }}
          >
            <X />
          </Button>
        </Card>
      )}

      {/* Add button */}
      {!adding && (
        <button
          onClick={() => setAdding(true)}
          className="flex items-center gap-2 rounded-2xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
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
      <Card className="mt-4 flex flex-1 flex-col p-2">
        <EmptyState
          className="flex-1"
          icon={<BookOpen strokeWidth={1.5} />}
          title="No word replacements yet"
          description="Add words that Whisper commonly mis-transcribes and map them to the correct spelling."
          action={
            <Button
              variant="primary"
              size="sm"
              icon={<Plus />}
              onClick={() => setAdding(true)}
            >
              Add first word
            </Button>
          }
        />
      </Card>
    );
  }

  return (
    <div className="mt-4 flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
      {/* Existing entries */}
      {entries.map((entry) => {
        const isEditing = editId === entry.id;

        if (isEditing) {
          return (
            <Card
              key={entry.id}
              className="flex items-center gap-2 border-amber-400/40 p-3 shadow-sm"
            >
              <Input
                value={editPhrase}
                onChange={(e) => setEditPhrase(e.target.value)}
                placeholder="Heard as…"
              />
              <ArrowRight size={14} className="shrink-0 text-text-muted" />
              <Input
                value={editReplacement}
                onChange={(e) => setEditReplacement(e.target.value)}
                placeholder="Replace with…"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(entry.id)}
              />
              <Button
                variant="ghost"
                size="sm"
                className="px-2 text-success"
                aria-label="Save"
                onClick={() => handleUpdate(entry.id)}
              >
                <Check />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="px-2"
                aria-label="Cancel"
                onClick={() => setEditId(null)}
              >
                <X />
              </Button>
            </Card>
          );
        }

        return (
          <Card
            key={entry.id}
            className="group flex items-center gap-3 bg-surface-1/80 px-4 py-3 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
          >
            <span className="flex-1 truncate text-sm font-medium text-text-secondary">
              {entry.phrase}
            </span>
            <ArrowRight size={12} className="shrink-0 text-text-muted" />
            <span className="flex-1 truncate text-sm text-amber-300/90">
              {entry.replacement}
            </span>
            <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
              <Button
                variant="ghost"
                size="sm"
                className="px-2"
                aria-label="Edit"
                onClick={() => startEdit(entry)}
              >
                <Pencil />
              </Button>
              <Button
                variant="ghost"
                size="sm"
                className="px-2 text-text-muted hover:bg-recording-500/10 hover:text-recording-400"
                aria-label="Delete"
                onClick={() => handleDelete(entry.id)}
              >
                <Trash2 />
              </Button>
            </div>
          </Card>
        );
      })}

      {/* Inline add row */}
      {adding && (
        <Card className="flex items-center gap-2 border-amber-400/40 p-3 shadow-sm">
          <Input
            ref={phraseRef}
            value={newPhrase}
            onChange={(e) => setNewPhrase(e.target.value)}
            placeholder="Heard as…"
          />
          <ArrowRight size={14} className="shrink-0 text-text-muted" />
          <Input
            value={newReplacement}
            onChange={(e) => setNewReplacement(e.target.value)}
            placeholder="Replace with…"
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          />
          <Button
            variant="ghost"
            size="sm"
            className="px-2 text-success"
            aria-label="Save"
            onClick={handleAdd}
          >
            <Check />
          </Button>
          <Button
            variant="ghost"
            size="sm"
            className="px-2"
            aria-label="Cancel"
            onClick={() => {
              setAdding(false);
              setNewPhrase("");
              setNewReplacement("");
            }}
          >
            <X />
          </Button>
        </Card>
      )}

      {/* Add button */}
      {!adding && (
        <button
          onClick={() => setAdding(true)}
          className="flex items-center gap-2 rounded-2xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
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
      <Card className="mt-4 flex flex-1 flex-col p-2">
        <EmptyState
          className="flex-1"
          icon={<FileText strokeWidth={1.5} />}
          title="No snippets yet"
          description="Create text shortcuts — say a trigger word and it expands to longer text."
          action={
            <Button
              variant="primary"
              size="sm"
              icon={<Plus />}
              onClick={() => setAdding(true)}
            >
              Add first snippet
            </Button>
          }
        />
      </Card>
    );
  }

  return (
    <div className="mt-4 flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
      {snippets.map((snippet) => {
        const isEditing = editId === snippet.id;

        if (isEditing) {
          return (
            <Card
              key={snippet.id}
              className="flex flex-col gap-2 border-amber-400/40 p-3 shadow-sm"
            >
              <div className="flex items-center gap-2">
                <Input
                  value={editTrigger}
                  onChange={(e) => setEditTrigger(e.target.value)}
                  className="w-32"
                  placeholder="Word…"
                />
                <ArrowRight size={14} className="shrink-0 text-text-muted" />
                <Input
                  value={editContent}
                  onChange={(e) => setEditContent(e.target.value)}
                  placeholder="Expands to…"
                />
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2 text-success"
                  aria-label="Save"
                  onClick={() => handleUpdate(snippet.id)}
                >
                  <Check />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2"
                  aria-label="Cancel"
                  onClick={() => setEditId(null)}
                >
                  <X />
                </Button>
              </div>
              <Input
                value={editDesc}
                onChange={(e) => setEditDesc(e.target.value)}
                placeholder="Description (optional)"
                onKeyDown={(e) => e.key === "Enter" && handleUpdate(snippet.id)}
              />
            </Card>
          );
        }

        return (
          <Card
            key={snippet.id}
            className="group bg-surface-1/80 px-4 py-3 transition-all duration-200 hover:border-border-hover hover:bg-surface-1"
          >
            <div className="flex items-center gap-3">
              <Kbd className="shrink-0 border-amber-400/25 bg-amber-500/[0.08] text-amber-300">
                {snippet.trigger}
              </Kbd>
              <ArrowRight size={12} className="shrink-0 text-text-muted" />
              <span className="flex-1 truncate text-sm text-text-primary">
                {snippet.content}
              </span>
              <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2"
                  aria-label="Edit"
                  onClick={() => startEdit(snippet)}
                >
                  <Pencil />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  className="px-2 text-text-muted hover:bg-recording-500/10 hover:text-recording-400"
                  aria-label="Delete"
                  onClick={() => handleDelete(snippet.id)}
                >
                  <Trash2 />
                </Button>
              </div>
            </div>
            {snippet.description && (
              <p className="ml-0.5 mt-1.5 text-xs text-text-muted">
                {snippet.description}
              </p>
            )}
          </Card>
        );
      })}

      {/* Inline add */}
      {adding && (
        <Card className="flex flex-col gap-2 border-amber-400/40 p-3 shadow-sm">
          <div className="flex items-center gap-2">
            <Input
              ref={triggerRef}
              value={newTrigger}
              onChange={(e) => setNewTrigger(e.target.value)}
              className="w-32"
              placeholder="Word…"
            />
            <ArrowRight size={14} className="shrink-0 text-text-muted" />
            <Input
              value={newContent}
              onChange={(e) => setNewContent(e.target.value)}
              placeholder="Expands to…"
            />
            <Button
              variant="ghost"
              size="sm"
              className="px-2 text-success"
              aria-label="Save"
              onClick={handleAdd}
            >
              <Check />
            </Button>
            <Button
              variant="ghost"
              size="sm"
              className="px-2"
              aria-label="Cancel"
              onClick={() => {
                setAdding(false);
                setNewTrigger("");
                setNewContent("");
                setNewDesc("");
              }}
            >
              <X />
            </Button>
          </div>
          <Input
            value={newDesc}
            onChange={(e) => setNewDesc(e.target.value)}
            placeholder="Description (optional)"
            onKeyDown={(e) => e.key === "Enter" && handleAdd()}
          />
        </Card>
      )}

      {!adding && (
        <button
          onClick={() => setAdding(true)}
          className="flex items-center gap-2 rounded-2xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
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
    <div className="flex h-full flex-col px-8 pt-6 pb-8">
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
      <div className="mt-5">
        <span className={eyebrow}>Library</span>
        <div className="mt-2.5">
          <Segmented
            options={tabs.map((tab) => ({ value: tab, label: tab }))}
            value={activeTab}
            onChange={setActiveTab}
          />
        </div>
      </div>

      {/* Tab content */}
      {activeTab === "Vocabulary" && <VocabularyTab />}
      {activeTab === "Words" && <WordsTab />}
      {activeTab === "Snippets" && <SnippetsTab />}
    </div>
  );
}
