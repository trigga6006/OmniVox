import { useState, useEffect, useCallback, useRef } from "react";
import { Mic, Keyboard, Info, Volume2, Type, Clipboard, RotateCcw } from "lucide-react";
import {
  getSettings,
  updateSettings,
  suspendHotkey,
  updateHotkey,
  type AppSettings,
  type HotkeyConfig,
} from "@/lib/tauri";
import { CODE_TO_VK } from "@/lib/vk-codes";
import { cn } from "@/lib/utils";

const outputModes = [
  { id: "clipboard", label: "Clipboard", icon: Clipboard },
  { id: "type_simulation", label: "Type", icon: Type },
  { id: "both", label: "Both", icon: null },
] as const;

type OutputMode = (typeof outputModes)[number]["id"];

/* ─────────────────── Hotkey Recorder Component ─────────────────── */

type HotkeyState = "display" | "listening" | "confirming";

interface CapturedKey {
  code: string;
  vk: number;
  label: string;
}

function HotkeySection({
  hotkey,
  onSaved,
}: {
  hotkey: HotkeyConfig | null;
  onSaved: (config: HotkeyConfig) => void;
}) {
  const [state, setState] = useState<HotkeyState>("display");
  const [captured, setCaptured] = useState<CapturedKey[]>([]);
  const heldRef = useRef<Set<string>>(new Set());

  const currentLabels = hotkey?.labels ?? ["LCtrl", "LAlt"];

  // ── Listening mode: capture keys ──────────────────────────
  useEffect(() => {
    if (state !== "listening") return;

    // Suspend the backend hook so our keypresses don't trigger recording
    suspendHotkey(true).catch(console.error);

    const collected: CapturedKey[] = [];
    heldRef.current = new Set();

    const onDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();

      const code = e.code;
      if (heldRef.current.has(code)) return; // repeat
      heldRef.current.add(code);

      const entry = CODE_TO_VK[code];
      if (!entry) return; // unknown key

      // Max 2 keys
      if (collected.length < 2 && !collected.some((k) => k.code === code)) {
        collected.push({ code, vk: entry.vk, label: entry.label });
        setCaptured([...collected]);
      }
    };

    const onUp = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      heldRef.current.delete(e.code);

      // All keys released → done capturing
      if (heldRef.current.size === 0 && collected.length > 0) {
        setState("confirming");
      }
    };

    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);

    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, [state]);

  const handleRemap = () => {
    setCaptured([]);
    setState("listening");
  };

  const handleRedo = () => {
    setCaptured([]);
    setState("listening");
  };

  const handleCancel = () => {
    setState("display");
    setCaptured([]);
    suspendHotkey(false).catch(console.error);
  };

  const handleSave = () => {
    if (captured.length === 0) return;
    const config: HotkeyConfig = {
      keys: captured.map((k) => k.vk),
      labels: captured.map((k) => k.label),
    };
    updateHotkey(config)
      .then(() => {
        onSaved(config);
        setState("display");
      })
      .catch(console.error);
  };

  const handleReset = () => {
    const config: HotkeyConfig = { keys: [0xa2, 0xa4], labels: ["LCtrl", "LAlt"] };
    updateHotkey(config)
      .then(() => onSaved(config))
      .catch(console.error);
  };

  const isDefault =
    currentLabels.length === 2 &&
    currentLabels[0] === "LCtrl" &&
    currentLabels[1] === "LAlt";

  return (
    <section
      className={cn(
        "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
        state === "listening"
          ? "border-amber-500/40"
          : state === "confirming"
            ? "border-green-500/30"
            : "border-border hover:border-border-hover"
      )}
      style={{ opacity: 0, animationDelay: "0.26s", animationFillMode: "forwards" }}
    >
      <div className="flex items-center gap-2 mb-3">
        <Keyboard size={14} strokeWidth={2} className="text-text-muted" />
        <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
          Hotkey
        </span>
      </div>

      <label className="block text-sm text-text-secondary mb-2">
        Push-to-talk shortcut
      </label>

      {/* ── Display state ── */}
      {state === "display" && (
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            {currentLabels.map((key, i) => (
              <div key={key} className="contents">
                {i > 0 && (
                  <span className="text-xs text-text-muted select-none">+</span>
                )}
                <kbd className="bg-surface-3 rounded-lg px-3 py-1.5 font-mono text-sm text-text-secondary border border-border shadow-sm">
                  {key}
                </kbd>
              </div>
            ))}
          </div>
          <button
            onClick={handleRemap}
            className="ml-2 text-xs font-medium text-amber-400 hover:text-amber-300 transition-colors"
          >
            Remap
          </button>
          {!isDefault && (
            <button
              onClick={handleReset}
              className="text-xs text-text-muted hover:text-text-secondary transition-colors"
            >
              Reset
            </button>
          )}
        </div>
      )}

      {/* ── Listening state ── */}
      {state === "listening" && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <div
              className="flex items-center justify-center gap-2 bg-surface-2 rounded-lg px-4 py-2.5 border border-amber-500/30 min-w-[160px]"
            >
              {captured.length === 0 ? (
                <span className="text-sm text-amber-400/70 animate-pulse">
                  Press your keys...
                </span>
              ) : (
                captured.map((k, i) => (
                  <div key={k.code} className="contents">
                    {i > 0 && (
                      <span className="text-xs text-text-muted">+</span>
                    )}
                    <kbd className="bg-amber-500/15 rounded-md px-2.5 py-1 font-mono text-sm text-amber-400 border border-amber-500/25">
                      {k.label}
                    </kbd>
                  </div>
                ))
              )}
            </div>
            <button
              onClick={handleCancel}
              className="text-xs text-text-muted hover:text-text-secondary transition-colors"
            >
              Cancel
            </button>
          </div>
          <p className="text-xs text-text-muted">
            Press 1 or 2 keys simultaneously, then release to confirm
          </p>
        </div>
      )}

      {/* ── Confirming state ── */}
      {state === "confirming" && (
        <div className="flex flex-col gap-3">
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-2">
              {captured.map((k, i) => (
                <div key={k.code} className="contents">
                  {i > 0 && (
                    <span className="text-xs text-text-muted">+</span>
                  )}
                  <kbd className="bg-green-500/10 rounded-lg px-3 py-1.5 font-mono text-sm text-green-400 border border-green-500/25 shadow-sm">
                    {k.label}
                  </kbd>
                </div>
              ))}
            </div>

            <div className="flex items-center gap-2 ml-2">
              <button
                onClick={handleSave}
                className="inline-flex items-center gap-1 rounded-md bg-amber-500/15 border border-amber-500/30 px-3 py-1 text-xs font-medium text-amber-400 hover:bg-amber-500/20 transition-colors"
              >
                Save
              </button>
              <button
                onClick={handleRedo}
                className="inline-flex items-center gap-1 rounded-md px-2.5 py-1 text-xs text-text-muted hover:text-text-secondary transition-colors"
                title="Try again"
              >
                <RotateCcw size={12} strokeWidth={2} />
                Redo
              </button>
              <button
                onClick={handleCancel}
                className="text-xs text-text-muted hover:text-text-secondary transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}

/* ─────────────────── Main Settings Page ─────────────────────── */

export function SettingsPage() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [activeMode, setActiveMode] = useState<OutputMode>("clipboard");

  useEffect(() => {
    getSettings()
      .then((s) => {
        setSettings(s);
        const mode = outputModes.find((m) => m.id === s.output_mode);
        setActiveMode(mode ? mode.id : "clipboard");
      })
      .catch((e) => console.error("Failed to load settings:", e));
  }, []);

  const handleModeChange = useCallback(
    (mode: OutputMode) => {
      setActiveMode(mode);
      if (!settings) return;

      const updated: AppSettings = { ...settings, output_mode: mode };
      setSettings(updated);
      updateSettings(updated).catch((e) =>
        console.error("Failed to save settings:", e)
      );
    },
    [settings]
  );

  const handleHotkeySaved = useCallback(
    (config: HotkeyConfig) => {
      if (settings) {
        setSettings({ ...settings, hotkey: config });
      }
    },
    [settings]
  );

  return (
    <div className="flex h-full flex-col p-6 overflow-y-auto">
      {/* Header */}
      <div
        className="animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl text-text-primary">Settings</h1>
        <p className="text-sm text-text-muted mt-1">Configuration</p>
      </div>

      <div className="mt-6 flex flex-col gap-5">
        {/* ── Audio ── */}
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
          style={{ opacity: 0, animationDelay: "0.1s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Mic size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Audio
            </span>
          </div>

          <div className="flex flex-col gap-3">
            <div>
              <label className="block text-sm text-text-secondary mb-1.5">
                Input device
              </label>
              <div className="flex items-center gap-2 bg-surface-2 rounded-lg px-3 py-2 border border-border">
                <Volume2 size={14} strokeWidth={1.75} className="text-text-muted shrink-0" />
                <span className="text-sm text-text-primary">Default Microphone</span>
              </div>
            </div>

            <div>
              <label className="block text-sm text-text-secondary mb-1.5">
                Sample rate
              </label>
              <span className="font-mono text-sm text-text-muted">16,000 Hz</span>
            </div>
          </div>
        </section>

        {/* ── Output ── */}
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
          style={{ opacity: 0, animationDelay: "0.18s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Type size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Output
            </span>
          </div>

          <label className="block text-sm text-text-secondary mb-2">
            Transcription delivery
          </label>

          <div className="inline-flex gap-1 bg-surface-2 rounded-lg p-1">
            {outputModes.map(({ id, label, icon: Icon }) => {
              const isActive = activeMode === id;
              return (
                <button
                  key={id}
                  onClick={() => handleModeChange(id)}
                  className={`inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                    isActive
                      ? "bg-amber-500/15 text-amber-400 border border-amber-500/30"
                      : "text-text-muted hover:text-text-secondary border border-transparent"
                  }`}
                >
                  {Icon && <Icon size={14} strokeWidth={1.75} />}
                  {label}
                </button>
              );
            })}
          </div>
        </section>

        {/* ── Hotkey ── */}
        <HotkeySection
          hotkey={settings?.hotkey ?? null}
          onSaved={handleHotkeySaved}
        />

        {/* ── About ── */}
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
          style={{ opacity: 0, animationDelay: "0.34s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Info size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              About
            </span>
          </div>

          <p className="text-sm text-text-primary">OmniVox v0.1.0</p>
          <p className="text-xs text-text-muted mt-1">
            Local-first AI dictation
          </p>
        </section>
      </div>
    </div>
  );
}
