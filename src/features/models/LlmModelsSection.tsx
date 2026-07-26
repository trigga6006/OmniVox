import { useCallback, useEffect, useRef, useState } from "react";
import {
  Download,
  Check,
  Loader2,
  AlertCircle,
  FlaskConical,
  Activity,
  ChevronDown,
} from "lucide-react";
import {
  listLlmModels,
  downloadLlmModel,
  setActiveLlmModel,
  getActiveLlmModel,
  onLlmDownloadProgress,
  onLlmModelLoaded,
  llmTestExtract,
  getLlmDiagnostics,
  getSettings,
  updateSettings,
  onSettingsChanged,
  type LlmModelInfo,
  type LlmExtractionRecord,
  type AppSettings,
} from "@/lib/tauri";
import { formatBytes, cn } from "@/lib/utils";
import { Button, Card, Toggle, Badge } from "@/components/ui";

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
  const [downloadErrors, setDownloadErrors] = useState<Record<string, string>>({});
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
      if (p.status === "downloading") {
        setDownloadErrors((prev) => {
          if (!prev[p.model_id]) return prev;
          const next = { ...prev };
          delete next[p.model_id];
          return next;
        });
      }
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
    setDownloadErrors((prev) => {
      const next = { ...prev };
      delete next[id];
      return next;
    });
    try {
      await downloadLlmModel(id);
    } catch (err) {
      console.error("LLM download failed:", err);
      const message = err instanceof Error ? err.message : String(err);
      setDownloadErrors((prev) => ({
        ...prev,
        [id]: message || "Download failed",
      }));
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

  // Diagnostics disclosure — refetched on every expand so it always shows
  // the current session's ring buffer without needing a live subscription.
  const [diagOpen, setDiagOpen] = useState(false);
  const [diagRecords, setDiagRecords] = useState<LlmExtractionRecord[]>([]);
  const handleDiagToggle = async () => {
    const next = !diagOpen;
    setDiagOpen(next);
    if (next) {
      try {
        const recs = await getLlmDiagnostics();
        if (mountedRef.current) setDiagRecords(recs);
      } catch {
        /* panel just shows the empty state */
      }
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
      <Card
        className="px-4 py-3.5 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        {/* Row 1: Enable toggle */}
        <div className="flex items-center justify-between">
          <div className="min-w-0 flex-1 pr-3">
            <p className="text-[14px] text-text-primary">Enable Structured Mode</p>
            <p className="mt-0.5 text-[11.5px] leading-snug text-text-muted">
              {hasDownloaded
                ? "Dictations become Markdown prompts in a preview panel."
                : "First turn-on downloads the default model (~1.0 GB)."}
            </p>
          </div>
          <Toggle
            accent="violet"
            checked={structuredMode}
            onChange={handleToggle}
            disabled={Object.keys(downloading).length > 0}
            aria-label="Toggle Structured Mode"
          />
        </div>

        {/* Row 2: sliders + test, only while enabled */}
        {structuredMode && (
          <div className="mt-3.5 grid grid-cols-1 items-center gap-x-5 gap-y-2.5 border-t border-border/60 pt-3.5 sm:grid-cols-[1fr_1fr_auto]">
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
            <Button
              size="sm"
              variant="secondary"
              onClick={handleTest}
              disabled={!testAvailable || testing}
              loading={testing}
              icon={<FlaskConical strokeWidth={2} />}
              className="self-center justify-self-end"
              title={
                testAvailable
                  ? "Run a canned dictation through the active model"
                  : "Download + activate a model to test"
              }
            >
              Test prompt
            </Button>
          </div>
        )}

        {/* Test output / error — appears below the row only after a
            test run, so the card stays compact in the steady state. */}
        {testResult && (
          <pre className="mt-3 max-h-[180px] overflow-y-auto whitespace-pre-wrap rounded-md border border-border bg-surface-2 p-3 font-mono text-[10.5px] leading-relaxed text-text-primary/90">
            {testResult}
          </pre>
        )}
        {testError && (
          <div className="mt-3 flex items-start gap-1.5 rounded-md border border-error/25 bg-error/[0.08] p-2 text-[10.5px] text-error">
            <AlertCircle size={11} className="mt-0.5 shrink-0" />
            <span>{testError}</span>
          </div>
        )}

        {/* Diagnostics disclosure — the last 20 extraction attempts from
            the in-memory ring buffer, so "why was that dictation slow /
            plain?" is answerable without env vars or log files. */}
        {structuredMode && (
          <div className="mt-3 border-t border-border/60 pt-3">
            <button
              onClick={handleDiagToggle}
              className="flex items-center gap-1.5 text-[11px] font-medium text-text-muted transition-colors hover:text-text-secondary"
            >
              <Activity size={11} strokeWidth={2} />
              Recent extractions
              <ChevronDown
                size={11}
                strokeWidth={2}
                className={cn("transition-transform", diagOpen && "rotate-180")}
              />
            </button>
            {diagOpen &&
              (diagRecords.length === 0 ? (
                <p className="mt-2 text-[10.5px] text-text-muted">
                  No structured extractions this session yet.
                </p>
              ) : (
                <div className="mt-2 flex max-h-[180px] flex-col gap-1 overflow-y-auto">
                  {diagRecords.map((r, i) => (
                    <div
                      key={`${r.timestamp}-${i}`}
                      className="flex items-center gap-2.5 rounded-md border border-border/50 bg-surface-2/50 px-2.5 py-1.5 text-[10.5px] tabular-nums"
                    >
                      <span className="shrink-0 text-text-muted">
                        {new Date(r.timestamp).toLocaleTimeString()}
                      </span>
                      <span
                        className={cn(
                          "min-w-0 truncate",
                          r.outcome === "ok" ? "text-success" : "text-error"
                        )}
                        title={r.outcome}
                      >
                        {r.outcome === "ok" ? `${r.duration_ms} ms` : r.outcome}
                      </span>
                      <span className="ml-auto shrink-0 text-text-muted">
                        {r.input_chars} in → {r.output_chars} out
                        {r.truncated_chars > 0 && ` · ${r.truncated_chars} cut`}
                      </span>
                    </div>
                  ))}
                </div>
              ))}
          </div>
        )}
      </Card>

      {/* Model list — row chrome deliberately mirrors the Whisper rows
          above so the eye reads both catalogs as the same shape of
          thing.  Violet accent stripe on the active row keeps it
          distinct from the Whisper "success-green" active stripe. */}
      <div className="mt-3 flex flex-col gap-2">
        {models.map((m, i) => {
          const progress = downloading[m.id];
          const isDownloading = progress !== undefined;
          const isActive = activeId === m.id;
          const downloadError = downloadErrors[m.id];

          return (
            <Card
              key={m.id}
              className={cn(
                "flex items-center gap-4 px-5 py-3.5 opacity-0 transition-colors duration-200 hover:border-border-hover animate-slide-up",
                isActive && "border-l-[3px] border-l-violet-400/80",
                m.is_default && !isActive && "border-l-[3px] border-l-violet-500/45"
              )}
              style={{
                animationDelay: `${0.09 + i * 0.04}s`,
                animationFillMode: "forwards",
              }}
            >
              {/* Left: name + badges + description + meta line */}
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[14px] font-medium text-text-primary">
                    {m.name}
                  </span>
                  {m.is_default && <Badge tone="violet">Default</Badge>}
                  {isActive && <Badge tone="green">Active</Badge>}
                </div>
                <p className="mt-0.5 line-clamp-1 text-xs leading-relaxed text-text-muted">
                  {m.description}
                </p>
                {isDownloading && (
                  <div className="mt-2 h-1 overflow-hidden rounded-full bg-surface-3">
                    <div
                      className="h-full bg-violet-400 transition-[width] duration-150"
                      style={{ width: `${progress ?? 0}%` }}
                    />
                  </div>
                )}
                {downloadError && (
                  <p className="mt-2 line-clamp-2 text-[11px] leading-snug text-error">
                    {downloadError}
                  </p>
                )}
              </div>

              {/* Center: size + quant + context */}
              <div className="flex shrink-0 items-center gap-3">
                <span className="w-[70px] text-right font-mono text-xs tabular-nums text-text-muted">
                  {formatBytes(m.size_bytes)}
                </span>
                <span className="w-[54px] rounded-full bg-surface-2 px-1.5 py-0.5 text-center text-[10px] text-text-muted">
                  {m.quantization}
                </span>
                <span className="w-[44px] text-right font-mono text-[10px] tabular-nums text-text-muted/80">
                  {(m.context_length / 1024).toFixed(0)}k ctx
                </span>
              </div>

              {/* Right: action button(s) */}
              <div className="flex w-[110px] shrink-0 items-center justify-end gap-1.5">
                {!m.is_downloaded && !isDownloading && (
                  <Button
                    size="sm"
                    variant="primary"
                    icon={<Download strokeWidth={2} />}
                    onClick={() => handleDownload(m.id)}
                  >
                    Download
                  </Button>
                )}
                {isDownloading && (
                  <div className="inline-flex items-center gap-1.5 text-xs tabular-nums text-violet-300">
                    <Loader2 size={12} className="animate-spin" />
                    {Math.round(progress ?? 0)}%
                  </div>
                )}
                {m.is_downloaded && !isActive && (
                  <Button
                    size="sm"
                    variant="secondary"
                    onClick={() => handleActivate(m.id)}
                  >
                    Activate
                  </Button>
                )}
                {m.is_downloaded && isActive && (
                  <span className="inline-flex items-center gap-1.5 text-xs font-medium text-success">
                    <Check size={13} strokeWidth={2} />
                    In use
                  </span>
                )}
              </div>
            </Card>
          );
        })}

        {models.length === 0 && (
          <div className="rounded-2xl border border-border bg-surface-1 px-5 py-6 text-center text-xs text-text-muted">
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
