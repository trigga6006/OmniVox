import { useState, useEffect, useCallback } from "react";
import { Mic, Keyboard, Info, Volume2, VolumeX, Type, Clipboard, Sun, Moon, Eye, ShieldCheck, Layers, X, Rocket, PenLine, ExternalLink, Send, ScanText, Terminal } from "lucide-react";
import { getVersion } from "@tauri-apps/api/app";
import {
  getSettings,
  getAudioDevices,
  setAudioDevice,
  onSettingsChanged,
  getPlatformInfo,
  openMicSettings,
  openAccessibilitySettings,
  type AppSettings,
  type AudioDevice,
  type HotkeyConfig,
  type PlatformInfo,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { useSettingsStore } from "@/stores/settingsStore";
import { useSettingsPatch } from "@/hooks/useSettingsPatch";
import { HotkeySection } from "./HotkeySection";
import { GpuAccelerationSection } from "./GpuAccelerationSection";

const outputModes = [
  {
    id: "clipboard",
    label: "Clipboard",
    icon: Clipboard,
    description: "Copy dictation to the clipboard. Paste it yourself with Ctrl+V.",
  },
  {
    id: "type_simulation",
    label: "Type",
    icon: Type,
    description: "Auto-paste into the focused app, then restore whatever you had on your clipboard.",
  },
  {
    id: "both",
    label: "Both",
    icon: null,
    description: "Auto-paste AND keep dictation on the clipboard so you can paste it again.",
  },
  {
    id: "typing",
    label: "Typing",
    icon: Keyboard,
    description: "Types text directly into the focused app. Doesn't use or change your clipboard.",
  },
] as const;

type OutputMode = (typeof outputModes)[number]["id"];

const writingStyles = [
  { id: "formal", label: "Formal" },
  { id: "casual", label: "Casual" },
  { id: "very_casual", label: "Very Casual" },
] as const;

type WritingStyleId = (typeof writingStyles)[number]["id"];

/* ─────────────────── Reusable settings primitives ─────────────── */

function Toggle({ on, onClick, disabled }: { on: boolean; onClick: () => void; disabled?: boolean }) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={cn(
        "relative inline-flex h-[22px] w-10 shrink-0 items-center rounded-full transition-colors",
        on ? "bg-amber-500" : "bg-surface-3",
        disabled && "opacity-60"
      )}
    >
      <span
        className={cn(
          "inline-block h-[16px] w-[16px] rounded-full bg-white shadow-sm transition-transform duration-200",
          on ? "translate-x-[21px]" : "translate-x-[3px]"
        )}
      />
    </button>
  );
}

function Segmented<T extends string>({
  options,
  value,
  onChange,
}: {
  options: readonly { id: T; label: string; icon?: typeof Mic | null }[];
  value: T;
  onChange: (v: T) => void;
}) {
  return (
    <div className="inline-flex gap-0.5 rounded-lg border border-border bg-surface-2/70 p-0.5">
      {options.map(({ id, label, icon: Icon }) => {
        const active = id === value;
        return (
          <button
            key={id}
            onClick={() => onChange(id)}
            className={cn(
              "inline-flex items-center gap-1.5 rounded-[7px] px-3 py-1.5 text-sm font-medium transition-all duration-150",
              active
                ? "bg-amber-500/[0.14] text-amber-200 shadow-sm ring-1 ring-amber-400/25"
                : "text-text-muted hover:bg-surface-1 hover:text-text-secondary"
            )}
          >
            {Icon && <Icon size={14} strokeWidth={1.75} />}
            {label}
          </button>
        );
      })}
    </div>
  );
}

function GroupCard({
  title,
  delay,
  children,
}: {
  title: string;
  delay: number;
  children: React.ReactNode;
}) {
  return (
    <section
      className="mb-4 break-inside-avoid animate-slide-up rounded-xl border border-border bg-surface-1/85 p-5 transition-colors hover:border-border-hover"
      style={{ opacity: 0, animationDelay: `${delay}s`, animationFillMode: "forwards" }}
    >
      <div className="mb-2.5">
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
          {title}
        </span>
      </div>
      <div className="divide-y divide-border/50">{children}</div>
    </section>
  );
}

