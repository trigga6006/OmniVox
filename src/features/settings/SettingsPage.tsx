import { useState, useEffect, useCallback, useRef } from "react";
import { Mic, Keyboard, Info, Volume2, Type, Clipboard, RotateCcw, Sparkles, Download, Loader2, Zap, Sun, Moon } from "lucide-react";
import {
  getSettings,
  updateSettings,
  suspendHotkey,
  updateHotkey,
  getAudioDevices,
  setAudioDevice,
  getAiCleanupStatus,
  enableAiCleanup,
  disableAiCleanup,
  downloadLlmModel,
  onDownloadProgress,
  setActiveModel,
  getActiveModel,
  type AppSettings,
  type AudioDevice,
  type HotkeyConfig,
  type DownloadProgress,
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

/* ─────────────────── AI Cleanup Section ────────────────────── */

type AiState = "idle" | "downloading" | "loading" | "ready";

function AiCleanupSection() {
  const [modelDownloaded, setModelDownloaded] = useState(false);
  const [upgradeAvailable, setUpgradeAvailable] = useState(false);
  const [aiState, setAiState] = useState<AiState>("idle");
  const [downloadPercent, setDownloadPercent] = useState(0);
  const [error, setError] = useState<string | null>(null);

  // Check initial status
  useEffect(() => {
    getAiCleanupStatus()
      .then((s) => {
        setModelDownloaded(s.model_downloaded);
        setUpgradeAvailable(s.upgrade_available);
        if (s.model_loaded) setAiState("ready");
      })
      .catch((e) => setError(String(e)));
  }, []);

  // Listen for LLM download progress events
  useEffect(() => {
    const unlisten = onDownloadProgress((progress: DownloadProgress) => {
      if (progress.model_id !== "llm-qwen3-1.7b") return;
      setDownloadPercent(Math.round(progress.progress_percent));
      if (progress.status === "Completed") {
        setModelDownloaded(true);
        setAiState("idle");
      }
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  const handleDownload = useCallback(async () => {
    setError(null);
    setAiState("downloading");
    setDownloadPercent(0);
    try {
      await downloadLlmModel();
      // download-progress event listener handles the rest
    } catch (e) {
      setError(String(e));
      setAiState("idle");
    }
  }, []);

  const handleEnable = useCallback(async () => {
    setError(null);
    setAiState("loading");
    try {
      await enableAiCleanup();
      setAiState("ready");
    } catch (e) {
      setError(String(e));
      setAiState("idle");
    }
  }, []);

  const handleDisable = useCallback(async () => {
    setError(null);
    try {
      await disableAiCleanup();
      setAiState("idle");
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const isActive = aiState === "ready";
  const isBusy = aiState === "downloading" || aiState === "loading";

  return (
    <section
      className={cn(
        "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
        isActive
          ? "border-amber-500/20"
          : "border-border hover:border-border-hover"
      )}
      style={{ opacity: 0, animationDelay: "0.22s", animationFillMode: "forwards" }}
    >
      <div className="flex items-center gap-2 mb-3">
        <Sparkles size={14} strokeWidth={2} className="text-text-muted" />
        <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
          AI Cleanup
        </span>
      </div>

      <p className="text-xs text-text-muted mb-4 max-w-[400px]">
        Uses a local AI model (Qwen3-1.7B) to remove filler words, fix grammar, and
        polish transcriptions. Runs entirely on your device.
      </p>

      {/* Upgrade hint when old model exists but new one hasn't been downloaded */}
      {upgradeAvailable && !modelDownloaded && aiState !== "downloading" && (
        <p className="text-xs text-amber-400/80 mb-3 flex items-center gap-1.5">
          <Sparkles size={12} strokeWidth={2} />
          New improved model available — download to upgrade.
        </p>
      )}

      {/* Step 1: Download model */}
      {!modelDownloaded && aiState !== "downloading" && (
        <button
          onClick={handleDownload}
          disabled={isBusy}
          className="inline-flex items-center gap-2 rounded-lg bg-amber-500/15 border border-amber-500/30 px-4 py-2 text-sm font-medium text-amber-400 hover:bg-amber-500/25 transition-colors disabled:opacity-50"
        >
          <Download size={14} strokeWidth={2} />
          Download Model (~1.2 GB)
        </button>
      )}

      {/* Downloading progress */}
      {aiState === "downloading" && (
        <div>
          <div className="flex items-center gap-2 mb-2">
            <Download size={14} strokeWidth={2} className="text-amber-400 animate-pulse" />
            <span className="text-sm text-amber-400">
              Downloading... {downloadPercent}%
            </span>
          </div>
          <div className="w-full bg-surface-3 rounded-full h-2">
            <div
              className="bg-amber-500 h-2 rounded-full transition-all duration-300"
              style={{ width: `${downloadPercent}%` }}
            />
          </div>
        </div>
      )}

      {/* Step 2: Enable / Disable */}
      {modelDownloaded && aiState === "idle" && (
        <button
          onClick={handleEnable}
          className="inline-flex items-center gap-2 rounded-lg bg-amber-500/15 border border-amber-500/30 px-4 py-2 text-sm font-medium text-amber-400 hover:bg-amber-500/25 transition-colors"
        >
          <Sparkles size={14} strokeWidth={2} />
          Enable AI Cleanup
        </button>
      )}

      {aiState === "loading" && (
        <div className="flex items-center gap-2">
          <Loader2 size={14} strokeWidth={2} className="text-amber-400 animate-spin" />
          <span className="text-sm text-amber-400">Loading model into memory...</span>
        </div>
      )}

      {aiState === "ready" && (
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-1.5">
            <div className="h-2 w-2 rounded-full bg-green-500" />
            <span className="text-sm text-green-400">Active</span>
          </div>
          <button
            onClick={handleDisable}
            className="text-xs text-text-muted hover:text-text-secondary transition-colors"
          >
            Disable
          </button>
        </div>
      )}

      {/* Error display */}
      {error && (
        <p className="mt-3 text-xs text-red-400 bg-red-500/10 rounded-md px-3 py-2 border border-red-500/20">
          {error}
        </p>
      )}
    </section>
  );
}

/* ─────────────────── Main Settings Page ─────────────────────── */

export function SettingsPage() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [activeMode, setActiveMode] = useState<OutputMode>("clipboard");
  const [audioDevices, setAudioDevices] = useState<AudioDevice[]>([]);
  const [selectedDeviceId, setSelectedDeviceId] = useState<string | null>(null);
  const [deviceMenuOpen, setDeviceMenuOpen] = useState(false);

  useEffect(() => {
    getSettings()
      .then((s) => {
        setSettings(s);
        const mode = outputModes.find((m) => m.id === s.output_mode);
        setActiveMode(mode ? mode.id : "clipboard");
      })
      .catch((e) => console.error("Failed to load settings:", e));

    getAudioDevices()
      .then((devices) => {
        setAudioDevices(devices);
        const def = devices.find((d) => d.is_default);
        setSelectedDeviceId(def?.id ?? devices[0]?.id ?? null);
      })
      .catch((e) => console.error("Failed to load audio devices:", e));
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

        {/* ── GPU Acceleration ── */}
        <GpuAccelerationSection
          enabled={settings?.gpu_acceleration ?? false}
          onToggle={handleGpuToggle}
        />

        {/* ── AI Cleanup ── */}
        <AiCleanupSection />

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
