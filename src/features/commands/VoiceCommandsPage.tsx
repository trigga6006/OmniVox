import { useState, useEffect, useCallback, useRef } from "react";
import {
  Command as CommandIcon,
  Plus,
  Trash2,
  Pencil,
  Check,
  X,
  RotateCcw,
} from "lucide-react";
import {
  listVoiceCommands,
  addVoiceCommand,
  updateVoiceCommand,
  deleteVoiceCommand,
  resetVoiceCommands,
  type VoiceCommand,
  type TriggerScope,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

/** Friendly labels for built-in action variant names. */
const ACTION_LABELS: Record<string, string> = {
  NewLine: "New line",
  NewParagraph: "New paragraph",
  DeleteLastWord: "Delete last word",
  Send: "Send (Enter)",
  SelectAll: "Select all",
  Copy: "Copy",
  Cut: "Cut",
  Undo: "Undo",
  Redo: "Redo",
  PressTab: "Tab",
  PressEscape: "Escape",
  PressEnter: "Enter",
};

/** Render an action string (built-in name or `key:ctrl+shift+k`) for display. */
function actionLabel(action: string): string {
  if (action in ACTION_LABELS) return ACTION_LABELS[action];
  if (action.startsWith("key:")) {
    return action
      .slice(4)
      .split("+")
      .map((t) => (t.length ? t[0].toUpperCase() + t.slice(1) : t))
      .join(" + ");
  }
  return action;
}

const SCOPE_LABELS: Record<TriggerScope, string> = {
  anywhere: "Anywhere",
  end_of_utterance: "End of utterance",
};

function ScopeSelect({
  value,
  onChange,
}: {
  value: TriggerScope;
  onChange: (scope: TriggerScope) => void;
}) {
  return (
    <select
      value={value}
      onChange={(e) => onChange(e.target.value as TriggerScope)}
      className="rounded-lg border border-border bg-surface-2 px-2 py-1 text-xs text-text-secondary outline-none transition-colors focus:border-amber-400/45"
    >
      <option value="anywhere">{SCOPE_LABELS.anywhere}</option>
      <option value="end_of_utterance">{SCOPE_LABELS.end_of_utterance}</option>
    </select>
  );
}

export function VoiceCommandsPage() {
  const [commands, setCommands] = useState<VoiceCommand[]>([]);
  const [loading, setLoading] = useState(true);

  // Inline add (custom command)
  const [adding, setAdding] = useState(false);
  const [newPhrase, setNewPhrase] = useState("");
  const [newCombo, setNewCombo] = useState("");
  const phraseRef = useRef<HTMLInputElement>(null);

  // Inline edit (phrase only)
  const [editId, setEditId] = useState<string | null>(null);
  const [editPhrase, setEditPhrase] = useState("");

  const load = useCallback(() => {
    setLoading(true);
    listVoiceCommands()
      .then(setCommands)
      .catch((e) => console.error("Failed to load voice commands:", e))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (adding) phraseRef.current?.focus();
  }, [adding]);

  const handleAdd = useCallback(() => {
    const p = newPhrase.trim();
    const combo = newCombo.trim();
    if (!p || !combo) return;
    const action = combo.startsWith("key:") ? combo : `key:${combo}`;
    addVoiceCommand(p, action, "anywhere")
      .then((cmd) => {
        setCommands((prev) => [...prev, cmd]);
        setNewPhrase("");
        setNewCombo("");
        setAdding(false);
      })
      .catch((e) => console.error("Failed to add voice command:", e));
  }, [newPhrase, newCombo]);

  const handleUpdatePhrase = useCallback(
    (cmd: VoiceCommand) => {
      const p = editPhrase.trim();
      if (!p) return;
      updateVoiceCommand(cmd.id, p, cmd.action, cmd.trigger_scope, cmd.enabled)
        .then(() => {
          setCommands((prev) =>
            prev.map((c) => (c.id === cmd.id ? { ...c, phrase: p } : c))
          );
          setEditId(null);
        })
        .catch((e) => console.error("Failed to update voice command:", e));
    },
    [editPhrase]
  );

  const patch = useCallback(
    (cmd: VoiceCommand, changes: Partial<VoiceCommand>) => {
      const next = { ...cmd, ...changes };
      updateVoiceCommand(
        next.id,
        next.phrase,
        next.action,
        next.trigger_scope,
        next.enabled
      )
        .then(() => {
          setCommands((prev) => prev.map((c) => (c.id === next.id ? next : c)));
        })
        .catch((e) => console.error("Failed to update voice command:", e));
    },
    []
  );

  const handleDelete = useCallback((id: string) => {
    deleteVoiceCommand(id)
      .then(() => setCommands((prev) => prev.filter((c) => c.id !== id)))
      .catch((e) => console.error("Failed to delete voice command:", e));
  }, []);

  const handleReset = useCallback(() => {
    resetVoiceCommands()
      .then(setCommands)
      .catch((e) => console.error("Failed to reset voice commands:", e));
  }, []);

  const startEdit = (cmd: VoiceCommand) => {
    setEditId(cmd.id);
    setEditPhrase(cmd.phrase);
  };

  return (
    <div className="flex h-full flex-col px-8 pt-6 pb-8">
      {/* Header */}
      <div className="flex items-start justify-between">
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
            Voice Commands
          </h1>
          <p className="mt-1 text-sm text-text-muted">
            Spoken triggers for keystrokes — enable, re-scope, or add your own
          </p>
        </div>
        <button
          onClick={handleReset}
          className="mt-1 inline-flex items-center gap-1.5 rounded-lg border border-border px-3 py-1.5 text-xs font-medium text-text-muted transition-colors hover:border-border-hover hover:text-text-secondary"
        >
          <RotateCcw size={13} />
          Reset to defaults
        </button>
      </div>

      {/* List */}
      <div className="mt-5 flex flex-1 flex-col gap-2 overflow-y-auto pr-1">
        {commands.map((cmd) => {
          const isEditing = editId === cmd.id;

          return (
            <div
              key={cmd.id}
              className={cn(
                "group flex items-center gap-3 rounded-xl border px-4 py-3 transition-all duration-200",
                cmd.enabled
                  ? "border-border bg-surface-1/80 hover:border-border-hover hover:bg-surface-1"
                  : "border-border/50 bg-surface-1/40 opacity-60 hover:opacity-100"
              )}
            >
              {/* Enable toggle */}
              <button
                onClick={() => patch(cmd, { enabled: !cmd.enabled })}
                title={cmd.enabled ? "Enabled" : "Disabled"}
                aria-pressed={cmd.enabled}
                className={cn(
                  "relative h-5 w-9 shrink-0 rounded-full transition-colors",
                  cmd.enabled ? "bg-amber-500/70" : "bg-surface-3"
                )}
              >
                <span
                  className={cn(
                    "absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform",
                    cmd.enabled ? "translate-x-4" : "translate-x-0.5"
                  )}
                />
              </button>

              {/* Phrase */}
              {isEditing ? (
                <input
                  value={editPhrase}
                  onChange={(e) => setEditPhrase(e.target.value)}
                  className="flex-1 rounded-lg border border-amber-400/40 bg-surface-2 px-3 py-1.5 text-sm text-text-primary outline-none focus:border-amber-400/60"
                  placeholder="Spoken phrase…"
                  onKeyDown={(e) => e.key === "Enter" && handleUpdatePhrase(cmd)}
                  autoFocus
                />
              ) : (
                <kbd className="shrink-0 rounded-md border border-amber-400/25 bg-amber-500/[0.08] px-2 py-0.5 font-mono text-xs text-amber-300">
                  {cmd.phrase}
                </kbd>
              )}

              {/* Action label */}
              {!isEditing && (
                <span className="flex-1 truncate text-sm text-text-secondary">
                  {actionLabel(cmd.action)}
                  {cmd.built_in ? "" : " (custom)"}
                </span>
              )}

              {/* Scope */}
              <ScopeSelect
                value={cmd.trigger_scope}
                onChange={(scope) => patch(cmd, { trigger_scope: scope })}
              />

              {/* Actions */}
              {isEditing ? (
                <div className="flex items-center gap-0.5 shrink-0">
                  <button
                    onClick={() => handleUpdatePhrase(cmd)}
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
              ) : (
                <div className="flex items-center gap-0.5 shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                  <button
                    onClick={() => startEdit(cmd)}
                    className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
                  >
                    <Pencil size={13} />
                  </button>
                  <button
                    onClick={() => handleDelete(cmd.id)}
                    className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
                  >
                    <Trash2 size={13} />
                  </button>
                </div>
              )}
            </div>
          );
        })}

        {/* Inline add row (custom key combo) */}
        {adding && (
          <div className="flex items-center gap-2 rounded-xl border border-amber-400/40 bg-surface-1 p-3 shadow-sm">
            <input
              ref={phraseRef}
              value={newPhrase}
              onChange={(e) => setNewPhrase(e.target.value)}
              className="flex-1 rounded-md border border-border bg-surface-2 px-2.5 py-1.5 text-sm text-text-primary outline-none focus:border-amber-500/40"
              placeholder="Spoken phrase…"
            />
            <input
              value={newCombo}
              onChange={(e) => setNewCombo(e.target.value)}
              className="w-40 rounded-md border border-border bg-surface-2 px-2.5 py-1.5 font-mono text-sm text-text-primary outline-none focus:border-amber-500/40"
              placeholder="ctrl+shift+k"
              onKeyDown={(e) => e.key === "Enter" && handleAdd()}
            />
            <button
              onClick={handleAdd}
              className="rounded-md p-1.5 text-green-400 transition-colors hover:bg-surface-3"
            >
              <Check size={14} />
            </button>
            <button
              onClick={() => {
                setAdding(false);
                setNewPhrase("");
                setNewCombo("");
              }}
              className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-3"
            >
              <X size={14} />
            </button>
          </div>
        )}

        {/* Add button */}
        {!adding && !loading && (
          <button
            onClick={() => setAdding(true)}
            className="flex items-center gap-2 rounded-xl border border-dashed border-border/70 px-4 py-3 text-sm text-text-muted transition-all duration-200 hover:border-amber-400/40 hover:bg-amber-500/[0.05] hover:text-amber-300"
          >
            <Plus size={14} strokeWidth={2} />
            Add custom command (key combo)
          </button>
        )}

        {!loading && commands.length === 0 && !adding && (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-center">
            <CommandIcon size={36} strokeWidth={1.5} className="text-text-muted" />
            <p className="text-sm text-text-secondary">No voice commands</p>
          </div>
        )}
      </div>
    </div>
  );
}
