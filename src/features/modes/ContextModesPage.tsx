import { useState, useEffect, useCallback } from "react";
import {
  Plus,
  Trash2,
  Pencil,
  Check,
  X,
  Mic,
  Code,
  Mail,
  FileText,
  GraduationCap,
  Briefcase,
  MessageSquare,
  BookOpen,
  Terminal,
  PenTool,
  Globe,
  Heart,
  Scale,
} from "lucide-react";
import {
  listContextModes,
  createContextMode,
  updateContextMode,
  deleteContextMode,
  setActiveContextMode,
  getActiveContextMode,
  onContextModeChanged,
  listModeDictionaryEntries,
  addModeDictionaryEntry,
  deleteModeDictionaryEntry,
  listModeSnippets,
  addModeSnippet,
  deleteModeSnippet,
  listAppBindings,
  addAppBinding,
  deleteAppBinding,
  type ContextMode,
  type DictionaryEntry,
  type Snippet,
  type AppBinding,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

const ICON_OPTIONS = [
  { name: "mic", Icon: Mic },
  { name: "code", Icon: Code },
  { name: "mail", Icon: Mail },
  { name: "file-text", Icon: FileText },
  { name: "graduation-cap", Icon: GraduationCap },
  { name: "briefcase", Icon: Briefcase },
  { name: "message-square", Icon: MessageSquare },
  { name: "book-open", Icon: BookOpen },
  { name: "terminal", Icon: Terminal },
  { name: "pen-tool", Icon: PenTool },
  { name: "globe", Icon: Globe },
  { name: "heart", Icon: Heart },
  { name: "scale", Icon: Scale },
] as const;

const COLOR_OPTIONS = [
  { name: "amber", class: "bg-amber-500" },
  { name: "blue", class: "bg-blue-500" },
  { name: "green", class: "bg-emerald-500" },
  { name: "purple", class: "bg-purple-500" },
  { name: "red", class: "bg-red-500" },
  { name: "cyan", class: "bg-cyan-500" },
] as const;

function getIconComponent(iconName: string) {
  return ICON_OPTIONS.find((o) => o.name === iconName)?.Icon ?? Mic;
}

function getColorClass(colorName: string) {
  return COLOR_OPTIONS.find((o) => o.name === colorName)?.class ?? "bg-amber-500";
}

export const DEFAULT_PROMPT = `You are a dictation cleanup assistant. /no_think
Clean the following transcribed speech:
- Remove filler words (um, uh, like, you know, so, basically, actually)
- Fix grammar, spelling, and punctuation
- Handle self-corrections (keep the intended word, remove false starts)
- Preserve the speaker's intended meaning exactly
- Do not add information or change meaning
Output ONLY the cleaned text, nothing else. No commentary, no tags, no explanation.`;

/* ──────────────────── Main Page ──────────────────── */

export function ContextModesPage() {
  const [modes, setModes] = useState<ContextMode[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState<ContextMode | null>(null);
  const [creating, setCreating] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [m, active] = await Promise.all([
        listContextModes(),
        getActiveContextMode(),
      ]);
      setModes(m);
      setActiveId(active?.id ?? null);
    } catch (e) {
      console.error("Failed to load modes:", e);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    load();

    // Listen for mode changes from other windows (e.g. the overlay pill)
    const unlisten = onContextModeChanged((payload) => {
      setActiveId(payload.id);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load]);

  const handleActivate = async (id: string) => {
    try {
      await setActiveContextMode(id);
      setActiveId(id);
    } catch (e) {
      console.error("Failed to activate mode:", e);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteContextMode(id);
      load();
    } catch (e) {
      console.error("Failed to delete mode:", e);
    }
  };

  if (editing || creating) {
    return (
      <ModeForm
        mode={editing}
        onSave={async (createdMode?: ContextMode) => {
          if (createdMode) {
            // New mode was just created — switch into edit mode so
            // dictionary, snippets, and app bindings are available.
            setCreating(false);
            setEditing(createdMode);
          } else {
            setEditing(null);
            setCreating(false);
          }
          load();
        }}
        onCancel={() => {
          setEditing(null);
          setCreating(false);
        }}
      />
    );
  }

  return (
    <div className="mx-auto max-w-3xl px-8 py-10">
      {/* Header */}
      <div className="mb-8 flex items-start justify-between gap-4">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            Context Modes
          </h1>
          <p className="mt-1 text-sm text-text-muted">
            Switch between profiles that customize writing style, dictionary entries,
            snippets, and app bindings.
          </p>
        </div>
        <button
          onClick={() => setCreating(true)}
          className="mt-1 inline-flex shrink-0 items-center gap-1.5 rounded-lg border border-amber-400/30 bg-amber-500/[0.10] px-3 py-1.5 text-sm font-medium text-amber-300 transition-colors hover:border-amber-400/55 hover:bg-amber-500/[0.18]"
        >
          <Plus size={14} />
          New Mode
        </button>
      </div>

      {/* Mode Cards */}
      {loading ? (
        <div className="py-12 text-center text-sm text-text-muted">Loading…</div>
      ) : (
        <div className="grid gap-2.5">
          {modes.map((mode, i) => {
            const Icon = getIconComponent(mode.icon);
            const colorCls = getColorClass(mode.color);
            const isActive = mode.id === activeId;

            return (
              <div
                key={mode.id}
                className={cn(
                  "rounded-xl border bg-surface-1/85 p-4 opacity-0 transition-all duration-200 hover:bg-surface-1 animate-slide-up",
                  isActive
                    ? "border-amber-400/35 shadow-[0_0_0_1px_rgb(232_180_95_/_0.06)]"
                    : "border-border hover:border-border-hover"
                )}
                style={{
                  animationDelay: `${i * 0.04}s`,
                  animationFillMode: "forwards",
                }}
              >
                <div className="flex items-center gap-3">
                  {/* Icon */}
                  <div
                    className={cn(
                      "flex h-10 w-10 items-center justify-center rounded-xl",
                      colorCls + "/15"
                    )}
                  >
                    <Icon size={17} className={colorCls.replace("bg-", "text-")} />
                  </div>

                  {/* Info */}
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate text-[14px] font-medium text-text-primary">
                        {mode.name}
                      </span>
                      {isActive && (
                        <span className="rounded-md border border-amber-400/25 bg-amber-500/[0.10] px-1.5 py-0.5 text-[9.5px] font-semibold uppercase tracking-[0.10em] text-amber-300">
                          Active
                        </span>
                      )}
                      {mode.is_builtin && (
                        <span className="text-[10.5px] text-text-muted">Built-in</span>
                      )}
                    </div>
                    <p className="mt-0.5 truncate text-xs text-text-muted">
                      {mode.description}
                    </p>
                  </div>

                  {/* Actions */}
                  <div className="flex items-center gap-1.5">
                    {!isActive && (
                      <button
                        onClick={() => handleActivate(mode.id)}
                        className="inline-flex items-center gap-1 rounded-md border border-transparent px-2.5 py-1 text-xs font-medium text-text-secondary transition-colors hover:border-amber-400/25 hover:bg-amber-500/[0.10] hover:text-amber-300"
                      >
                        <Check size={12} />
                        Activate
                      </button>
                    )}
                    <button
                      onClick={() => setEditing(mode)}
                      className="flex h-7 w-7 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
                      title="Edit"
                    >
                      <Pencil size={13} />
                    </button>
                    {!mode.is_builtin && (
                      <button
                        onClick={() => handleDelete(mode.id)}
                        className="flex h-7 w-7 items-center justify-center rounded-md text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
                        title="Delete"
                      >
                        <Trash2 size={13} />
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

/* ──────────────────── Mode Form ──────────────────── */

function ModeForm({
  mode,
  onSave,
  onCancel,
}: {
  mode: ContextMode | null;
  onSave: (createdMode?: ContextMode) => void;
  onCancel: () => void;
}) {
  const isEdit = mode !== null;

  const [name, setName] = useState(mode?.name ?? "");
  const [description, setDescription] = useState(mode?.description ?? "");
  const [icon, setIcon] = useState(mode?.icon ?? "mic");
  const [color, setColor] = useState(mode?.color ?? "amber");
  const [writingStyle, setWritingStyle] = useState(mode?.writing_style ?? "formal");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Mode-scoped dictionary entries & snippets (only when editing)
  const [dictEntries, setDictEntries] = useState<DictionaryEntry[]>([]);
  const [modeSnippets, setModeSnippets] = useState<Snippet[]>([]);
  const [newPhrase, setNewPhrase] = useState("");
  const [newReplacement, setNewReplacement] = useState("");
  const [newTrigger, setNewTrigger] = useState("");
  const [newContent, setNewContent] = useState("");
  const [bindings, setBindings] = useState<AppBinding[]>([]);
  const [newProcessName, setNewProcessName] = useState("");

  useEffect(() => {
    if (!mode) return;
    listModeDictionaryEntries(mode.id).then(setDictEntries).catch(() => {});
    listModeSnippets(mode.id).then(setModeSnippets).catch(() => {});
    listAppBindings(mode.id).then(setBindings).catch((e) => console.error("Failed to load app bindings:", e));
  }, [mode?.id]);

  const handleAddDictEntry = async () => {
    if (!mode || !newPhrase.trim() || !newReplacement.trim()) return;
    try {
      const entry = await addModeDictionaryEntry(mode.id, newPhrase.trim(), newReplacement.trim());
      setDictEntries((prev) => [...prev, entry]);
      setNewPhrase("");
      setNewReplacement("");
    } catch {}
  };

  const handleDeleteDictEntry = async (id: string) => {
    try {
      await deleteModeDictionaryEntry(id);
      setDictEntries((prev) => prev.filter((e) => e.id !== id));
    } catch {}
  };

  const handleAddSnippet = async () => {
    if (!mode || !newTrigger.trim() || !newContent.trim()) return;
    try {
      const snippet = await addModeSnippet(mode.id, newTrigger.trim(), newContent.trim());
      setModeSnippets((prev) => [...prev, snippet]);
      setNewTrigger("");
      setNewContent("");
    } catch {}
  };

  const handleDeleteSnippet = async (id: string) => {
    try {
      await deleteModeSnippet(id);
      setModeSnippets((prev) => prev.filter((s) => s.id !== id));
    } catch {}
  };

  const handleAddBinding = async () => {
    if (!mode || !newProcessName.trim()) return;
    try {
      const binding = await addAppBinding(mode.id, newProcessName.trim());
      setBindings((prev) => [...prev, binding]);
      setNewProcessName("");
    } catch (e) {
      console.error("Failed to add app binding:", e);
    }
  };

  const handleDeleteBinding = async (id: string) => {
    try {
      await deleteAppBinding(id);
      setBindings((prev) => prev.filter((b) => b.id !== id));
    } catch (e) {
      console.error("Failed to delete app binding:", e);
    }
  };

  const handleSubmit = async () => {
    if (!name.trim()) {
      setError("Name is required");
      return;
    }

    // Flush any pending inputs before saving
    if (mode && newProcessName.trim()) {
      try {
        await addAppBinding(mode.id, newProcessName.trim());
        setNewProcessName("");
      } catch {}
    }
    if (mode && newPhrase.trim() && newReplacement.trim()) {
      try {
        await addModeDictionaryEntry(mode.id, newPhrase.trim(), newReplacement.trim());
        setNewPhrase("");
        setNewReplacement("");
      } catch {}
    }
    if (mode && newTrigger.trim() && newContent.trim()) {
      try {
        await addModeSnippet(mode.id, newTrigger.trim(), newContent.trim());
        setNewTrigger("");
        setNewContent("");
      } catch {}
    }

    setSaving(true);
    setError(null);
    try {
      if (isEdit) {
        await updateContextMode(mode.id, name, description, icon, color, writingStyle);
        onSave();
      } else {
        const created = await createContextMode(name, description, icon, color, writingStyle);
        onSave(created);
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mx-auto max-w-3xl px-8 py-10">
      {/* Header */}
      <div className="mb-7 flex items-center gap-3">
        <button
          onClick={onCancel}
          className="flex h-9 w-9 items-center justify-center rounded-lg text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
        >
          <X size={16} />
        </button>
        <h1 className="font-display text-xl font-semibold tracking-[-0.02em] text-text-primary">
          {isEdit ? "Edit Mode" : "New Context Mode"}
        </h1>
      </div>

      <div className="space-y-5">
        {/* Name */}
        <div>
          <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
            Name
          </label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. Programming"
            className="w-full rounded-lg border border-border bg-surface-2 px-3 py-2 text-sm text-text-primary outline-none transition-colors placeholder:text-text-muted focus:border-amber-400/45 focus:bg-surface-1"
          />
        </div>

        {/* Description */}
        <div>
          <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
            Description
          </label>
          <input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Brief description of what this mode is for"
            className="w-full rounded-lg border border-border bg-surface-2 px-3 py-2 text-sm text-text-primary outline-none transition-colors placeholder:text-text-muted focus:border-amber-400/45 focus:bg-surface-1"
          />
        </div>

        {/* Icon + Color row */}
        <div className="flex gap-6">
          <div>
            <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
              Icon
            </label>
            <div className="flex flex-wrap gap-1">
              {ICON_OPTIONS.map(({ name: n, Icon }) => (
                <button
                  key={n}
                  onClick={() => setIcon(n)}
                  className={cn(
                    "flex h-9 w-9 items-center justify-center rounded-lg transition-all duration-150",
                    icon === n
                      ? "border border-amber-400/35 bg-amber-500/[0.12] text-amber-300"
                      : "border border-transparent text-text-muted hover:bg-surface-2 hover:text-text-secondary"
                  )}
                >
                  <Icon size={14} />
                </button>
              ))}
            </div>
          </div>

          <div>
            <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
              Color
            </label>
            <div className="flex gap-1.5">
              {COLOR_OPTIONS.map(({ name: n, class: cls }) => (
                <button
                  key={n}
                  onClick={() => setColor(n)}
                  className={cn(
                    "h-7 w-7 rounded-full transition-all duration-200",
                    cls,
                    color === n
                      ? "scale-110 ring-2 ring-white/45 ring-offset-2 ring-offset-surface-0"
                      : "opacity-70 hover:opacity-100"
                  )}
                />
              ))}
            </div>
          </div>
        </div>

        {/* Writing Style */}
        <div>
          <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
            Writing Style
          </label>
          <div className="inline-flex gap-1 bg-surface-2 rounded-lg p-1">
            {(
              [
                { id: "formal", label: "Formal" },
                { id: "casual", label: "Casual" },
                { id: "very_casual", label: "Very Casual" },
              ] as const
            ).map(({ id, label }) => (
              <button
                key={id}
                onClick={() => setWritingStyle(id)}
                className={cn(
                  "inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
                  writingStyle === id
                    ? "border border-amber-400/30 bg-amber-500/[0.10] text-amber-300"
                    : "border border-transparent text-text-muted hover:text-text-secondary"
                )}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {/*
        <div>
          <div className="flex items-center justify-between mb-1.5">
            <label className="text-xs font-medium text-text-muted uppercase tracking-wider">
              AI Cleanup Prompt
            </label>
            {isEdit && mode.is_builtin && prompt !== DEFAULT_PROMPT && (
              <button
                onClick={() => setPrompt(DEFAULT_PROMPT)}
                className="text-[11px] text-amber-400 hover:text-amber-300"
              >
                Reset to default
              </button>
            )}
          </div>
          <textarea
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            rows={8}
            className="w-full rounded-lg bg-surface-2 border border-border px-3 py-2 text-sm text-text-primary font-mono leading-relaxed placeholder:text-text-muted focus:outline-none focus:border-amber-500/40 resize-y"
            placeholder="Instructions for the AI cleanup model..."
          />
          <p className="text-[11px] text-text-muted mt-1">
            This prompt tells the AI how to clean up your dictation. Customize it
            for domain-specific terminology and formatting.
          </p>
        </div>
        */}

        {/* Mode-scoped Dictionary Entries */}
        {isEdit && (
          <div>
            <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
              Custom Words ({dictEntries.length})
            </label>
            <div className="overflow-hidden rounded-xl border border-border bg-surface-2/80">
              {dictEntries.length > 0 && (
                <div className="max-h-48 overflow-y-auto divide-y divide-border">
                  {dictEntries.map((entry) => (
                    <div
                      key={entry.id}
                      className="flex items-center gap-2 px-3 py-1.5 text-xs group"
                    >
                      <span className="text-text-muted truncate min-w-0">
                        {entry.phrase}
                      </span>
                      <span className="text-text-muted shrink-0">&rarr;</span>
                      <span className="text-text-primary truncate flex-1 min-w-0">
                        {entry.replacement}
                      </span>
                      <button
                        onClick={() => handleDeleteDictEntry(entry.id)}
                        className="shrink-0 opacity-0 group-hover:opacity-100 text-text-muted hover:text-recording-400 transition-opacity"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-2 px-3 py-2 border-t border-border">
                <input
                  value={newPhrase}
                  onChange={(e) => setNewPhrase(e.target.value)}
                  placeholder="Heard as…"
                  className="flex-1 min-w-0 bg-transparent text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddDictEntry()}
                />
                <span className="text-text-muted text-xs shrink-0">&rarr;</span>
                <input
                  value={newReplacement}
                  onChange={(e) => setNewReplacement(e.target.value)}
                  placeholder="Replace with…"
                  className="flex-1 min-w-0 bg-transparent text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddDictEntry()}
                />
                <button
                  onClick={handleAddDictEntry}
                  disabled={!newPhrase.trim() || !newReplacement.trim()}
                  className="shrink-0 rounded-md p-1 text-amber-300 transition-colors hover:bg-amber-500/[0.10] disabled:text-text-muted disabled:opacity-40 disabled:hover:bg-transparent"
                >
                  <Plus size={14} />
                </button>
              </div>
            </div>
            <p className="text-[11px] text-text-muted mt-1">
              Words and phrases corrected when this mode is active.
            </p>
          </div>
        )}

        {/* Mode-scoped Snippets */}
        {isEdit && (
          <div>
            <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
              Snippets ({modeSnippets.length})
            </label>
            <div className="overflow-hidden rounded-xl border border-border bg-surface-2/80">
              {modeSnippets.length > 0 && (
                <div className="max-h-48 overflow-y-auto divide-y divide-border">
                  {modeSnippets.map((snippet) => (
                    <div
                      key={snippet.id}
                      className="flex items-center gap-2 px-3 py-1.5 text-xs group"
                    >
                      <span className="text-text-muted truncate min-w-0">
                        {snippet.trigger}
                      </span>
                      <span className="text-text-muted shrink-0">&rarr;</span>
                      <span className="text-text-primary truncate flex-1 min-w-0 font-mono">
                        {snippet.content}
                      </span>
                      <button
                        onClick={() => handleDeleteSnippet(snippet.id)}
                        className="shrink-0 opacity-0 group-hover:opacity-100 text-text-muted hover:text-recording-400 transition-opacity"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-2 px-3 py-2 border-t border-border">
                <input
                  value={newTrigger}
                  onChange={(e) => setNewTrigger(e.target.value)}
                  placeholder="Word…"
                  className="w-28 shrink-0 bg-transparent text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddSnippet()}
                />
                <span className="text-text-muted text-xs shrink-0">&rarr;</span>
                <input
                  value={newContent}
                  onChange={(e) => setNewContent(e.target.value)}
                  placeholder="Expands to…"
                  className="flex-1 min-w-0 bg-transparent text-xs text-text-primary font-mono placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddSnippet()}
                />
                <button
                  onClick={handleAddSnippet}
                  disabled={!newTrigger.trim() || !newContent.trim()}
                  className="shrink-0 rounded-md p-1 text-amber-300 transition-colors hover:bg-amber-500/[0.10] disabled:text-text-muted disabled:opacity-40 disabled:hover:bg-transparent"
                >
                  <Plus size={14} />
                </button>
              </div>
            </div>
            <p className="text-[11px] text-text-muted mt-1">
              Trigger words that expand into longer text when this mode is active.
            </p>
          </div>
        )}

        {/* App Bindings — auto-switch mode when this app is focused */}
        {isEdit && (
          <div>
            <label className="mb-2 block text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
              App Bindings ({bindings.length})
            </label>
            <div className="overflow-hidden rounded-xl border border-border bg-surface-2/80">
              {bindings.length > 0 && (
                <div className="max-h-48 overflow-y-auto divide-y divide-border">
                  {bindings.map((binding) => (
                    <div
                      key={binding.id}
                      className="flex items-center gap-2 px-3 py-1.5 text-xs group"
                    >
                      <span className="text-text-primary truncate flex-1 min-w-0 font-mono">
                        {binding.process_name}
                      </span>
                      <button
                        onClick={() => handleDeleteBinding(binding.id)}
                        className="shrink-0 opacity-0 group-hover:opacity-100 text-text-muted hover:text-recording-400 transition-opacity"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-2 px-3 py-2 border-t border-border">
                <input
                  value={newProcessName}
                  onChange={(e) => setNewProcessName(e.target.value)}
                  placeholder="e.g. Code.exe, chrome.exe"
                  className="flex-1 min-w-0 bg-transparent text-xs text-text-primary font-mono placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddBinding()}
                />
                <button
                  onClick={handleAddBinding}
                  disabled={!newProcessName.trim()}
                  className="shrink-0 rounded-md p-1 text-amber-300 transition-colors hover:bg-amber-500/[0.10] disabled:text-text-muted disabled:opacity-40 disabled:hover:bg-transparent"
                >
                  <Plus size={14} />
                </button>
              </div>
            </div>
            <p className="text-[11px] text-text-muted mt-1">
              When recording starts with this app focused, OmniVox auto-switches
              to this mode. Enable auto-switch in Settings.
            </p>
          </div>
        )}

        {/* Error */}
        {error && (
          <div className="rounded-lg border border-recording-500/25 bg-recording-500/[0.08] px-3 py-2 text-sm text-recording-400">
            {error}
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center gap-3 pt-2">
          <button
            onClick={handleSubmit}
            disabled={saving}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-lg px-4 py-2 text-sm font-medium transition-colors",
              saving
                ? "bg-amber-500/[0.08] text-amber-300/60"
                : "border border-amber-400/35 bg-amber-500/[0.14] text-amber-300 hover:border-amber-400/60 hover:bg-amber-500/[0.22]"
            )}
          >
            <Check size={14} />
            {saving ? "Saving…" : isEdit ? "Save Changes" : "Create & Continue"}
          </button>
          <button
            onClick={onCancel}
            className="rounded-lg px-4 py-2 text-sm text-text-muted transition-colors hover:text-text-secondary"
          >
            Cancel
          </button>
        </div>
        {!isEdit && (
          <p className="text-[11px] text-text-muted">
            After creating, you'll be able to add custom words, snippets, and app bindings.
          </p>
        )}
      </div>
    </div>
  );
}
