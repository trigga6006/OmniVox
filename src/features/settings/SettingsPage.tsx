import { useState, useEffect, useCallback, useRef } from "react";
import { Mic, Keyboard, Info, Volume2, Type, Clipboard, RotateCcw, Loader2, Zap, Sun, Moon, Eye, ShieldCheck, Layers, X, Rocket, PenLine, ExternalLink } from "lucide-react";
import {
  getSettings,
  updateSettings,
  suspendHotkey,
  updateHotkey,
  getAudioDevices,
  setAudioDevice,
  setActiveModel,
  getActiveModel,
  onSettingsChanged,
  getPlatformInfo,
  openMicSettings,
  openAccessibilitySettings,
  type AppSettings,
  type AudioDevice,
  type HotkeyConfig,
  type PlatformInfo,
} from "@/lib/tauri";
import { CODE_TO_VK } from "@/lib/vk-codes";
import { cn } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settingsStore";

const outputModes = [
  { id: "clipboard", label: "Clipboard", icon: Clipboard },
  { id: "type_simulation", label: "Type", icon: Type },
  { id: "both", label: "Both", icon: null },
] as const;

type OutputMode = (typeof outputModes)[number]["id"];

const writingStyles = [
  { id: "formal", label: "Formal" },
  { id: "casual", label: "Casual" },
  { id: "very_casual", label: "Very Casual" },
] as const;

type WritingStyleId = (typeof writingStyles)[number]["id"];

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
      style={{ opacity: 0, animationDelay: "0.32s", animationFillMode: "forwards" }}
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

/* ─────────────────── GPU Acceleration Section ─────────────── */

function GpuAccelerationSection({
  enabled,
  onToggle,
}: {
  enabled: boolean;
  onToggle: (enabled: boolean) => void;
}) {
  const [reloading, setReloading] = useState(false);

  const handleToggle = useCallback(async () => {
    const next = !enabled;
    setReloading(true);
    onToggle(next);

    // Reload the active Whisper model so it picks up the new GPU setting.
    try {
      const active = await getActiveModel();
      if (active) {
        await setActiveModel(active.id);
      }
    } catch (e) {
      console.error("Failed to reload model after GPU toggle:", e);
    } finally {
      setReloading(false);
    }
  }, [enabled, onToggle]);

  return (
    <section
      className={cn(
        "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
        enabled
          ? "border-amber-500/20"
          : "border-border hover:border-border-hover"
      )}
      style={{ opacity: 0, animationDelay: "0.19s", animationFillMode: "forwards" }}
    >
      <div className="flex items-center gap-2 mb-3">
        <Zap size={14} strokeWidth={2} className="text-text-muted" />
        <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
          GPU Acceleration
        </span>
      </div>

      <p className="text-xs text-text-muted mb-4 max-w-[400px]">
        Offload Whisper inference to your GPU via Vulkan for significantly faster
        transcription. Works with both AMD and NVIDIA GPUs.
      </p>

      <div className="flex items-center gap-3">
        <button
          onClick={handleToggle}
          disabled={reloading}
          className={cn(
            "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
            enabled ? "bg-amber-500" : "bg-surface-3",
            reloading && "opacity-60"
          )}
        >
          <span
            className={cn(
              "inline-block h-4 w-4 rounded-full bg-white transition-transform",
              enabled ? "translate-x-6" : "translate-x-1"
            )}
          />
        </button>
        <span className="text-sm text-text-secondary">
          {reloading ? (
            <span className="flex items-center gap-1.5">
              <Loader2 size={13} strokeWidth={2} className="animate-spin text-amber-400" />
              Reloading model...
            </span>
          ) : enabled ? (
            "Enabled"
          ) : (
            "Disabled"
          )}
        </span>
      </div>
    </section>
  );
}

/* ─────────────────── Main Settings Page ─────────────────────── */

