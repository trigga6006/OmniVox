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
import { Button, Card, Input, Badge, Segmented } from "@/components/ui";

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

// Painterly mosaic tesserae (keys preserved for saved modes).
const COLOR_OPTIONS = [
  { name: "amber", class: "bg-amber-500" },   // salmon (via token)
  { name: "blue", class: "bg-[#6e809b]" },    // slate
  { name: "green", class: "bg-[#9ba97b]" },   // sage
  { name: "purple", class: "bg-[#a1768e]" },  // plum
  { name: "red", class: "bg-[#c76a4c]" },     // clay
  { name: "cyan", class: "bg-[#5e948c]" },    // teal
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

/* ──────────────────── Shared layout helpers ──────────────────── */

const eyebrowClass =
  "font-mono text-[10px] font-semibold uppercase tracking-[0.2em] text-text-muted";

/** Labeled section in the mode editor — mono eyebrow + content. */
function FormSection({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <span className={cn("mb-2 block", eyebrowClass)}>{label}</span>
      {children}
      {hint && <p className="mt-1.5 text-[11px] text-text-muted">{hint}</p>}
    </div>
  );
}

/** Compact rounded surface that wraps a list of rows + an add-row. */
function ListShell({ children }: { children: React.ReactNode }) {
  return (
    <div className="overflow-hidden rounded-xl border border-border bg-surface-2/80">
      {children}
    </div>
  );
}

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
    <div className="mx-auto max-w-3xl px-8 pt-6 pb-10">
      {/* Header */}
      <div className="mb-6 flex items-start justify-between gap-4">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            Context Modes
          </h1>
          <p className="mt-1 text-sm text-text-muted">
            Switch between profiles that customize writing style, dictionary entries,
            snippets, and app bindings.
          </p>
        </div>
        <Button
          variant="primary"
          size="sm"
          icon={<Plus />}
          onClick={() => setCreating(true)}
          className="mt-1 shrink-0"
        >
          New Mode
        </Button>
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
              <Card
                key={mode.id}
                className={cn(
                  "animate-slide-up p-4 opacity-0 transition-colors hover:bg-surface-2",
                  isActive && "border-amber-400/35"
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
                      {isActive && <Badge tone="green">Active</Badge>}
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
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Check />}
                        onClick={() => handleActivate(mode.id)}
                      >
                        Activate
                      </Button>
                    )}
                    <Button
                      variant="ghost"
                      size="sm"
                      icon={<Pencil />}
                      aria-label="Edit"
                      title="Edit"
                      onClick={() => setEditing(mode)}
                    />
                    {!mode.is_builtin && (
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Trash2 />}
                        aria-label="Delete"
                        title="Delete"
                        onClick={() => handleDelete(mode.id)}
                      />
                    )}
                  </div>
                </div>
              </Card>
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
  // Empty (legacy rows) means the default agent-prompt profile.
  const [structuredProfile, setStructuredProfile] = useState(
    mode?.structured_profile || "agent-prompt"
  );
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
        await updateContextMode(
          mode.id,
          name,
          description,
          icon,
          color,
          writingStyle,
          structuredProfile
        );
        onSave();
      } else {
        const created = await createContextMode(
          name,
          description,
          icon,
          color,
          writingStyle,
          structuredProfile
        );
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
        <Button
          variant="ghost"
          size="sm"
          icon={<X />}
          aria-label="Back"
          onClick={onCancel}
        />
        <h1 className="font-display text-xl font-semibold tracking-[-0.02em] text-text-primary">
          {isEdit ? "Edit Mode" : "New Context Mode"}
        </h1>
      </div>

      <div className="space-y-5">
        {/* Name */}
        <FormSection label="Name">
          <Input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="e.g. Programming"
          />
        </FormSection>

        {/* Description */}
        <FormSection label="Description">
          <Input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            placeholder="Brief description of what this mode is for"
          />
        </FormSection>

        {/* Icon + Color row */}
        <div className="flex gap-6">
          <FormSection label="Icon">
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
          </FormSection>

          <FormSection label="Color">
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
          </FormSection>
        </div>

        {/* Writing Style */}
        <FormSection label="Writing Style">
          <Segmented
            options={[
              { value: "formal", label: "Formal" },
              { value: "casual", label: "Casual" },
              { value: "very_casual", label: "Very Casual" },
            ]}
            value={writingStyle}
            onChange={setWritingStyle}
          />
        </FormSection>

        {/* Structured Mode Profile */}
        <FormSection
          label="Structured Mode Profile"
          hint="How Structured Mode formats dictations while this mode is active: a prompt for an AI coding agent, an email draft, or a notes outline."
        >
          <Segmented
            options={[
              { value: "agent-prompt", label: "Agent Prompt" },
              { value: "email", label: "Email" },
              { value: "notes-outline", label: "Notes" },
            ]}
            value={structuredProfile}
            onChange={setStructuredProfile}
          />
        </FormSection>

        {/* Mode-scoped Dictionary Entries */}
        {isEdit && (
          <FormSection
            label={`Custom Words (${dictEntries.length})`}
            hint="Words and phrases corrected when this mode is active."
          >
            <ListShell>
              {dictEntries.length > 0 && (
                <div className="max-h-48 divide-y divide-border overflow-y-auto">
                  {dictEntries.map((entry) => (
                    <div
                      key={entry.id}
                      className="group flex items-center gap-2 px-3 py-1.5 text-xs"
                    >
                      <span className="min-w-0 truncate text-text-muted">
                        {entry.phrase}
                      </span>
                      <span className="shrink-0 text-text-muted">&rarr;</span>
                      <span className="min-w-0 flex-1 truncate text-text-primary">
                        {entry.replacement}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Trash2 />}
                        aria-label="Delete word"
                        className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                        onClick={() => handleDeleteDictEntry(entry.id)}
                      />
                    </div>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-2 border-t border-border px-3 py-2">
                <input
                  value={newPhrase}
                  onChange={(e) => setNewPhrase(e.target.value)}
                  placeholder="Heard as…"
                  className="min-w-0 flex-1 bg-transparent text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddDictEntry()}
                />
                <span className="shrink-0 text-xs text-text-muted">&rarr;</span>
                <input
                  value={newReplacement}
                  onChange={(e) => setNewReplacement(e.target.value)}
                  placeholder="Replace with…"
                  className="min-w-0 flex-1 bg-transparent text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddDictEntry()}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Plus />}
                  aria-label="Add word"
                  className="shrink-0 text-amber-300"
                  disabled={!newPhrase.trim() || !newReplacement.trim()}
                  onClick={handleAddDictEntry}
                />
              </div>
            </ListShell>
          </FormSection>
        )}

        {/* Mode-scoped Snippets */}
        {isEdit && (
          <FormSection
            label={`Snippets (${modeSnippets.length})`}
            hint="Trigger words that expand into longer text when this mode is active."
          >
            <ListShell>
              {modeSnippets.length > 0 && (
                <div className="max-h-48 divide-y divide-border overflow-y-auto">
                  {modeSnippets.map((snippet) => (
                    <div
                      key={snippet.id}
                      className="group flex items-center gap-2 px-3 py-1.5 text-xs"
                    >
                      <span className="min-w-0 truncate text-text-muted">
                        {snippet.trigger}
                      </span>
                      <span className="shrink-0 text-text-muted">&rarr;</span>
                      <span className="min-w-0 flex-1 truncate font-mono text-text-primary">
                        {snippet.content}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Trash2 />}
                        aria-label="Delete snippet"
                        className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                        onClick={() => handleDeleteSnippet(snippet.id)}
                      />
                    </div>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-2 border-t border-border px-3 py-2">
                <input
                  value={newTrigger}
                  onChange={(e) => setNewTrigger(e.target.value)}
                  placeholder="Word…"
                  className="w-28 shrink-0 bg-transparent text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddSnippet()}
                />
                <span className="shrink-0 text-xs text-text-muted">&rarr;</span>
                <input
                  value={newContent}
                  onChange={(e) => setNewContent(e.target.value)}
                  placeholder="Expands to…"
                  className="min-w-0 flex-1 bg-transparent font-mono text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddSnippet()}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Plus />}
                  aria-label="Add snippet"
                  className="shrink-0 text-amber-300"
                  disabled={!newTrigger.trim() || !newContent.trim()}
                  onClick={handleAddSnippet}
                />
              </div>
            </ListShell>
          </FormSection>
        )}

        {/* App Bindings — auto-switch mode when this app is focused */}
        {isEdit && (
          <FormSection
            label={`App Bindings (${bindings.length})`}
            hint="When recording starts with this app focused, OmniVox auto-switches to this mode. Enable auto-switch in Settings."
          >
            <ListShell>
              {bindings.length > 0 && (
                <div className="max-h-48 divide-y divide-border overflow-y-auto">
                  {bindings.map((binding) => (
                    <div
                      key={binding.id}
                      className="group flex items-center gap-2 px-3 py-1.5 text-xs"
                    >
                      <span className="min-w-0 flex-1 truncate font-mono text-text-primary">
                        {binding.process_name}
                      </span>
                      <Button
                        variant="ghost"
                        size="sm"
                        icon={<Trash2 />}
                        aria-label="Delete binding"
                        className="shrink-0 opacity-0 transition-opacity group-hover:opacity-100"
                        onClick={() => handleDeleteBinding(binding.id)}
                      />
                    </div>
                  ))}
                </div>
              )}
              <div className="flex items-center gap-2 border-t border-border px-3 py-2">
                <input
                  value={newProcessName}
                  onChange={(e) => setNewProcessName(e.target.value)}
                  placeholder="e.g. Code.exe, chrome.exe"
                  className="min-w-0 flex-1 bg-transparent font-mono text-xs text-text-primary placeholder:text-text-muted focus:outline-none"
                  onKeyDown={(e) => e.key === "Enter" && handleAddBinding()}
                />
                <Button
                  variant="ghost"
                  size="sm"
                  icon={<Plus />}
                  aria-label="Add binding"
                  className="shrink-0 text-amber-300"
                  disabled={!newProcessName.trim()}
                  onClick={handleAddBinding}
                />
              </div>
            </ListShell>
          </FormSection>
        )}

        {/* Error */}
        {error && (
          <div className="rounded-lg border border-recording-500/25 bg-recording-500/[0.08] px-3 py-2 text-sm text-recording-400">
            {error}
          </div>
        )}

        {/* Actions */}
        <div className="flex items-center gap-3 pt-2">
          <Button
            variant="primary"
            icon={<Check />}
            loading={saving}
            disabled={saving}
            onClick={handleSubmit}
          >
            {saving ? "Saving…" : isEdit ? "Save Changes" : "Create & Continue"}
          </Button>
          <Button variant="ghost" onClick={onCancel}>
            Cancel
          </Button>
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