function Row({
  icon: Icon,
  title,
  description,
  control,
  children,
}: {
  icon?: typeof Mic;
  title: React.ReactNode;
  description?: React.ReactNode;
  control?: React.ReactNode;
  children?: React.ReactNode;
}) {
  return (
    <div className="py-3.5 first:pt-0 last:pb-0">
      <div className="flex items-start justify-between gap-4">
        <div className="flex min-w-0 items-start gap-2.5">
          {Icon && <Icon size={15} strokeWidth={1.75} className="mt-px shrink-0 text-text-muted" />}
          <div className="min-w-0">
            <div className="text-[13.5px] font-medium text-text-primary">{title}</div>
            {description && (
              <p className="mt-1 text-xs leading-relaxed text-text-muted">{description}</p>
            )}
          </div>
        </div>
        {control && <div className="shrink-0 pt-0.5">{control}</div>}
      </div>
      {children && <div className="mt-3 pl-[26px]">{children}</div>}
    </div>
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
  const { replaceSettings, patchSettings } = useSettingsPatch(setSettings);
  // Version is sourced from tauri.conf.json via the Tauri app API
  // rather than hardcoded — so the About section stays correct across
  // releases without anyone remembering to hand-edit this file.  Null
  // until the async call resolves; the label gracefully falls back to
  // just "OmniVox" in the meantime.
  const [appVersion, setAppVersion] = useState<string | null>(null);

  useEffect(() => {
    getSettings()
      .then((s) => {
        replaceSettings(s);
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
      .catch((e) => console.error("Failed to load platform info:", e));

    getVersion()
      .then(setAppVersion)
      .catch((e) => console.error("Failed to load app version:", e));

    // Stay in sync when settings change from the overlay pill (or any window)
    const unlisten = onSettingsChanged((s) => {
      replaceSettings(s);
      const mode = outputModes.find((m) => m.id === s.output_mode);
      setActiveMode(mode ? mode.id : "clipboard");
      const style = writingStyles.find((st) => st.id === s.writing_style);
      setActiveStyle(style ? style.id : "formal");
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [replaceSettings]);

  const handleModeChange = useCallback(
    (mode: OutputMode) => {
      setActiveMode(mode);
      patchSettings({ output_mode: mode }).catch((e) => {
        console.error("Failed to save settings:", e);
        const previous = outputModes.find((m) => m.id === settings?.output_mode);
        setActiveMode(previous?.id ?? "clipboard");
      });
    },
    [patchSettings, settings?.output_mode]
  );

  const handleStyleChange = useCallback(
    (style: WritingStyleId) => {
      setActiveStyle(style);
      patchSettings({ writing_style: style }).catch((e) => {
        console.error("Failed to save settings:", e);
        const previous = writingStyles.find((st) => st.id === settings?.writing_style);
        setActiveStyle(previous?.id ?? "formal");
      });
    },
    [patchSettings, settings?.writing_style]
  );

  const handleGpuToggle = useCallback(
    (enabled: boolean) => {
      patchSettings({ gpu_acceleration: enabled }).catch((e) =>
        console.error("Failed to save settings:", e)
      );
    },
    [patchSettings]
  );

  const handleHotkeySaved = useCallback(
    (config: HotkeyConfig) => {
      if (settings) {
        replaceSettings({ ...settings, hotkey: config });
      }
    },
    [replaceSettings, settings]
  );

  const handleLivePreviewToggle = useCallback(() => {
    patchSettings((current) => ({ live_preview: !current.live_preview })).catch(console.error);
  }, [patchSettings]);

  const handleNoiseReductionToggle = useCallback(() => {
    patchSettings((current) => ({ noise_reduction: !current.noise_reduction })).catch(console.error);
  }, [patchSettings]);

  const handleScreenContextToggle = useCallback(() => {
    patchSettings((current) => ({ use_screen_context: !current.use_screen_context })).catch(console.error);
  }, [patchSettings]);

  const handleStructuredScreenContextToggle = useCallback(() => {
    patchSettings((current) => ({
      structured_use_screen_context: !current.structured_use_screen_context,
    })).catch(console.error);
  }, [patchSettings]);

  const handleCommandIntentToggle = useCallback(() => {
    patchSettings((current) => ({ command_intent: !current.command_intent })).catch(console.error);
  }, [patchSettings]);

  const handleIntentConfirmToggle = useCallback(() => {
    patchSettings((current) => ({
      intent_confirm_destructive: !current.intent_confirm_destructive,
    })).catch(console.error);
  }, [patchSettings]);

  const handleAudioDuckingToggle = useCallback(() => {
    patchSettings((current) => ({ audio_ducking: !current.audio_ducking })).catch(console.error);
  }, [patchSettings]);

  const handleDuckingAmountChange = useCallback(
    (value: number) => {
      patchSettings({ ducking_amount: value }).catch(console.error);
    },
    [patchSettings]
  );

  const handleVoiceCommandsToggle = useCallback(() => {
    patchSettings((current) => ({ voice_commands: !current.voice_commands })).catch(console.error);
  }, [patchSettings]);

  const handleCommandSendToggle = useCallback(() => {
    patchSettings((current) => ({ command_send: !current.command_send })).catch(console.error);
  }, [patchSettings]);

  const handleAutoSwitchToggle = useCallback(() => {
    patchSettings((current) => ({ auto_switch_modes: !current.auto_switch_modes })).catch(console.error);
  }, [patchSettings]);

  const handleShipModeToggle = useCallback(() => {
    patchSettings((current) => ({ ship_mode: !current.ship_mode })).catch(console.error);
  }, [patchSettings]);

  const currentTheme = settings?.theme ?? "dark";
  const handleThemeChange = useCallback(
    (theme: string) => {
      patchSettings({ theme }).catch(console.error);
      useSettingsStore.getState().setSettings({ theme });
    },
    [patchSettings]
  );

  const selectedDevice =
    audioDevices.find((d) => d.id === selectedDeviceId)?.name ?? "Default Microphone";

  return (
    <div className="flex h-full flex-col overflow-y-auto px-8 pt-6 pb-10">
      {/* Header */}
      <div
        className="animate-slide-up"
        style={{ opacity: 0, animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl font-semibold tracking-[-0.02em] text-text-primary">
          Settings
        </h1>
        <p className="mt-1 text-sm text-text-muted">
          Configure how OmniVox listens, transcribes, and behaves.
        </p>
      </div>

      {/* Two-column masonry — essentials first, width-filling */}
      <div className="mt-5 max-w-5xl gap-x-4 [column-fill:balance] columns-1 lg:columns-2">
        {/* ── Shortcut (most-used control) ── */}
        <div className="mb-4 break-inside-avoid">
          <HotkeySection hotkey={settings?.hotkey ?? null} onSaved={handleHotkeySaved} />
        </div>

        {/* ── Performance ── */}
        <div className="mb-4 break-inside-avoid">
          <GpuAccelerationSection
            enabled={settings?.gpu_acceleration ?? false}
            onToggle={handleGpuToggle}
          />
        </div>

        {/* ── Output ── */}
        <GroupCard title="Output" delay={0.1}>
          <Row
            icon={Send}
            title="Transcription delivery"
            description={outputModes.find((m) => m.id === activeMode)?.description}
          >
            <Segmented options={outputModes} value={activeMode} onChange={handleModeChange} />
          </Row>
          <Row
            icon={PenLine}
            title="Writing style"
            description="Default capitalization and punctuation. Context modes can override this."
          >
            <Segmented options={writingStyles} value={activeStyle} onChange={handleStyleChange} />
          </Row>
        </GroupCard>

        {/* ── Audio ── */}
        <GroupCard title="Audio" delay={0.13}>
          <Row icon={Volume2} title="Input device" description="Sample rate: 16,000 Hz">
            <div className="relative">
              <button
                onClick={() => setDeviceMenuOpen((p) => !p)}
                className="flex w-full items-center gap-2 rounded-lg border border-border bg-surface-2/80 px-3 py-2 text-left transition-colors hover:border-border-hover hover:bg-surface-2"
              >
                <Volume2 size={14} strokeWidth={1.75} className="shrink-0 text-text-muted" />
                <span className="flex-1 truncate text-sm text-text-primary">{selectedDevice}</span>
                <svg width="12" height="12" viewBox="0 0 12 12" className="shrink-0 text-text-muted">
                  <path d="M3 4.5L6 7.5L9 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </button>
              {deviceMenuOpen && audioDevices.length > 0 && (
                <div className="absolute left-0 right-0 z-10 mt-1.5 overflow-hidden rounded-lg border border-border bg-surface-1 shadow-lg backdrop-blur-sm">
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
                          "flex w-full items-center gap-2 px-3 py-2 text-left text-sm transition-colors",
                          isActive ? "bg-amber-500/[0.10] text-amber-300" : "text-text-primary hover:bg-surface-2/80"
                        )}
                      >
                        <Volume2 size={13} strokeWidth={1.75} className={isActive ? "text-amber-300" : "text-text-muted"} />
                        <span className="truncate">{device.name}</span>
                        {device.is_default && (
                          <span className="ml-auto shrink-0 text-[10px] text-text-muted">Default</span>
                        )}
                      </button>
                    );
                  })}
                </div>
              )}
            </div>
          </Row>

          <Row
            icon={ShieldCheck}
            title="Noise reduction"
            description="Filter fan noise, keyboard clicks, and other non-speech sounds with RNNoise before transcription."
            control={<Toggle on={!!settings?.noise_reduction} onClick={handleNoiseReductionToggle} />}
          />

          <Row
            icon={VolumeX}
            title="Audio ducking"
            description="Lower system volume while dictating so other audio doesn't compete with your mic. Restored when recording stops."
            control={<Toggle on={!!settings?.audio_ducking} onClick={handleAudioDuckingToggle} />}
          >
            {settings?.audio_ducking && (
              <div>
                <div className="mb-2 flex items-center justify-between">
                  <span className="text-xs text-text-muted">Reduction amount</span>
                  <span className="text-xs font-medium tabular-nums text-text-secondary">
                    {settings.ducking_amount}%
                  </span>
                </div>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={5}
                  value={settings.ducking_amount}
                  onChange={(e) => handleDuckingAmountChange(parseInt(e.target.value, 10))}
                  className="w-full cursor-pointer"
                />
                <div className="mt-1 flex justify-between">
                  <span className="text-[10px] text-text-muted">None</span>
                  <span className="text-[10px] text-text-muted">Full mute</span>
                </div>
              </div>
            )}
          </Row>

          {/* macOS permission buttons */}
          {platformInfo?.os === "macos" && (
            <Row title="System permissions" description="macOS requires explicit access for the mic and global hotkeys.">
              <div className="flex gap-2">
                <button
                  onClick={() => openMicSettings().catch(console.error)}
                  className="flex items-center gap-1.5 rounded-lg border border-border bg-surface-2 px-2.5 py-1.5 text-xs text-text-secondary transition-colors hover:border-border-hover hover:bg-surface-1 hover:text-text-primary"
                >
                  <Mic size={12} />
                  Microphone
                  <ExternalLink size={10} className="opacity-50" />
                </button>
                <button
                  onClick={() => openAccessibilitySettings().catch(console.error)}
                  className="flex items-center gap-1.5 rounded-lg border border-border bg-surface-2 px-2.5 py-1.5 text-xs text-text-secondary transition-colors hover:border-border-hover hover:bg-surface-1 hover:text-text-primary"
                >
                  <Keyboard size={12} />
                  Accessibility
                  <ExternalLink size={10} className="opacity-50" />
                </button>
              </div>
            </Row>
          )}
        </GroupCard>

        {/* ── Transcription ── */}
        <GroupCard title="Transcription" delay={0.16}>
          <Row
            icon={ScanText}
            title="Screen context"
            description="Read visible text in the focused app to transcribe file paths, identifiers, and commands verbatim. Local only — never leaves your device."
            control={<Toggle on={!!settings?.use_screen_context} onClick={handleScreenContextToggle} />}
          >
            {settings?.use_screen_context && settings?.structured_mode && (
              <div className="rounded-lg border border-border/60 bg-surface-2/40 p-3">
                <p className="mb-2.5 text-xs leading-relaxed text-text-muted">
                  Also pass screen-context tokens into Structured Mode so the LLM substitutes
                  phonetic guesses with verbatim screen text.
                </p>
                <div className="flex items-center gap-3">
                  <Toggle
                    on={!!settings?.structured_use_screen_context}
                    onClick={handleStructuredScreenContextToggle}
                  />
                  <span className="text-xs text-text-secondary">
                    {settings?.structured_use_screen_context ? "Used in Structured Mode" : "Whisper only"}
                  </span>
                </div>
              </div>
            )}
          </Row>

          <Row
            icon={Eye}
            title="Live preview"
            description={
              <>
                Show live transcription words in the floating pill while recording.
                <span className="text-amber-300/85"> Adds latency</span> — runs inference during
                recording.
              </>
            }
            control={<Toggle on={!!settings?.live_preview} onClick={handleLivePreviewToggle} />}
          />

          <Row
            icon={Mic}
            title="Voice commands"
            description={'Say "new line", "new paragraph", or "delete last word" while dictating.'}
            control={<Toggle on={!!settings?.voice_commands} onClick={handleVoiceCommandsToggle} />}
          >
            {settings?.voice_commands && (
              <div className="flex flex-col gap-3">
                <button
                  onClick={() => setShowVoiceCommands(true)}
                  className="flex items-center gap-1.5 self-start rounded-md px-2 py-1 text-xs text-text-muted transition-colors hover:bg-surface-2/70 hover:text-text-secondary"
                >
                  <Info size={12} />
                  View all commands
                </button>
                <div className="rounded-lg border border-border/60 bg-surface-2/40 p-3">
                  <div className="mb-2 flex items-center gap-1.5">
                    <Send size={12} strokeWidth={2} className="text-text-muted" />
                    <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                      Command Send
                    </span>
                  </div>
                  <p className="mb-3 text-xs leading-relaxed text-text-muted">
                    Say "send" at the end of your dictation to press Enter and send the message.
                  </p>
                  <div className="flex items-center gap-3">
                    <Toggle on={!!settings?.command_send} onClick={handleCommandSendToggle} />
                    <span className="text-xs text-text-secondary">
                      {settings?.command_send ? "Enabled" : "Disabled"}
                    </span>
                  </div>
                </div>
              </div>
            )}
          </Row>

          <Row
            icon={Terminal}
            title={
              <span className="inline-flex items-center gap-2">
                Command intent
                <span className="rounded-md border border-amber-400/30 bg-amber-500/[0.12] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-amber-200">
                  Experimental
                </span>
              </span>
            }
            description={'Start a command with "computer" to have the local LLM turn it into actions and run them.'}
            control={<Toggle on={!!settings?.command_intent} onClick={handleCommandIntentToggle} />}
          >
            {settings?.command_intent && (
              <div className="rounded-lg border border-border/60 bg-surface-2/40 p-3">
                <div className="mb-2 flex items-center gap-1.5">
                  <ShieldCheck size={12} strokeWidth={2} className="text-text-muted" />
                  <span className="text-[11px] font-semibold uppercase tracking-wider text-text-muted">
                    Confirm before destructive actions
                  </span>
                </div>
                <p className="mb-3 text-xs leading-relaxed text-text-muted">
                  Ask before running a plan that could close a window, cut text, or launch an app.
                </p>
                <div className="flex items-center gap-3">
                  <Toggle
                    on={!!settings?.intent_confirm_destructive}
                    onClick={handleIntentConfirmToggle}
                  />
                  <span className="text-xs text-text-secondary">
                    {settings?.intent_confirm_destructive ? "Enabled" : "Disabled"}
                  </span>
                </div>
              </div>
            )}
          </Row>
        </GroupCard>

        {/* ── Behavior ── */}
        <GroupCard title="Behavior" delay={0.19}>
          <Row
            icon={Layers}
            title="Auto context switching"
            description="Switch context mode based on the focused app when recording starts. Bind apps to modes in the Context Modes editor."
            control={<Toggle on={!!settings?.auto_switch_modes} onClick={handleAutoSwitchToggle} />}
          />
          <Row
            icon={Rocket}
            title={
              <span className="inline-flex items-center gap-2">
                Ship Mode
                <span className="rounded-md border border-amber-400/30 bg-amber-500/[0.12] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] text-amber-200">
                  Beta
                </span>
              </span>
            }
            description={
              <>
                Press Enter automatically after output to send the message — built for agentic
                workflows. Requires Type or Both output mode.
                <span className="block text-amber-300/80"> Sends immediately, with no chance to edit.</span>
              </>
            }
            control={<Toggle on={!!settings?.ship_mode} onClick={handleShipModeToggle} />}
          />
        </GroupCard>

        {/* ── Appearance ── */}
        <GroupCard title="Appearance" delay={0.22}>
          <Row title="Theme">
            <Segmented
              options={[
                { id: "dark", label: "Dark", icon: Moon },
                { id: "light", label: "Light", icon: Sun },
              ] as const}
              value={currentTheme === "light" ? "light" : "dark"}
              onChange={handleThemeChange}
            />
          </Row>
        </GroupCard>

        {/* ── About ── */}
        <GroupCard title="About" delay={0.25}>
          <Row
            title={`OmniVox${appVersion ? ` v${appVersion}` : ""}`}
            description={
              <span className="flex flex-wrap items-center gap-1.5">
                <span>Local-first AI dictation</span>
                <span className="mx-0.5 text-text-muted/40">·</span>
                <span>Developed by</span>
                <svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 200 200" fill="none" className="inline-block shrink-0">
                  <path d="M 196.52,99.98 C 196.52,46.71 152.83,1.63 99.48,1.63 H 98.56 C 63.42,1.63 37.98,18.76 20.77,43.31 C 9.12,60.21 3.48,77.89 3.48,100.51 C 3.48,151.93 47.02,198.23 97.61,198.23 H 99.16 C 151.91,198.23 196.52,154.55 196.52,99.98 Z M 98.36,147.41 C 71.71,147.41 52.39,125.18 52.39,100.26 C 52.39,73.43 74.39,48.29 101.04,51.81 C 126.03,52.96 147.31,73.92 147.31,100.08 C 147.31,125.28 127.07,147.41 98.36,147.41 Z" fill="url(#oi-grad-1)" />
                  <path d="M 101.61,1.66 C 66.27,0.61 42.09,15.71 23.04,39.79 C 11.81,54.74 6.31,70.73 6.31,91.81 C 6.31,132.79 41.91,166.39 80.12,166.39 C 114.61,166.39 147.41,141.01 147.41,103.03 L 147.16,103.16 C 145.97,126.15 126.06,146.93 98.36,147.34 C 71.71,147.74 52.39,125.51 52.39,100.59 C 52.39,70.05 76.18,33.75 119.02,33.75 C 157.19,33.75 193.37,65.79 193.37,110.08 C 193.37,126.01 187.32,142.79 178.19,157.01 C 190.72,140.14 196.52,123.08 196.52,100.01 C 196.52,47.58 155.51,3.16 101.61,1.66 Z" fill="url(#oi-grad-2)" />
                  <defs>
                    <linearGradient id="oi-grad-1" x1="10.0251" y1="18.7862" x2="183.632" y2="181.489" gradientUnits="userSpaceOnUse">
                      <stop stopColor="#3269C7" />
                      <stop offset="0.49" stopColor="#244BC6" />
                      <stop offset="1" stopColor="#56B6E7" />
                    </linearGradient>
                    <linearGradient id="oi-grad-2" x1="10.0251" y1="18.7862" x2="183.632" y2="181.489" gradientUnits="userSpaceOnUse">
                      <stop stopColor="#3269C7" />
                      <stop offset="0.49" stopColor="#4493D5" />
                      <stop offset="1" stopColor="#56B6E7" />
                    </linearGradient>
                  </defs>
                </svg>
                <span>Omni Impact</span>
              </span>
            }
          />
        </GroupCard>
      </div>

      {/* ── Voice Commands Reference Popup ── */}
      {showVoiceCommands && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-md animate-fade-in"
          onClick={() => setShowVoiceCommands(false)}
        >
          <div
            className="w-full max-w-sm rounded-2xl border border-border bg-surface-1 p-6 shadow-lg animate-scale-in"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="mb-5 flex items-center justify-between">
              <div className="flex items-center gap-2">
                <Mic size={16} strokeWidth={2} className="text-amber-300" />
                <h3 className="text-[14px] font-semibold text-text-primary">Voice Commands</h3>
              </div>
              <button
                onClick={() => setShowVoiceCommands(false)}
                className="rounded-md p-1.5 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-secondary"
              >
                <X size={14} />
              </button>
            </div>
            <div className="space-y-2">
              {[
                { phrase: "new line", desc: "Insert a line break" },
                { phrase: "new paragraph", desc: "Insert a paragraph break" },
                { phrase: "delete last word", desc: "Remove the previous word" },
                { phrase: "send", desc: "Press Enter to send (must be last word)" },
              ].map((cmd) => (
                <div
                  key={cmd.phrase}
                  className="flex items-center justify-between gap-3 rounded-lg border border-border/60 bg-surface-2/55 px-3 py-2.5"
                >
                  <div>
                    <span className="font-mono text-xs font-medium text-amber-300">"{cmd.phrase}"</span>
                    <p className="mt-0.5 text-xs text-text-muted">{cmd.desc}</p>
                  </div>
                </div>
              ))}
            </div>
            <p className="mt-5 text-center text-[10.5px] text-text-muted">
              Speak these phrases naturally during dictation
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
