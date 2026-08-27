import { useCallback, useEffect, useRef, useState } from "react";
import {
  Download,
  Trash2,
  Check,
  Loader2,
  AlertCircle,
  FlaskConical,
} from "lucide-react";
import {
  listLlmModels,
  downloadLlmModel,
  deleteLlmModel,
  onLlmDownloadProgress,
  llmTestCleanup,
  getSettingsSnapshot,
  patchSettings,
  onSettingsChanged,
  type LlmModelInfo,
  type LlmCleanupResult,
  type AppSettings,
} from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import {
  Badge,
  Button,
  Card,
  Input,
  ModelRow,
  SkeletonRows,
  Toggle,
} from "@/components/ui";

/** Runtime memory reads as GB once it clears 1 GB — same rule in every tab. */
function memoryLabel(mb: number) {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

/** Parameter counts read as billions past 1 000 M. */
function paramLabel(millions: number) {
  return millions >= 1000
    ? `${(millions / 1000).toFixed(1)}B params`
    : `${millions}M params`;
}

const SAMPLE_TEXT =
  "um so basically send the uh the report by five pm on friday i mean saturday";

/**
 * AI Transcript Cleanup manager — the fourth Models tab. Runs dictations
 * through a small local model (S1-mini by Superwhisper) that strips fillers,
 * resolves spoken self-corrections, and tidies punctuation/casing/formatting
 * — English only, entirely on-device.
 *
 * Shares the LlmModelInfo catalog and download/progress plumbing with
 * Structured Mode, but is activated through its own settings slot
 * (`cleanup_mode` / `active_cleanup_model_id`) rather than
 * `setActiveLlmModel`, since the backend only allows purpose-"structured"
 * models on that path. There is exactly one purpose-"cleanup" catalog
 * entry today, so the section shows a single model row rather than a
 * pick-one list.
 */
export function CleanupModelSection() {
  const [models, setModels] = useState<LlmModelInfo[]>([]);
  const [downloading, setDownloading] = useState<Record<string, number>>({});
  const [downloadErrors, setDownloadErrors] = useState<Record<string, string>>({});
  const [deletingId, setDeletingId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [testText, setTestText] = useState(SAMPLE_TEXT);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<LlmCleanupResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const mountedRef = useRef(true);
  const settingsRevision = useRef<number | undefined>(undefined);

  // StrictMode fires this effect's cleanup between mount and remount — see
  // LlmModelsSection for why the ref needs the explicit re-arm on mount.
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refresh = useCallback(async () => {
    try {
      const m = await listLlmModels();
      if (!mountedRef.current) return;
      setModels(m.filter((model) => model.purpose === "cleanup"));
    } catch (err) {
      console.error("Failed to load cleanup models:", err);
    } finally {
      if (mountedRef.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    getSettingsSnapshot()
      .then((snapshot) => {
        if (mountedRef.current) {
          settingsRevision.current = snapshot.revision;
          setSettings(snapshot.settings);
        }
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
    return () => {
      unlistenSettings.then((fn) => fn());
      unlistenProgress.then((fn) => fn());
    };
  }, [refresh]);

  // Patch helper mirroring LlmModelsSection's pattern: preserve everything,
  // persist the change, let the broadcast event reconcile the local copy.
  const applyPatch = useCallback(
    async (patch: Partial<AppSettings>) => {
      if (!settings) return;
      const updated: AppSettings = { ...settings, ...patch };
      setSettings(updated); // optimistic so the toggle feels instant
      try {
        const snapshot = await patchSettings(patch, settingsRevision.current);
        settingsRevision.current = snapshot.revision;
        if (mountedRef.current) setSettings(snapshot.settings);
      } catch (e) {
        console.error("Update settings failed:", e);
        try {
          const snapshot = await getSettingsSnapshot();
          settingsRevision.current = snapshot.revision;
          if (mountedRef.current) setSettings(snapshot.settings);
        } catch {
          /* ignore */
        }
      }
    },
    [settings]
  );

  const cleanupModel = models[0] ?? null;
  const cleanupMode = settings?.cleanup_mode ?? false;
  // `onLlmDownloadProgress` is a process-wide event, so the map also collects
  // Structured-Mode downloads. Mirror the purpose filter applied to `models`
  // in `refresh` before consulting it, or a structured download dead-disables
  // this toggle.
  const downloadingHere = models.some((m) => downloading[m.id] !== undefined);

  // Toggle flow mirrors LlmModelsSection: flipping it on with the model not
  // yet downloaded auto-downloads it and activates it in the same turn.
  const handleToggle = useCallback(async () => {
    if (!settings || !cleanupModel) return;
    const next = !cleanupMode;

    if (next && !cleanupModel.is_downloaded && !downloading[cleanupModel.id]) {
      setDownloading((prev) => ({ ...prev, [cleanupModel.id]: 0 }));
      try {
        await downloadLlmModel(cleanupModel.id);
        await applyPatch({
          cleanup_mode: true,
          active_cleanup_model_id: cleanupModel.id,
        });
      } catch (e) {
        console.error("Inline cleanup model download failed:", e);
        const message = e instanceof Error ? e.message : String(e);
        setDownloadErrors((prev) => ({
          ...prev,
          [cleanupModel.id]: message || "Download failed",
        }));
        setDownloading((prev) => {
          const next = { ...prev };
          delete next[cleanupModel.id];
          return next;
        });
      }
      return;
    }
    if (next && !settings.active_cleanup_model_id) {
      await applyPatch({
        cleanup_mode: true,
        active_cleanup_model_id: cleanupModel.id,
      });
      return;
    }
    await applyPatch({ cleanup_mode: next });
  }, [settings, cleanupMode, cleanupModel, downloading, applyPatch]);

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
      console.error("Cleanup model download failed:", err);
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

  const handleDelete = async (id: string) => {
    setDeletingId(id);
    try {
      await deleteLlmModel(id);
      await refresh();
      // The backend clears active_cleanup_model_id when its model is
      // deleted — re-fetch so the toggle/Active badge reflect that instead
      // of going stale.
      const snapshot = await getSettingsSnapshot().catch(() => null);
      if (snapshot && mountedRef.current) {
        settingsRevision.current = snapshot.revision;
        setSettings(snapshot.settings);
      }
    } catch (err) {
      console.error("Cleanup model delete failed:", err);
    } finally {
      if (mountedRef.current) setDeletingId(null);
    }
  };

  const handleTest = async () => {
    const text = testText.trim();
    if (!text || testing) return;
    setTesting(true);
    setTestError(null);
    setTestResult(null);
    try {
      const result = await llmTestCleanup(text);
      if (mountedRef.current) setTestResult(result);
    } catch (e) {
      if (mountedRef.current) setTestError(String(e));
    } finally {
      if (mountedRef.current) setTesting(false);
    }
  };

  const testAvailable = !!cleanupModel?.is_downloaded && cleanupMode;

  return (
    <section className="flex flex-col gap-3">
      {/* Enable + explainer */}
      <Card
        className="px-4 py-3.5 opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <div className="flex items-center justify-between">
          <div className="min-w-0 flex-1 pr-3">
            <p className="text-sm text-text-primary">Enable AI Transcript Cleanup</p>
            <p className="mt-0.5 text-xs leading-snug text-text-muted">
              Removes filler words ("um", "uh"), resolves spoken self-corrections ("send it
              Friday — I mean Saturday"), and fixes punctuation, casing, and the formatting of
              numbers, dates, and emails. English only — runs entirely on your device.
            </p>
          </div>
          <Toggle
            accent="teal"
            checked={cleanupMode}
            onChange={handleToggle}
            disabled={downloadingHere}
            aria-label="Toggle AI Transcript Cleanup"
          />
        </div>
        <p className="mt-3 border-t border-border/60 pt-3 text-xs text-text-muted/75">
          Powered by <span className="text-text-secondary">S1-mini by Superwhisper</span>.
          {!cleanupModel?.is_downloaded && " First turn-on downloads the model (~462 MB)."}
        </p>
      </Card>

      {/* Model row — same row chrome as the Structured Mode / Whisper
          catalogs. Exactly one purpose-"cleanup" entry today, so there's no
          Activate button: the toggle above owns activation via
          active_cleanup_model_id. */}
      <div className="flex flex-col gap-2">
        {loading && models.length === 0 && <SkeletonRows count={1} />}

        {models.map((m, i) => {
          const progress = downloading[m.id];
          const isDownloading = progress !== undefined;
          const isActive = settings?.active_cleanup_model_id === m.id;
          const isDeleting = deletingId === m.id;
          const downloadError = downloadErrors[m.id];

          return (
            <ModelRow
              key={m.id}
              name={m.name}
              accent="teal"
              rail={isActive ? "strong" : undefined}
              className="opacity-0 animate-slide-up"
              style={{
                animationDelay: `${0.09 + i * 0.04}s`,
                animationFillMode: "forwards",
              }}
              badges={isActive ? <Badge tone="green">Active</Badge> : undefined}
              description={m.description}
              meta={[
                formatBytes(m.size_bytes),
                m.quantization,
                `${(m.context_length / 1024).toFixed(0)}k ctx`,
                paramLabel(m.parameter_count_millions),
                `${m.capability_tier} tier`,
                `~${memoryLabel(m.estimated_memory_mb)} RAM`,
              ]}
              progress={isDownloading ? (progress ?? 0) : undefined}
              error={downloadError}
              action={
                <>
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
                    /* `text-teal` is the role token (`--color-teal`), which the
                       light theme remaps; the stock `teal-300` scale is not,
                       and read at 1.4:1 on paper. */
                    <div className="inline-flex items-center gap-1.5 font-mono text-xs tabular-nums text-teal">
                      <Loader2 size={12} className="animate-spin" />
                      {Math.round(progress ?? 0)}%
                    </div>
                  )}
                  {m.is_downloaded && !isDownloading && (
                    <>
                      <span className="inline-flex items-center gap-1.5 text-xs font-medium text-success">
                        <Check size={13} strokeWidth={2} />
                        Downloaded
                      </span>
                      <Button
                        size="sm"
                        variant="ghost"
                        icon={<Trash2 strokeWidth={2} />}
                        loading={isDeleting}
                        onClick={() => handleDelete(m.id)}
                        aria-label="Delete cleanup model"
                        className="px-2 text-text-muted hover:text-error"
                      />
                    </>
                  )}
                </>
              }
              /* Test box lives in the row's footer slot — it tests THIS model,
                 so it belongs to the row rather than to a card floating below
                 it. */
              footer={
                <>
                  <div className="mb-2 flex items-center gap-1.5">
                    <FlaskConical size={12} strokeWidth={2} className="text-text-muted" />
                    <span className="eyebrow">Test cleanup</span>
                  </div>
                  <p className="mb-3 text-xs leading-snug text-text-muted">
                    Run a raw dictation through the model to see the cleaned-up result.
                  </p>

                  <div className="flex items-center gap-2">
                    <Input
                      type="text"
                      value={testText}
                      onChange={(e) => setTestText(e.target.value)}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") handleTest();
                      }}
                      placeholder="Paste a raw dictation…"
                      aria-label="Raw dictation to clean up"
                      className="min-w-0 flex-1"
                    />
                    <Button
                      size="sm"
                      variant="primary"
                      onClick={handleTest}
                      disabled={!testAvailable || !testText.trim()}
                      loading={testing}
                      icon={<FlaskConical strokeWidth={2} />}
                      title={
                        testAvailable
                          ? "Run this text through the active cleanup model"
                          : "Download + enable AI Transcript Cleanup to test"
                      }
                    >
                      Test
                    </Button>
                  </div>

                  {testResult && (
                    <div className="mt-3 rounded-[var(--radius-m)] border border-border bg-surface-2/40 px-3 py-2.5">
                      <p className="text-sm leading-relaxed text-text-primary">
                        {testResult.output}
                      </p>
                      <p className="mt-2 font-mono text-2xs tabular-nums text-text-muted">
                        {testResult.duration_ms}ms
                      </p>
                    </div>
                  )}
                  {testError && (
                    <div className="mt-3 flex items-start gap-1.5 rounded-[var(--radius-s)] border border-error/25 bg-error/[0.08] p-2 text-xs text-error">
                      <AlertCircle size={12} className="mt-0.5 shrink-0" />
                      <span>{testError}</span>
                    </div>
                  )}
                </>
              }
            />
          );
        })}

        {!loading && models.length === 0 && (
          <div className="rounded-[var(--radius-l)] border border-border bg-surface-1 px-5 py-6 text-center text-xs text-text-muted">
            No cleanup model in the catalog yet.
          </div>
        )}
      </div>
    </section>
  );
}