export function SettingsPage() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [activeMode, setActiveMode] = useState<OutputMode>("clipboard");
  const [activeStyle, setActiveStyle] = useState<WritingStyleId>("formal");
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [deviceMenuOpen, setDeviceMenuOpen] = useState(false);
  const [showVoiceCommands, setShowVoiceCommands] = useState(false);
  const [platformInfo, setPlatformInfo] = useState<PlatformInfo | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setSettings(s);
        const mode = outputModes.find((m) => m.id === s.output_mode);
        setActiveMode(mode ? mode.id : "clipboard");
        const style = writingStyles.find((st) => st.id === s.writing_style);
        setActiveStyle(style ? style.id : "formal");
      })
      .catch((e) => console.error("Failed to load settings:", e));

    getAudioDevices()
      .then((devices) => {
        setAudioDevices(devices);
        const def = devices.find((d) => d.is_default);
        setSelectedDeviceId(def?.id ?? devices[0]?.id ?? null);
      })
      .catch((e) => console.error("Failed to load audio devices:", e));

    getPlatformInfo()
      .then(setPlatformInfo)
      .catch(() => {});

    // Stay in sync when settings change from the overlay pill (or any window)
    const unlisten = onSettingsChanged((s) => {
      setSettings(s);
      const mode = outputModes.find((m) => m.id === s.output_mode);
      setActiveMode(mode ? mode.id : "clipboard");
      const style = writingStyles.find((st) => st.id === s.writing_style);
      setActiveStyle(style ? style.id : "formal");
    });
    return () => { unlisten.then((fn) => fn()); };
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

  const handleStyleChange = useCallback(
    (style: WritingStyleId) => {
      setActiveStyle(style);
      if (!settings) return;

      const updated: AppSettings = { ...settings, writing_style: style };
      setSettings(updated);
      updateSettings(updated).catch((e) =>
        console.error("Failed to save settings:", e)
      );
    },
    [settings]
  );

  const handleGpuToggle = useCallback(
    (enabled: boolean) => {
      if (!settings) return;

      const updated: AppSettings = { ...settings, gpu_acceleration: enabled };
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

  const handleLivePreviewToggle = useCallback(() => {
    if (!settings) return;
    const updated = { ...settings, live_preview: !settings.live_preview };
    setSettings(updated);
    updateSettings(updated).catch(console.error);
  }, [settings]);

  const handleNoiseReductionToggle = useCallback(() => {
    if (!settings) return;
    const updated = { ...settings, noise_reduction: !settings.noise_reduction };
    setSettings(updated);
    updateSettings(updated).catch(console.error);
  }, [settings]);

  const handleVoiceCommandsToggle = useCallback(() => {
    if (!settings) return;
    const updated = { ...settings, voice_commands: !settings.voice_commands };
    setSettings(updated);
    updateSettings(updated).catch(console.error);
  }, [settings]);

  const handleAutoSwitchToggle = useCallback(() => {
    if (!settings) return;
    const updated = { ...settings, auto_switch_modes: !settings.auto_switch_modes };
    setSettings(updated);
    updateSettings(updated).catch(console.error);
  }, [settings]);

  const handleShipModeToggle = useCallback(() => {
    if (!settings) return;
    const updated = { ...settings, ship_mode: !settings.ship_mode };
    setSettings(updated);
    updateSettings(updated).catch(console.error);
  }, [settings]);

  const currentTheme = settings?.theme ?? "dark";
  const handleThemeChange = useCallback(
    (theme: string) => {
      if (!settings) return;
      const updated = { ...settings, theme };
      setSettings(updated);
      updateSettings(updated).catch(console.error);
      useSettingsStore.getState().setSettings({ theme });
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
        <h1 className="font-display font-semibold text-2xl text-text-primary">Settings</h1>
        <p className="text-sm text-text-muted mt-1">Configuration</p>
      </div>

      <div className="mt-6 flex flex-col gap-5">
        {/* ── Appearance ── */}
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
          style={{ opacity: 0, animationDelay: "0.08s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Sun size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Appearance
            </span>
          </div>

          <label className="block text-sm text-text-secondary mb-2">Theme</label>
          <div className="inline-flex gap-1 bg-surface-2 rounded-lg p-1">
            {([
              { id: "dark", label: "Dark", Icon: Moon },
              { id: "light", label: "Light", Icon: Sun },
            ] as const).map(({ id, label, Icon }) => {
              const isActive = currentTheme === id;
              return (
                <button
                  key={id}
                  onClick={() => handleThemeChange(id)}
                  className={cn(
                    "inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
                    isActive
                      ? "bg-amber-500/15 text-amber-400 border border-amber-500/30"
                      : "text-text-muted hover:text-text-secondary border border-transparent"
                  )}
                >
                  <Icon size={14} strokeWidth={1.75} />
                  {label}
                </button>
              );
            })}
          </div>
        </section>

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
            <div className="relative">
              <label className="block text-sm text-text-secondary mb-1.5">
                Input device
              </label>
              <button
                onClick={() => setDeviceMenuOpen((p) => !p)}
                className="flex items-center gap-2 w-full bg-surface-2 rounded-lg px-3 py-2 border border-border hover:border-border-hover transition-colors text-left"
              >
                <Volume2 size={14} strokeWidth={1.75} className="text-text-muted shrink-0" />
                <span className="text-sm text-text-primary truncate flex-1">
                  {audioDevices.find((d) => d.id === selectedDeviceId)?.name ?? "Default Microphone"}
                </span>
                <svg width="12" height="12" viewBox="0 0 12 12" className="text-text-muted shrink-0">
                  <path d="M3 4.5L6 7.5L9 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </button>

              {deviceMenuOpen && audioDevices.length > 0 && (
                <div className="absolute z-10 left-0 right-0 mt-1 bg-surface-1 border border-border rounded-lg shadow-xl overflow-hidden">
                  {audioDevices.map((device) => {
                    const isActive = device.id === selectedDeviceId;
                    return (
                      <button
                        key={device.id}
                        onClick={() => {
                          setSelectedDeviceId(device.id);
                          setDeviceMenuOpen(false);
                          setAudioDevice(device.id).catch(console.error);
                        }}
                        className={cn(
                          "flex items-center gap-2 w-full px-3 py-2 text-left text-sm transition-colors",
                          isActive
                            ? "bg-amber-500/10 text-amber-400"
                            : "text-text-primary hover:bg-surface-2"
                        )}
                      >
                        <Volume2 size={13} strokeWidth={1.75} className={isActive ? "text-amber-400" : "text-text-muted"} />
                        <span className="truncate">{device.name}</span>
                        {device.is_default && (
                          <span className="text-[10px] text-text-muted ml-auto shrink-0">Default</span>
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>

            <div>
              <label className="block text-sm text-text-secondary mb-1.5">
                Sample rate
              </label>
              <span className="font-mono text-sm text-text-muted">16,000 Hz</span>
            </div>

            {/* macOS permission buttons */}
            {platformInfo?.os === "macos" && (
              <div className="border-t border-border pt-3 mt-1 flex flex-col gap-2">
                <p className="text-[11px] text-text-muted">
                  macOS requires explicit permission for microphone access and global hotkeys.
                </p>
                <div className="flex gap-2">
                  <button
                    onClick={() => openMicSettings().catch(console.error)}
                    className="flex items-center gap-1.5 rounded-md bg-surface-2 border border-border px-2.5 py-1.5 text-xs text-text-secondary hover:text-text-primary hover:border-border-hover transition-colors"
                  >
                    <Mic size={12} />
                    Microphone Access
                    <ExternalLink size={10} className="opacity-50" />
                  </button>
                  <button
                    onClick={() => openAccessibilitySettings().catch(console.error)}
                    className="flex items-center gap-1.5 rounded-md bg-surface-2 border border-border px-2.5 py-1.5 text-xs text-text-secondary hover:text-text-primary hover:border-border-hover transition-colors"
                  >
                    <Keyboard size={12} />
                    Accessibility
                    <ExternalLink size={10} className="opacity-50" />
                  </button>
                </div>
              </div>
            )}
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

        {/* ── Writing Style ── */}
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
          style={{ opacity: 0, animationDelay: "0.185s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <PenLine size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Writing Style
            </span>
          </div>

          <p className="text-xs text-text-muted mb-4 max-w-[400px]">
            Controls capitalization and punctuation in your transcriptions.
            This is the default style — context modes can override it.
          </p>

          <div className="inline-flex gap-1 bg-surface-2 rounded-lg p-1">
            {writingStyles.map(({ id, label }) => {
              const isActive = activeStyle === id;
              return (
                <button
                  key={id}
                  onClick={() => handleStyleChange(id)}
                  className={`inline-flex items-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium transition-colors ${
                    isActive
                      ? "bg-amber-500/15 text-amber-400 border border-amber-500/30"
                      : "text-text-muted hover:text-text-secondary border border-transparent"
                  }`}
                >
                  {label}
                </button>
              );
            })}
          </div>
        </section>

        {/* ── Live Preview ── */}
        <section
          className={cn(
            "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
            settings?.live_preview
              ? "border-amber-500/20"
              : "border-border hover:border-border-hover"
          )}
          style={{ opacity: 0, animationDelay: "0.19s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Eye size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Live Preview
            </span>
          </div>

          <p className="text-xs text-text-muted mb-4 max-w-[400px]">
            Show live transcription words in the floating pill while recording.
            <span className="text-amber-400/70"> Adds latency</span> — runs
            inference during recording, which can delay the final transcription.
          </p>

          <div className="flex items-center gap-3">
            <button
              onClick={handleLivePreviewToggle}
              className={cn(
                "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
                settings?.live_preview ? "bg-amber-500" : "bg-surface-3"
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white transition-transform",
                  settings?.live_preview ? "translate-x-6" : "translate-x-1"
                )}
              />
            </button>
            <span className="text-sm text-text-secondary">
              {settings?.live_preview ? "Enabled" : "Disabled"}
            </span>
          </div>
        </section>

        {/* ── Noise Reduction ── */}
        <section
          className={cn(
            "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
            settings?.noise_reduction
              ? "border-amber-500/20"
              : "border-border hover:border-border-hover"
          )}
          style={{ opacity: 0, animationDelay: "0.22s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <ShieldCheck size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Noise Reduction
            </span>
          </div>

          <p className="text-xs text-text-muted mb-4 max-w-[400px]">
            Remove background noise from recordings before transcription.
            Filters fan noise, keyboard clicks, and other non-speech sounds
            using RNNoise.
          </p>

          <div className="flex items-center gap-3">
            <button
              onClick={handleNoiseReductionToggle}
              className={cn(
                "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
                settings?.noise_reduction ? "bg-amber-500" : "bg-surface-3"
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white transition-transform",
                  settings?.noise_reduction ? "translate-x-6" : "translate-x-1"
                )}
              />
            </button>
            <span className="text-sm text-text-secondary">
              {settings?.noise_reduction ? "Enabled" : "Disabled"}
            </span>
          </div>
        </section>

        {/* ── Voice Commands ── */}
        <section
          className={cn(
            "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
            settings?.voice_commands
              ? "border-amber-500/20"
              : "border-border hover:border-border-hover"
          )}
          style={{ opacity: 0, animationDelay: "0.25s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Mic size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Voice Commands
            </span>
          </div>

          <p className="text-xs text-text-muted mb-4 max-w-[400px]">
            Recognize spoken commands during dictation. Say "new line" for a line
            break, "new paragraph" for a paragraph break, or "delete last word"
            to remove the previous word.
          </p>

          <div className="flex items-center gap-3">
            <button
              onClick={handleVoiceCommandsToggle}
              className={cn(
                "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
                settings?.voice_commands ? "bg-amber-500" : "bg-surface-3"
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white transition-transform",
                  settings?.voice_commands ? "translate-x-6" : "translate-x-1"
                )}
              />
            </button>
            <span className="text-sm text-text-secondary">
              {settings?.voice_commands ? "Enabled" : "Disabled"}
            </span>

            <button
              onClick={() => setShowVoiceCommands(true)}
              className="ml-auto text-xs text-text-muted hover:text-text-secondary transition-colors flex items-center gap-1"
            >
              <Info size={12} />
              View commands
            </button>
          </div>
        </section>

        {/* ── Voice Commands Reference Popup ── */}
        {showVoiceCommands && (
          <div
            className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm animate-fade-in"
            onClick={() => setShowVoiceCommands(false)}
          >
            <div
              className="bg-surface-1 border border-border rounded-2xl p-6 w-full max-w-sm shadow-2xl animate-slide-up"
              style={{ animationDuration: "0.15s" }}
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-center justify-between mb-5">
                <div className="flex items-center gap-2">
                  <Mic size={16} strokeWidth={2} className="text-amber-500" />
                  <h3 className="text-sm font-semibold text-text-primary">Voice Commands</h3>
                </div>
                <button
                  onClick={() => setShowVoiceCommands(false)}
                  className="text-text-muted hover:text-text-secondary transition-colors p-1 rounded-lg hover:bg-surface-2"
                >
                  <X size={14} />
                </button>
              </div>

              <div className="space-y-3">
                {[
                  { phrase: "new line", desc: "Insert a line break" },
                  { phrase: "new paragraph", desc: "Insert a paragraph break" },
                  { phrase: "delete last word", desc: "Remove the previous word" },
                ].map((cmd) => (
                  <div
                    key={cmd.phrase}
                    className="flex items-center justify-between gap-3 p-3 rounded-lg bg-surface-2/50 border border-border/50"
                  >
                    <div>
                      <span className="text-xs font-mono font-medium text-amber-500">
                        "{cmd.phrase}"
                      </span>
                      <p className="text-xs text-text-muted mt-0.5">{cmd.desc}</p>
                    </div>
                  </div>
                ))}
              </div>

              <p className="text-[10px] text-text-muted mt-4 text-center">
                Speak these phrases naturally during dictation
              </p>
            </div>
          </div>
        )}

        {/* ── Auto Context Switching ── */}
        <section
          className={cn(
            "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
            settings?.auto_switch_modes
              ? "border-amber-500/20"
              : "border-border hover:border-border-hover"
          )}
          style={{ opacity: 0, animationDelay: "0.25s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Layers size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Auto Context Switching
            </span>
          </div>

          <p className="text-xs text-text-muted mb-4 max-w-[400px]">
            Automatically switch context mode based on which application is
            focused when recording starts. Bind apps to modes in the Context
            Modes editor.
          </p>

          <div className="flex items-center gap-3">
            <button
              onClick={handleAutoSwitchToggle}
              className={cn(
                "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
                settings?.auto_switch_modes ? "bg-amber-500" : "bg-surface-3"
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white transition-transform",
                  settings?.auto_switch_modes ? "translate-x-6" : "translate-x-1"
                )}
              />
            </button>
            <span className="text-sm text-text-secondary">
              {settings?.auto_switch_modes ? "Enabled" : "Disabled"}
            </span>
          </div>
        </section>

        {/* ── Ship Mode ── */}
        <section
          className={cn(
            "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
            settings?.ship_mode
              ? "border-amber-500/20"
              : "border-border hover:border-border-hover"
          )}
          style={{ opacity: 0, animationDelay: "0.28s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Rocket size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Ship Mode
            </span>
            <span className="text-[10px] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded bg-amber-500/15 text-amber-400 border border-amber-500/25">
              Beta
            </span>
          </div>

          <p className="text-xs text-text-muted mb-4 max-w-[400px]">
            Automatically sends your transcription by pressing Enter after
            output. Built for agentic workflows — Claude Code, Cursor, and
            similar tools where hands-free submit keeps you in flow.
            Requires Type Simulation or Both output mode.
          </p>

          <p className="text-[11px] text-amber-400/70 mb-4 max-w-[400px]">
            Your message will be sent immediately after transcription — there
            is no chance to edit before it goes out.
          </p>

          <div className="flex items-center gap-3">
            <button
              onClick={handleShipModeToggle}
              className={cn(
                "relative inline-flex h-6 w-11 items-center rounded-full transition-colors",
                settings?.ship_mode ? "bg-amber-500" : "bg-surface-3"
              )}
            >
              <span
                className={cn(
                  "inline-block h-4 w-4 rounded-full bg-white transition-transform",
                  settings?.ship_mode ? "translate-x-6" : "translate-x-1"
                )}
              />
            </button>
            <span className="text-sm text-text-secondary">
              {settings?.ship_mode ? "Enabled" : "Disabled"}
            </span>
          </div>
        </section>

        {/* ── GPU Acceleration ── */}
        <GpuAccelerationSection
          enabled={settings?.gpu_acceleration ?? false}
          onToggle={handleGpuToggle}
        />

        {/* ── Hotkey ── */}
        <HotkeySection
          hotkey={settings?.hotkey ?? null}
          onSaved={handleHotkeySaved}
        />

        {/* ── About ── */}
        <section
          className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
          style={{ opacity: 0, animationDelay: "0.40s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Info size={14} strokeWidth={2} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              About
            </span>
          </div>

          <p className="text-sm text-text-primary">OmniVox v0.1.6</p>
          <p className="text-xs text-text-muted mt-1">
            Local-first AI dictation
          </p>
        </section>
      </div>
    </div>
  );
}
