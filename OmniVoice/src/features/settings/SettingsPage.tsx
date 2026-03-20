import { useState } from "react";
import { Mic, Keyboard, Info, Volume2, Type, Clipboard } from "lucide-react";

const outputModes = [
  { id: "clipboard", label: "Clipboard", icon: Clipboard },
  { id: "type", label: "Type", icon: Type },
  { id: "both", label: "Both", icon: null },
] as const;

type OutputMode = (typeof outputModes)[number]["id"];

const hotkeyKeys = ["Ctrl", "Win"];

export function SettingsPage() {
  const [activeMode, setActiveMode] = useState<OutputMode>("clipboard");

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
            {/* Device selector */}
            <div>
              <label className="block text-sm text-text-secondary mb-1.5">
                Input device
              </label>
              <div className="flex items-center gap-2 bg-surface-2 rounded-lg px-3 py-2 border border-border">
                <Volume2 size={14} strokeWidth={1.75} className="text-text-muted shrink-0" />
                <span className="text-sm text-text-primary">Default Microphone</span>
              </div>
            </div>

            {/* Sample rate */}
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
                  onClick={() => setActiveMode(id)}
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
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
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

          <div className="flex items-center gap-2">
            {hotkeyKeys.map((key, i) => (
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
        </section>

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
