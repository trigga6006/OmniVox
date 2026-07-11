import { useState, useEffect, useCallback, useRef, useMemo } from "react";
import {
  Command as CommandIcon,
  Plus,
  Trash2,
  Pencil,
  Check,
  X,
  RotateCcw,
  Keyboard,
  MousePointerClick,
  Rocket,
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
import {
  Button,
  Card,
  CardHeader,
  CardTitle,
  CardBody,
  Toggle,
  Segmented,
  Input,
  Kbd,
  Badge,
  EmptyState,
} from "@/components/ui";
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
  BulletItem: "Bullet list item",
  NumberedItem: "Numbered list item",
  EndList: "End list",
};

/** Friendly labels for the mouse action encodings (`mouse:click`, …). */
const MOUSE_LABELS: Record<string, string> = {
  "mouse:click": "Mouse click",
  "mouse:right_click": "Right click",
  "mouse:double_click": "Double click",
  "mouse:scroll_up": "Scroll up",
  "mouse:scroll_down": "Scroll down",
};

/** Render an action string (built-in name, `key:…`, `mouse:…`, or `launch:…`). */
function actionLabel(action: string): string {
  if (action in ACTION_LABELS) return ACTION_LABELS[action];
  if (action in MOUSE_LABELS) return MOUSE_LABELS[action];
  if (action.startsWith("launch:")) return `Launch: ${action.slice(7)}`;
  if (action.startsWith("key:")) {
    return action
      .slice(4)
      .split("+")
      .map((t) => (t.length ? t[0].toUpperCase() + t.slice(1) : t))
      .join(" + ");
  }
  return action;
}

/** The kind of action being composed in the add form. */
type ActionType = "key" | "mouse" | "launch";

const SCOPE_OPTIONS: { value: TriggerScope; label: string }[] = [
  { value: "anywhere", label: "Anywhere" },
  { value: "end_of_utterance", label: "At end" },
];

interface CommandRowProps {
  cmd: VoiceCommand;
  isEditing: boolean;
  editPhrase: string;
  onEditPhraseChange: (value: string) => void;
  onSubmitEdit: () => void;
  onCancelEdit: () => void;
  onStartEdit: () => void;
  onToggleEnabled: (enabled: boolean) => void;
  onScopeChange: (scope: TriggerScope) => void;
  onDelete: () => void;
}

/**
 * One command row.  Defined at module scope (not nested in VoiceCommandsPage)
 * so typing in the rename input doesn't remount it on every keystroke, which
 * jumped the caret to the end and made mid-string edits unusable.
 */
function CommandRow({
  cmd,
  isEditing,
  editPhrase,
  onEditPhraseChange,
  onSubmitEdit,
  onCancelEdit,
  onStartEdit,
  onToggleEnabled,
  onScopeChange,
  onDelete,
}: CommandRowProps) {
  return (
    <div
      className={cn(
        "group flex items-center gap-3 py-2.5 transition-opacity",
        !cmd.enabled && "opacity-55 hover:opacity-100"
      )}
    >
      <Toggle
        checked={cmd.enabled}
        onChange={onToggleEnabled}
        aria-label={`${cmd.phrase} ${cmd.enabled ? "enabled" : "disabled"}`}
      />

      {isEditing ? (
        <Input
          value={editPhrase}
          onChange={(e) => onEditPhraseChange(e.target.value)}
          className="h-8 flex-1"
          placeholder="Spoken phrase…"
          onKeyDown={(e) => {
            if (e.key === "Enter") onSubmitEdit();
            if (e.key === "Escape") onCancelEdit();
          }}
          autoFocus
        />
      ) : (
        <>
          <Kbd className="shrink-0">{cmd.phrase}</Kbd>
          <span className="flex-1 truncate text-[13px] text-text-secondary">
            {actionLabel(cmd.action)}
          </span>
        </>
      )}

      {isEditing ? (
        <div className="flex shrink-0 items-center gap-1">
          <Button size="sm" variant="primary" icon={<Check />} onClick={onSubmitEdit}>
            Save
          </Button>
          <Button size="sm" variant="ghost" icon={<X />} onClick={onCancelEdit}>
            Cancel
          </Button>
        </div>
      ) : (
        <>
          <Segmented
            options={SCOPE_OPTIONS}
            value={cmd.trigger_scope}
            onChange={onScopeChange}
            className="shrink-0"
          />
          <div className="flex w-[52px] shrink-0 items-center justify-end gap-0.5 opacity-0 transition-opacity group-hover:opacity-100 focus-within:opacity-100">
            <button
              onClick={onStartEdit}
              title="Rename phrase"
              className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
            >
              <Pencil size={13} />
            </button>
            {!cmd.built_in && (
              <button
                onClick={onDelete}
                title="Delete"
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-recording-500/10 hover:text-recording-400"
              >
                <Trash2 size={13} />
              </button>
            )}
          </div>
        </>
      )}
    </div>
  );
}

