import { useCallback, useEffect, useRef, useState } from "react";
import {
  Download,
  Check,
  Loader2,
  AlertCircle,
  FlaskConical,
} from "lucide-react";
import {
  listLlmModels,
  downloadLlmModel,
  setActiveLlmModel,
  getActiveLlmModel,
  onLlmDownloadProgress,
  onLlmModelLoaded,
  llmTestExtract,
  getSettings,
  updateSettings,
  onSettingsChanged,
  type LlmModelInfo,
  type AppSettings,
} from "@/lib/tauri";
import { formatBytes, cn } from "@/lib/utils";

/**
 * Structured-Mode LLM manager — lives on the Models page alongside the
 * Whisper catalog so there's one central "pick and tune your models"
 * surface.  The companion `StructuredModeSection` in Settings was
 * folded into this component; routing the user through Settings just to
 * download a second language model was a dead-end interaction.
 *
 * Shape:
 *   • Section header (matches the Whisper one above it)
 *   • Compact config sub-card — Enable toggle + min-chars + LLM timeout
 *     + Test button.  Deliberately smaller than the settings version
 *     so the model list stays the visual anchor of the page.
 *   • Model rows using the same row chrome as the Whisper list so users
 *     read "models below, knobs above" at a glance.
 */
export function LlmModelsSection() {
  const [models, setModels] = useState<LlmModelInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [downloading, setDownloading] = useState<Record<string, number>>({});
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  // StrictMode fires this effect's cleanup between mount and remount,
  // which flips the ref to false.  Without the explicit reset on
  // mount, the second run of the effect never re-arms it, and every
  // subsequent `setModels` / `setSettings` is dropped — producing the
  // "no Structured Mode models in the catalog" empty state even when
  // the backend returned a full list.
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const [m, active] = await Promise.all([
        listLlmModels(),
        getActiveLlmModel(),
      ]);
      if (!mountedRef.current) return;
      setModels(m);
      setActiveId(active?.id ?? null);
    } catch (err) {
      console.error("Failed to load LLM models:", err);
    }
  }, []);

  // Initial load + subscribe to settings / download progress / model-loaded.
  useEffect(() => {
    refresh();
    getSettings()
      .then((s) => {
        if (mountedRef.current) setSettings(s);
      })
      .catch(() => {});

    const unlistenSettings = onSettingsChanged((s) => {
      if (mountedRef.current) setSettings(s);
    });
    const unlistenProgress = onLlmDownloadProgress((p) => {
      if (!mountedRef.current) return;
      setDownloading((prev) => {
        const next = { ...prev };
        if (p.status === "downloading") {
          next[p.model_id] = p.progress_percent;
        } else {
          delete next[p.model_id];
        }
        return next;
      });
      if (p.status === "completed") refresh();
    });
    const unlistenLoaded = onLlmModelLoaded(() => refresh());
    return () => {
      unlistenSettings.then((fn) => fn());
      unlistenProgress.then((fn) => fn());
      unlistenLoaded.then((fn) => fn());
    };
  }, [refresh]);

  // Patch helper mirroring SettingsPage's pattern: preserve everything,
  // persist the change, let the broadcast event reconcile the local copy.
  const applyPatch = useCallback(
    async (patch: Partial<AppSettings>) => {
      if (!settings) return;
      const updated: AppSettings = { ...settings, ...patch };
      setSettings(updated); // optimistic so sliders feel instant
      try {
        await updateSettings(updated);
      } catch (e) {
        console.error("Update settings failed:", e);
        // Revert on failure — re-fetch rather than trust our optimistic copy.
        try {
          const s = await getSettings();
          if (mountedRef.current) setSettings(s);
        } catch {
          /* ignore */
        }
      }
    },
    [settings]
  );

  const downloadedModels = models.filter((m) => m.is_downloaded);
  const hasDownloaded = downloadedModels.length > 0;
  const structuredMode = settings?.structured_mode ?? false;

  // Toggle flow mirrors the old StructuredModeSection: if the user flips
  // it on with nothing downloaded, auto-download the default and make it
  // active in the same turn so they aren't stuck on "enabled but no model."
  const handleToggle = useCallback(async () => {
    if (!settings) return;
    const next = !structuredMode;
    const defaultModel = models.find((m) => m.is_default);
    const fallbackDownloaded =
      downloadedModels.find((m) => m.is_default) ?? downloadedModels[0] ?? null;

    if (next && !hasDownloaded && defaultModel && !downloading[defaultModel.id]) {
      setDownloading((prev) => ({ ...prev, [defaultModel.id]: 0 }));
      try {
        await downloadLlmModel(defaultModel.id);
        await setActiveLlmModel(defaultModel.id);
        await applyPatch({
          structured_mode: true,
          active_llm_model_id: defaultModel.id,
        });
      } catch (e) {
        console.error("Inline LLM download failed:", e);
      }
      return;
    }
    if (next && !settings.active_llm_model_id && fallbackDownloaded) {
      try {
        await setActiveLlmModel(fallbackDownloaded.id);
        await applyPatch({
          structured_mode: true,
          active_llm_model_id: fallbackDownloaded.id,
        });
      } catch (e) {
        console.error("Auto-select LLM failed:", e);
      }
      return;
    }
    await applyPatch({ structured_mode: next });
  }, [
    settings,
    structuredMode,
    models,
    downloadedModels,
    hasDownloaded,
    downloading,
    applyPatch,
  ]);

  const handleDownload = async (id: string) => {
    setDownloading((prev) => ({ ...prev, [id]: 0 }));
    try {
      await downloadLlmModel(id);
    } catch (err) {
      console.error("LLM download failed:", err);
      setDownloading((prev) => {
        const next = { ...prev };
        delete next[id];
        return next;
      });
    }
  };

  const handleActivate = async (id: string) => {
    try {
      await setActiveLlmModel(id);
      setActiveId(id);
      await applyPatch({ active_llm_model_id: id });
    } catch (err) {
      console.error("Activate LLM failed:", err);
    }
  };

  const handleTest = async () => {
    setTesting(true);
    setTestError(null);
    setTestResult(null);
    try {
      const md = await llmTestExtract();
      if (mountedRef.current) setTestResult(md);
    } catch (e) {
      if (mountedRef.current) setTestError(String(e));
    } finally {
      if (mountedRef.current) setTesting(false);
    }
  };

  const minChars = settings?.structured_min_chars ?? 40;
  const llmTimeout = settings?.llm_timeout_secs ?? 8;
  const testAvailable = hasDownloaded && structuredMode;

  return (
    <section className="flex flex-col">
      {/* Compact config card — Enable toggle + sliders + test button.
          Same chrome as the model rows but tighter padding so it reads
          as a settings strip, not another model.  Collapses to the
          toggle-only row when Structured Mode is off to avoid noise
          before the user has opted in.  The tab bar identifies this
          section now, so no section-header is rendered here. */}
      <div
        className="bg-surface-1 rounded-xl border border-border px-4 py-3 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        {/* Row 1: Enable toggle */}
        <div className="flex items-center justify-between">
          <div className="min-w-0 flex-1 pr-3">
            <p className="text-sm text-text-primary">Enable Structured Mode</p>
            <p className="text-[11px] text-text-muted mt-0.5 leading-snug">
              {hasDownloaded
                ? "Dictations become Markdown prompts in a preview panel."
                : "First turn-on downloads the default model (~1.0 GB)."}
            </p>
          </div>
          <button
            onClick={handleToggle}
            disabled={Object.keys(downloading).length > 0}
            className={cn(
              "relative inline-flex h-5 w-9 items-center rounded-full transition-colors shrink-0",
              structuredMode ? "bg-violet-500" : "bg-white/15",
              Object.keys(downloading).length > 0 && "opacity-60"
            )}
            aria-label="Toggle Structured Mode"
          >
            <span
              className={cn(
                "inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform",
                structuredMode ? "translate-x-5" : "translate-x-0.5"
              )}
            />
          </button>
        </div>

        {/* Row 2: sliders + test, only while enabled */}
        {structuredMode && (
          <div className="mt-3 pt-3 border-t border-white/5 grid grid-cols-1 sm:grid-cols-[1fr_1fr_auto] gap-x-5 gap-y-2.5 items-center">
            <CompactSlider
              label="Min chars"
              hint="Shorter utterances skip the LLM"
              min={20}
              max={200}
              step={5}
              value={minChars}
              suffix=""
              onChange={(v) => applyPatch({ structured_min_chars: v })}
            />
            <CompactSlider
              label="LLM timeout"
              hint="Fall back to plain text after"
              min={3}
              max={15}
              step={1}
              value={llmTimeout}
              suffix="s"
              onChange={(v) => applyPatch({ llm_timeout_secs: v })}
            />
            <button
              onClick={handleTest}
              disabled={!testAvailable || testing}
              title={
                testAvailable
                  ? "Run a canned dictation through the active model"
                  : "Download + activate a model to test"
              }
              className={cn(
                "flex items-center gap-1.5 px-3 py-1.5 text-[11px] rounded-md border transition-colors self-center justify-self-end",
                testAvailable
                  ? "bg-violet-500/15 hover:bg-violet-500/25 border-violet-400/30 text-violet-200"
                  : "bg-white/[0.03] border-white/5 text-text-muted/60 cursor-not-allowed",
                testing && "opacity-60 cursor-wait"
              )}
            >
              {testing ? (
                <Loader2 size={11} className="animate-spin" />
              ) : (
                <FlaskConical size={11} strokeWidth={2} />
              )}
              Test prompt
            </button>
          </div>
        )}

        {/* Test output / error — appears below the row only after a
            test run, so the card stays compact in the steady state. */}
        {testResult && (
          <pre className="mt-3 p-2.5 bg-surface-2 rounded-md text-[10px] font-mono text-text-primary/85 whitespace-pre-wrap max-h-[180px] overflow-y-auto">
            {testResult}
          </pre>
        )}
        {testError && (
          <div className="mt-3 p-2 bg-red-500/10 border border-red-400/25 rounded-md text-[10px] text-red-300 flex items-start gap-1.5">
            <AlertCircle size={11} className="shrink-0 mt-0.5" />
            <span>{testError}</span>
          </div>
        )}
      </div>

      {/* Model list — row chrome deliberately mirrors the Whisper rows
          above so the eye reads both catalogs as the same shape of
          thing.  Violet accent stripe on the active row keeps it
          distinct from the Whisper "success-green" active stripe. */}
      <div className="mt-3 flex flex-col gap-2">
        {models.map((m, i) => {
          const progress = downloading[m.id];
          const isDownloading = progress !== undefined;
          const isActive = activeId === m.id;

          return (
            <div
              key={m.id}
              className={cn(
                "bg-surface-1 rounded-xl border border-border px-5 py-3.5 transition-colors hover:border-border-hover opacity-0 animate-slide-up flex items-center gap-4",
                isActive && "border-l-[3px] border-l-violet-400/70",
                m.is_default && !isActive && "border-l-[3px] border-l-violet-500/40"
              )}
              style={{
                animationDelay: `${0.09 + i * 0.04}s`,
                animationFillMode: "forwards",
              }}
            >
              {/* Left: name + badges + description + meta line */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-medium text-text-primary text-sm">
                    {m.name}
                  </span>
                  {m.is_default && (
                    <span className="text-[9px] font-medium uppercase tracking-wider text-violet-300 bg-violet-500/15 px-1.5 py-0.5 rounded">
                      Default
                    </span>
                  )}
                  {isActive && (
                    <span className="text-[9px] font-medium uppercase tracking-wider text-success bg-success/10 px-1.5 py-0.5 rounded">
                      Active
                    </span>
                  )}
                </div>
                <p className="text-xs text-text-muted mt-0.5 leading-relaxed line-clamp-1">
                  {m.description}
                </p>
                {isDownloading && (
                  <div className="mt-2 h-1 bg-white/10 rounded-full overflow-hidden">
                    <div
                      className="h-full bg-violet-400 transition-[width] duration-150"
                      style={{ width: `${progress ?? 0}%` }}
                    />
                  </div>
                )}
              </div>

              {/* Center: size + quant + context */}
              <div className="flex items-center gap-3 shrink-0">
                <span className="font-mono text-xs text-text-muted w-[70px] text-right">
                  {formatBytes(m.size_bytes)}
                </span>
                <span className="bg-surface-3 text-text-muted text-[10px] px-1.5 py-0.5 rounded-full w-[52px] text-center">
                  {m.quantization}
                </span>
                <span className="font-mono text-[10px] text-text-muted/75 w-[42px] text-right">
                  {(m.context_length / 1024).toFixed(0)}k ctx
                </span>
              </div>

              {/* Right: action button(s) */}
              {/* Width matches the Whisper row's action column so the two
                  catalogs line up visually when switching tabs.  Narrower
                  now that the Delete button is gone (LLM models are
                  download-only from the user's perspective — kept on disk
                  to avoid an easy "oops, that was my active model" mistake). */}
              <div className="shrink-0 w-[100px] flex justify-end items-center gap-1.5">
                {!m.is_downloaded && !isDownloading && (
                  <button
                    onClick={() => handleDownload(m.id)}
                    className="inline-flex items-center gap-1 bg-violet-500/15 hover:bg-violet-500/25 border border-violet-400/30 text-violet-200 rounded-lg px-3 py-1 text-xs font-medium transition-colors"
                  >
                    <Download size={12} strokeWidth={2} />
                    Download
                  </button>
                )}
                {isDownloading && (
                  <div className="inline-flex items-center gap-1.5 text-violet-200 text-xs">
                    <Loader2 size={12} className="animate-spin" />
                    {Math.round(progress ?? 0)}%
                  </div>
                )}
                {m.is_downloaded && !isActive && (
                  <button
                    onClick={() => handleActivate(m.id)}
                    className="inline-flex items-center gap-1 bg-violet-500/10 text-violet-300 hover:bg-violet-500/20 rounded-lg px-3 py-1 text-xs font-medium transition-colors"
                  >
                    Activate
                  </button>
                )}
                {m.is_downloaded && isActive && (
                  <span className="inline-flex items-center gap-1.5 text-success text-xs font-medium">
                    <Check size={13} strokeWidth={2} />
                    In use
                  </span>
                )}
              </div>
            </div>
          );
        })}

        {models.length === 0 && (
          <div className="bg-surface-1 rounded-xl border border-border px-5 py-6 text-center text-xs text-text-muted">
            No Structured Mode models in the catalog yet.
          </div>
        )}
      </div>
    </section>
  );
}

/**
 * Compact inline slider used by the config strip.  Label + value + hint
 * stay on a single line to keep the strip at ~two rows total.  The
 * full-size version lives in the old StructuredModeSection and was
 * meant for a detail panel — too visually heavy for a models page.
 */
function CompactSlider({
  label,
  hint,
  min,
  max,
  step,
  value,
  suffix,
  onChange,
}: {
  label: string;
  hint?: string;
  min: number;
  max: number;
  step: number;
  value: number;
  suffix: string;
  onChange: (v: number) => void;
}) {
  return (
    <div className="min-w-0">
      <div className="flex items-baseline justify-between gap-2 mb-1">
        <span className="text-[11px] text-text-primary truncate">{label}</span>
        <span className="text-[11px] text-text-muted tabular-nums shrink-0">
          {value}
          {suffix}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value, 10))}
        className="w-full accent-violet-500 h-1 cursor-pointer"
        aria-label={label}
      />
      {hint && (
        <p className="text-[10px] text-text-muted/75 mt-0.5 truncate">{hint}</p>
      )}
    </div>
  );
}
