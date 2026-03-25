import { useState, useEffect, useCallback } from "react";
import {
  Layers,
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

const DEFAULT_PROMPT = `You are a dictation cleanup assistant. /no_think
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
      <div className="flex items-start justify-between gap-4 mb-8">
        <div>
          <div className="flex items-center gap-2 mb-1">
            <Layers size={18} className="text-amber-400" />
            <h1 className="text-lg font-semibold text-text-primary">
              Context Modes
            </h1>
          </div>
          <p className="text-sm text-text-muted">
            Switch between profiles that customize how dictation is cleaned up.
            Each mode has its own AI prompt, dictionary, and snippets.
          </p>
        </div>
        <button
          onClick={() => setCreating(true)}
          className="shrink-0 mt-0.5 inline-flex items-center gap-1.5 rounded-lg bg-amber-500/15 px-3 py-1.5 text-sm font-medium text-amber-400 border border-amber-500/30 hover:bg-amber-500/25 transition-colors"
        >
          <Plus size={14} />
          New Mode
        </button>
      </div>

      {/* Mode Cards */}
      {loading ? (
        <div className="text-sm text-text-muted text-center py-12">Loading...</div>
      ) : (
        <div className="grid gap-3">
          {modes.map((mode, i) => {
            const Icon = getIconComponent(mode.icon);
            const colorCls = getColorClass(mode.color);
            const isActive = mode.id === activeId;

            return (
              <div
                key={mode.id}
                className={cn(
                  "bg-surface-1 rounded-xl border p-4 transition-all animate-slide-up",
                  isActive
                    ? "border-amber-500/30"
                    : "border-border hover:border-border-hover"
                )}
                style={{
                  opacity: 0,
                  animationDelay: `${i * 0.05}s`,
                  animationFillMode: "forwards",
                }}
              >
                <div className="flex items-center gap-3">
                  {/* Icon */}
                  <div
                    className={cn(
                      "flex h-9 w-9 items-center justify-center rounded-lg",
                      colorCls + "/15"
                    )}
                  >
                    <Icon size={16} className={colorCls.replace("bg-", "text-")} />
                  </div>

                  {/* Info */}
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="text-sm font-medium text-text-primary truncate">
                        {mode.name}
                      </span>
                      {isActive && (
                        <span className="text-[10px] font-medium text-amber-400 bg-amber-500/10 px-1.5 py-0.5 rounded">
                          ACTIVE
                        </span>
                      )}
                      {mode.is_builtin && (
                        <span className="text-[10px] text-text-muted">Built-in</span>
                      )}
                    </div>
                    <p className="text-xs text-text-muted truncate">
                      {mode.description}
                    </p>
                  </div>

                  {/* Actions */}
                  <div className="flex items-center gap-1.5">
                    {!isActive && (
                      <button
                        onClick={() => handleActivate(mode.id)}
                        className="inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs font-medium text-text-secondary hover:text-amber-400 hover:bg-amber-500/10 border border-transparent hover:border-amber-500/20 transition-colors"
                      >
                        <Check size={12} />
                        Activate
                      </button>
                    )}
                    <button
                      onClick={() => setEditing(mode)}
                      className="flex h-7 w-7 items-center justify-center rounded-md text-text-muted hover:text-text-secondary hover:bg-surface-2 transition-colors"
                      title="Edit"
                    >
                      <Pencil size={13} />
                    </button>
                    {!mode.is_builtin && (
                      <button
                        onClick={() => handleDelete(mode.id)}
                        className="flex h-7 w-7 items-center justify-center rounded-md text-text-muted hover:text-recording-400 hover:bg-recording-500/10 transition-colors"
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
  const [prompt, setPrompt] = useState(mode?.llm_prompt ?? DEFAULT_PROMPT);
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
        await updateContextMode(mode.id, name, description, icon, color, prompt, writingStyle);
        onSave();
      } else {
        const created = await createContextMode(name, description, icon, color, prompt, writingStyle);
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
      <div className="flex items-center gap-3 mb-6">
        <button
          onClick={onCancel}
          className="flex h-8 w-8 items-center justify-center rounded-lg text-text-muted hover:text-text-secondary hover:bg-surface-2 transition-colors"
        >
          <X size={16} />
        </button>
        <h1 className="text-lg font-semibold text-text-primary">
          {isEdit ? "Edit Mode" : "New Context Mode"}
        </h1>
      </div>

      <div className="space-y-5">
        {/* Name */}
        <div>
          <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
            Name
          </label>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. Programming"
            className="w-full rounded-lg bg-surface-2 border border-border px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-amber-500/40"
          />
        </div>

        {/* Description */}
        <div>
          <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
            Description
          </label>
          <input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Brief description of what this mode is for"
            className="w-full rounded-lg bg-surface-2 border border-border px-3 py-2 text-sm text-text-primary placeholder:text-text-muted focus:outline-none focus:border-amber-500/40"
          />
        </div>

        {/* Icon + Color row */}
        <div className="flex gap-6">
          <div>
            <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
              Icon
            </label>
            <div className="flex flex-wrap gap-1">
              {ICON_OPTIONS.map(({ name: n, Icon }) => (
                <button
                  key={n}
                  onClick={() => setIcon(n)}
                  className={cn(
                    "flex h-8 w-8 items-center justify-center rounded-md transition-colors",
                    icon === n
                      ? "bg-amber-500/15 text-amber-400 border border-amber-500/30"
                      : "text-text-muted hover:text-text-secondary hover:bg-surface-2 border border-transparent"
                  )}
                >
                  <Icon size={14} />
                </button>
              ))}
            </div>
          </div>

          <div>
            <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
              Color
            </label>
            <div className="flex gap-1.5">
              {COLOR_OPTIONS.map(({ name: n, class: cls }) => (
                <button
                  key={n}
                  onClick={() => setColor(n)}
                  className={cn(
                    "h-7 w-7 rounded-full transition-all",
                    cls,
                    color === n
                      ? "ring-2 ring-white/40 ring-offset-2 ring-offset-surface-0 scale-110"
                      : "opacity-60 hover:opacity-100"
                  )}
                />
              ))}
            </div>
          </div>
        </div>

        {/* Writing Style */}
        <div>
          <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
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
                className={`inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                  writingStyle === id
                    ? "bg-amber-500/15 text-amber-400 border border-amber-500/30"
                    : "text-text-muted hover:text-text-secondary border border-transparent"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </div>

        {/* LLM Prompt */}
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

        {/* Mode-scoped Dictionary Entries */}
        {isEdit && (
          <div>
            <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
              Custom Words ({dictEntries.length})
            </label>
            <div className="rounded-lg bg-surface-2 border border-border overflow-hidden">
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
                  className="shrink-0 text-amber-400 disabled:text-text-muted disabled:opacity-30"
                >
                  <Plus size={14} />
                </button>
              </div>
            </div>
            <p className="text-[11px] text-text-muted mt-1">
              Words and phrases corrected when this mode is active. Applied before
              AI cleanup.
            </p>
          </div>
        )}

        {/* Mode-scoped Snippets */}
        {isEdit && (
          <div>
            <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
              Snippets ({modeSnippets.length})
            </label>
            <div className="rounded-lg bg-surface-2 border border-border overflow-hidden">
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
                  className="shrink-0 text-amber-400 disabled:text-text-muted disabled:opacity-30"
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
            <label className="block text-xs font-medium text-text-muted uppercase tracking-wider mb-1.5">
              App Bindings ({bindings.length})
            </label>
            <div className="rounded-lg bg-surface-2 border border-border overflow-hidden">
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
                  className="shrink-0 text-amber-400 disabled:text-text-muted disabled:opacity-30"
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
          <div className="text-sm text-recording-400 bg-recording-500/10 border border-recording-500/20 rounded-lg px-3 py-2">
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
                ? "bg-amber-500/10 text-amber-400/50"
                : "bg-amber-500/15 text-amber-400 border border-amber-500/30 hover:bg-amber-500/25"
            )}
          >
            <Check size={14} />
            {saving ? "Saving..." : isEdit ? "Save Changes" : "Create & Continue"}
          </button>
          <button
            onClick={onCancel}
            className="rounded-lg px-4 py-2 text-sm text-text-muted hover:text-text-secondary transition-colors"
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