export function VoiceCommandsPage() {
  const [commands, setCommands] = useState<VoiceCommand[]>([]);

  // Inline add (custom command)
  const [adding, setAdding] = useState(false);
  const [newPhrase, setNewPhrase] = useState("");
  const [newActionType, setNewActionType] = useState<ActionType>("key");
  const [newCombo, setNewCombo] = useState("");
  const [newMouse, setNewMouse] = useState("mouse:click");
  const [newLaunch, setNewLaunch] = useState("");
  const phraseRef = useRef<HTMLInputElement>(null);

  // Inline edit (phrase only)
  const [editId, setEditId] = useState<string | null>(null);
  const [editPhrase, setEditPhrase] = useState("");

  const load = useCallback(() => {
    listVoiceCommands()
      .then(setCommands)
      .catch((e) => console.error("Failed to load voice commands:", e));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (adding) phraseRef.current?.focus();
  }, [adding]);

  const resetAddForm = useCallback(() => {
    setNewPhrase("");
    setNewCombo("");
    setNewLaunch("");
    setNewMouse("mouse:click");
    setNewActionType("key");
    setAdding(false);
  }, []);

  const handleAdd = useCallback(() => {
    const p = newPhrase.trim();
    if (!p) return;

    // Build the stored action string and default scope from the chosen type.
    // Mouse commands default to end-of-utterance as a false-trigger guard.
    let action: string;
    let scope: TriggerScope = "anywhere";
    if (newActionType === "mouse") {
      action = newMouse;
      scope = "end_of_utterance";
    } else if (newActionType === "launch") {
      const cl = newLaunch.trim();
      if (!cl) return;
      action = `launch:${cl}`;
    } else {
      const combo = newCombo.trim();
      if (!combo) return;
      action = combo.startsWith("key:") ? combo : `key:${combo}`;
    }

    addVoiceCommand(p, action, scope)
      .then((cmd) => {
        setCommands((prev) => [...prev, cmd]);
        resetAddForm();
      })
      .catch((e) => console.error("Failed to add voice command:", e));
  }, [newPhrase, newActionType, newCombo, newMouse, newLaunch, resetAddForm]);

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

  // Group commands for visual hierarchy: editing keystrokes, the opt-in
  // mouse/window controls, and the user's own custom commands.
  const groups = useMemo(() => {
    const editing: VoiceCommand[] = [];
    const pointer: VoiceCommand[] = [];
    const custom: VoiceCommand[] = [];
    for (const cmd of commands) {
      if (!cmd.built_in) custom.push(cmd);
      else if (cmd.action.startsWith("mouse:") || cmd.action.startsWith("key:"))
        pointer.push(cmd);
      else editing.push(cmd);
    }
    return { editing, pointer, custom };
  }, [commands]);

  const renderRow = (cmd: VoiceCommand) => (
    <CommandRow
      key={cmd.id}
      cmd={cmd}
      isEditing={editId === cmd.id}
      editPhrase={editPhrase}
      onEditPhraseChange={setEditPhrase}
      onSubmitEdit={() => handleUpdatePhrase(cmd)}
      onCancelEdit={() => setEditId(null)}
      onStartEdit={() => startEdit(cmd)}
      onToggleEnabled={(enabled) => patch(cmd, { enabled })}
      onScopeChange={(scope) => patch(cmd, { trigger_scope: scope })}
      onDelete={() => handleDelete(cmd.id)}
    />
  );

  return (
    <div className="flex h-full flex-col overflow-y-auto px-8 pt-6 pb-10">
      {/* Header */}
      <div
        className="flex items-start justify-between animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <div>
          <h1 className="font-display text-2xl font-semibold tracking-[-0.025em] text-text-primary">
            Voice Commands
          </h1>
          <p className="mt-1 text-sm text-text-muted">
            Spoken triggers that run keystrokes while you dictate — enable, re-scope, or add your own.
          </p>
        </div>
        <Button variant="ghost" size="sm" icon={<RotateCcw />} onClick={handleReset}>
          Reset to defaults
        </Button>
      </div>

      <div className="mt-6 flex max-w-3xl flex-col gap-4">
        {/* Editing keystrokes */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Keyboard size={15} className="text-text-muted" />
              <CardTitle>Editing</CardTitle>
              <Badge tone="neutral">{groups.editing.length}</Badge>
            </div>
          </CardHeader>
          <CardBody className="divide-y divide-border pt-0">
            {groups.editing.map(renderRow)}
          </CardBody>
        </Card>

        {/* Mouse & window (opt-in) */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <MousePointerClick size={15} className="text-text-muted" />
              <CardTitle>Mouse &amp; window</CardTitle>
              <Badge tone="neutral">{groups.pointer.length}</Badge>
            </div>
            <span className="text-[11.5px] text-text-muted">Off by default</span>
          </CardHeader>
          <CardBody className="divide-y divide-border pt-0">
            {groups.pointer.map(renderRow)}
          </CardBody>
        </Card>

        {/* Custom commands */}
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <CommandIcon size={15} className="text-text-muted" />
              <CardTitle>Custom</CardTitle>
              {groups.custom.length > 0 && <Badge tone="amber">{groups.custom.length}</Badge>}
            </div>
            {!adding && (
              <Button size="sm" variant="secondary" icon={<Plus />} onClick={() => setAdding(true)}>
                Add command
              </Button>
            )}
          </CardHeader>
          <CardBody className="pt-0">
            {groups.custom.length > 0 && (
              <div className="divide-y divide-border">
                {groups.custom.map(renderRow)}
              </div>
            )}

            {!adding && groups.custom.length === 0 && (
              <EmptyState
                className="py-8"
                icon={<CommandIcon />}
                title="No custom commands yet"
                description="Add a spoken phrase that fires a key combo, a mouse action, or launches an app."
              />
            )}

            {adding && (
              <div className="mt-1 rounded-xl border border-amber-500/30 bg-surface-2/60 p-3">
                <div className="flex flex-wrap items-center gap-2">
                  <Input
                    ref={phraseRef}
                    value={newPhrase}
                    onChange={(e) => setNewPhrase(e.target.value)}
                    className="h-8 w-44"
                    placeholder="Spoken phrase…"
                    onKeyDown={(e) => e.key === "Escape" && resetAddForm()}
                  />
                  <Segmented
                    options={[
                      { value: "key", label: "Key", icon: <Keyboard /> },
                      { value: "mouse", label: "Mouse", icon: <MousePointerClick /> },
                      { value: "launch", label: "Launch", icon: <Rocket /> },
                    ]}
                    value={newActionType}
                    onChange={(v) => setNewActionType(v as ActionType)}
                  />

                  {newActionType === "key" && (
                    <Input
                      value={newCombo}
                      onChange={(e) => setNewCombo(e.target.value)}
                      className="h-8 w-40 font-mono"
                      placeholder="ctrl+shift+k"
                      onKeyDown={(e) => e.key === "Enter" && handleAdd()}
                    />
                  )}
                  {newActionType === "mouse" && (
                    <Segmented
                      options={Object.entries(MOUSE_LABELS).map(([value, label]) => ({
                        value,
                        label,
                      }))}
                      value={newMouse}
                      onChange={setNewMouse}
                    />
                  )}
                  {newActionType === "launch" && (
                    <Input
                      value={newLaunch}
                      onChange={(e) => setNewLaunch(e.target.value)}
                      className="h-8 w-56 font-mono"
                      placeholder={`notepad "C:\\file.txt"`}
                      onKeyDown={(e) => e.key === "Enter" && handleAdd()}
                    />
                  )}

                  <div className="ml-auto flex items-center gap-1.5">
                    <Button size="sm" variant="primary" icon={<Check />} onClick={handleAdd}>
                      Add
                    </Button>
                    <Button size="sm" variant="ghost" icon={<X />} onClick={resetAddForm}>
                      Cancel
                    </Button>
                  </div>
                </div>
                {newActionType === "launch" && (
                  <p className="mt-2 text-[11.5px] leading-snug text-text-muted">
                    Runs this program directly with its arguments — no shell, so variables and
                    metacharacters are not interpreted.
                  </p>
                )}
              </div>
            )}
          </CardBody>
        </Card>
      </div>
    </div>
  );
}
