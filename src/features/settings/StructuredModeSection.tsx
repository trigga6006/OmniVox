import { useCallback, useEffect, useRef, useState } from "react";
import { Sparkles, Download, Loader2, AlertCircle, Check } from "lucide-react";
import {
  type AppSettings,
  type LlmModelInfo,
  type LlmDownloadProgress,
  listLlmModels,
  downloadLlmModel,
  setActiveLlmModel,
  llmTestExtract,
  onLlmDownloadProgress,
  onLlmModelLoaded,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";

interface Props {
  settings: AppSettings | null;
  onPatch: (patch: Partial<AppSettings>) => Promise<void>;
  onOpenLlmModels: () => void;
}

/**
 * "Structured Mode" Settings section.
 *
 * Wires the toggle, model picker, timeout/min-chars sliders, and a Test
 * button that runs a canned extraction against the loaded LLM.  When the
 * toggle is flipped on with no model present, the section prompts for an
 * inline download of the default catalog entry.
 */
export function StructuredModeSection({ settings, onPatch, onOpenLlmModels }: Props) {
  const [models, setModels] = useState<LlmModelInfo[]>([]);
  const [downloading, setDownloading] = useState<string | null>(null);
  const [downloadPct, setDownloadPct] = useState(0);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => () => {
    mountedRef.current = false;
  }, []);

  const refreshModels = useCallback(async () => {
    try {
      const m = await listLlmModels();
      if (mountedRef.current) setModels(m);
    } catch {
      /* ignore — empty list renders the "no models" state */
    }
  }, []);

  useEffect(() => {
    refreshModels();

    const unlistenProgress = onLlmDownloadProgress((p: LlmDownloadProgress) => {
      if (!mountedRef.current) return;
      if (p.status === "downloading") {
        setDownloading(p.model_id);
        setDownloadPct(p.progress_percent);
      } else if (p.status === "completed") {
        setDownloading(null);
        setDownloadPct(100);
        refreshModels();
      } else {
        setDownloading(null);
        setDownloadPct(0);
      }
    });

    const unlistenLoaded = onLlmModelLoaded(() => refreshModels());

    return () => {
      unlistenProgress.then((fn) => fn());
      unlistenLoaded.then((fn) => fn());
    };
  }, [refreshModels]);

  const structuredMode = settings?.structured_mode ?? false;
  const activeLlmId = settings?.active_llm_model_id ?? null;
  const downloadedModels = models.filter((m) => m.is_downloaded);
  const defaultModel = models.find((m) => m.is_default);
  const hasAny = downloadedModels.length > 0;
  const fallbackDownloadedModel =
    downloadedModels.find((m) => m.is_default) ?? downloadedModels[0] ?? null;

  const handleToggle = useCallback(async () => {
    const next = !structuredMode;
    // When enabling with nothing downloaded, silently trigger the default download.
    if (next && !hasAny && defaultModel && !downloading) {
      setDownloading(defaultModel.id);
      try {
        await downloadLlmModel(defaultModel.id);
        await setActiveLlmModel(defaultModel.id);
        await onPatch({
          structured_mode: true,
          active_llm_model_id: defaultModel.id,
        });
      } catch (e) {
        setDownloading(null);
        console.error("Inline LLM download failed:", e);
      }
      return;
    }
    if (next && !activeLlmId && fallbackDownloadedModel) {
      try {
        await setActiveLlmModel(fallbackDownloadedModel.id);
        await onPatch({
          structured_mode: true,
          active_llm_model_id: fallbackDownloadedModel.id,
        });
      } catch (e) {
        console.error("Auto-select LLM failed:", e);
      }
      return;
    }
    await onPatch({ structured_mode: next });
  }, [
    structuredMode,
    hasAny,
    defaultModel,
    downloading,
    activeLlmId,
    fallbackDownloadedModel,
    onPatch,
  ]);

  const handlePickModel = useCallback(
    async (modelId: string) => {
      try {
        await setActiveLlmModel(modelId);
        await onPatch({ active_llm_model_id: modelId });
      } catch (e) {
        console.error("Activate LLM failed:", e);
      }
    },
    [onPatch]
  );

  const handleTest = useCallback(async () => {
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
  }, []);

  return (
    <section
      className="bg-surface-1 rounded-xl border border-border p-5 hover:border-border-hover transition-colors animate-slide-up"
      style={{ opacity: 0, animationDelay: "0.33s", animationFillMode: "forwards" }}
    >
      <div className="flex items-center gap-2 mb-3">
        <Sparkles size={14} strokeWidth={2} className="text-violet-400" />
        <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
          Structured Mode
        </span>
      </div>

      <p className="text-xs text-text-muted mb-4 leading-relaxed">
        Turn dictation into polished agent prompts using a local LLM. Voice
        commands and bullet formatting are disabled while Structured Mode is on
        — the LLM is the sole formatter.
      </p>

      {/* Main toggle */}
      <div className="flex items-center justify-between mb-4">
        <div>
          <p className="text-sm text-text-primary">Enable Structured Mode</p>
          <p className="text-[11px] text-text-muted mt-0.5">
            {hasAny
              ? "Dictations produce a Markdown prompt in a preview panel."
              : "First turn-on downloads the default model (~1.0 GB)."}
          </p>
        </div>
        <button
          onClick={handleToggle}
          disabled={!!downloading}
          className={cn(
            "relative inline-flex h-5 w-9 items-center rounded-full transition-colors",
            structuredMode ? "bg-violet-500" : "bg-white/15",
            downloading && "opacity-60"
          )}
        >
          <span
            className={cn(
              "inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform",
              structuredMode ? "translate-x-5" : "translate-x-0.5"
            )}
          />
        </button>
      </div>

      {/* Download progress */}
      {downloading && (
        <div className="mb-4 p-3 rounded-lg bg-violet-500/10 border border-violet-400/25">
          <div className="flex items-center gap-2 mb-1.5">
            <Download size={12} className="text-violet-300" />
            <span className="text-[11px] text-violet-200">
              Downloading {models.find((m) => m.id === downloading)?.name ?? downloading}…
            </span>
          </div>
          <div className="h-1 bg-white/10 rounded-full overflow-hidden">
            <div
              className="h-full bg-violet-400 transition-[width] duration-150"
              style={{ width: `${downloadPct}%` }}
            />
          </div>
        </div>
      )}

      {/* Model picker */}
      {hasAny && (
        <div className="mb-4">
          <label className="text-[11px] text-text-muted block mb-1.5">Active model</label>
          <div className="flex items-center gap-2">
            <select
              value={activeLlmId ?? ""}
              onChange={(e) => handlePickModel(e.target.value)}
              className="flex-1 bg-surface-2 border border-border rounded-md px-2.5 py-1.5 text-sm text-text-primary focus:outline-none focus:border-violet-400/50"
            >
              {!activeLlmId && <option value="">Select a model</option>}
              {downloadedModels.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>
            <button
              onClick={onOpenLlmModels}
              className="px-2.5 py-1.5 text-[11px] text-text-muted hover:text-text-primary border border-border hover:border-border-hover rounded-md transition-colors"
            >
              Manage…
            </button>
          </div>
        </div>
      )}

      {/* Sliders */}
      {structuredMode && (
        <div className="space-y-3 mb-4">
          <NumericSlider
            label="Minimum characters"
            hint={`Short utterances (< ${settings?.structured_min_chars ?? 40} chars) fall through to plain dictation.`}
            min={20}
            max={200}
            step={5}
            value={settings?.structured_min_chars ?? 40}
            onChange={(v) => onPatch({ structured_min_chars: v })}
          />
          <NumericSlider
            label="LLM timeout (seconds)"
            hint="After this, the pipeline falls back to plain text."
            min={3}
            max={15}
            step={1}
            value={settings?.llm_timeout_secs ?? 8}
            onChange={(v) => onPatch({ llm_timeout_secs: v })}
          />
        </div>
      )}

      {/* Test button */}
      {hasAny && structuredMode && (
        <div>
          <button
            onClick={handleTest}
            disabled={testing}
            className={cn(
              "flex items-center gap-1.5 px-3 py-1.5 text-[11px] rounded-md transition-colors",
              "bg-violet-500/15 hover:bg-violet-500/25 border border-violet-400/30 text-violet-200",
              testing && "opacity-60 cursor-wait"
            )}
          >
            {testing ? (
              <Loader2 size={11} className="animate-spin" />
            ) : (
              <Check size={11} />
            )}
            Test with a canned prompt
          </button>
          {testResult && (
            <pre className="mt-2 p-2.5 bg-surface-2 rounded-md text-[10px] font-mono text-text-primary/85 whitespace-pre-wrap max-h-[200px] overflow-y-auto">
              {testResult}
            </pre>
          )}
          {testError && (
            <div className="mt-2 p-2 bg-red-500/10 border border-red-400/25 rounded-md text-[10px] text-red-300 flex items-start gap-1.5">
              <AlertCircle size={11} className="shrink-0 mt-0.5" />
              <span>{testError}</span>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

function NumericSlider({
  label,
  hint,
  min,
  max,
  step,
  value,
  onChange,
}: {
  label: string;
  hint?: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (v: number) => void;
}) {
  return (
    <div>
      <div className="flex items-center justify-between mb-1">
        <span className="text-[11px] text-text-primary">{label}</span>
        <span className="text-[11px] text-text-muted tabular-nums">{value}</span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseInt(e.target.value, 10))}
        className="w-full accent-violet-500"
      />
      {hint && <p className="text-[10px] text-text-muted mt-0.5">{hint}</p>}
    </div>
  );
}
