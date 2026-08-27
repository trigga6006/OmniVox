import { useCallback, useEffect, useState } from "react";
import { AlertCircle, Download, Check, Cpu } from "lucide-react";
import {
  listModels,
  getHardwareInfo,
  downloadModel,
  setActiveModel,
  getActiveModel,
  onModelLoaded,
  type ModelInfo,
  type HardwareInfo,
} from "@/lib/tauri";
import { formatBytes } from "@/lib/utils";
import { Badge, Button, Card, ModelRow, SkeletonRows } from "@/components/ui";

/** Runtime memory reads as GB once it clears 1 GB — same rule in every tab. */
function memoryLabel(mb: number) {
  return mb >= 1024 ? `${(mb / 1024).toFixed(1)} GB` : `${mb} MB`;
}

/**
 * Whisper (speech-recognition) model catalog.  Extracted from ModelsPage
 * when the page moved to a tabbed layout so each tab's component owns
 * its own state + effects and ModelsPage itself can stay a thin
 * orchestrator.
 */
export function SpeechModelsSection() {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [hardware, setHardware] = useState<HardwareInfo | null>(null);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [activatingId, setActivatingId] = useState<string | null>(null);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [m, hw, active] = await Promise.all([
        listModels(),
        getHardwareInfo(),
        getActiveModel(),
      ]);
      setModels(m);
      setHardware(hw);
      if (active) setActiveModelId(active.id);
      setError(null);
    } catch (err) {
      // A failed catalog read used to log to the console and leave a blank
      // tab — indistinguishable from "no models exist". Say what happened.
      console.error("Failed to load models:", err);
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = onModelLoaded(() => refresh());
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [refresh]);

  const handleDownload = async (modelId: string) => {
    setDownloadingId(modelId);
    try {
      await downloadModel(modelId);
      await refresh();
    } catch (err) {
      console.error("Download failed:", err);
    } finally {
      setDownloadingId(null);
    }
  };

  // Loading a model is a multi-second backend swap; the button owns that wait
  // ("Loading…" + spinner) instead of the UI looking frozen.
  const handleActivate = async (modelId: string) => {
    setActivatingId(modelId);
    try {
      await setActiveModel(modelId);
      setActiveModelId(modelId);
    } catch (err) {
      console.error("Failed to activate model:", err);
    } finally {
      setActivatingId(null);
    }
  };

  return (
    <div className="flex flex-col">
      {/* Whisper model list. One ModelRow recipe, shared with the LLM /
          Command / Cleanup tabs — accent green for the active model, amber
          for the recommended one. */}
      {error && models.length === 0 && (
        <div
          role="alert"
          className="flex items-start gap-2 rounded-[var(--radius-m)] border border-error/25 bg-error/[0.08] px-3 py-2.5 text-xs text-error"
        >
          <AlertCircle size={13} className="mt-px shrink-0" />
          <span className="min-w-0 flex-1">
            Couldn't load the speech-model catalog. {error}
          </span>
          <Button size="sm" variant="ghost" onClick={() => void refresh()}>
            Retry
          </Button>
        </div>
      )}

      {loading && models.length === 0 ? (
        <SkeletonRows count={4} />
      ) : (
        <div className="flex flex-col gap-2">
          {models.map((model, i) => {
            const isDownloading = downloadingId === model.id;
            const isActivating = activatingId === model.id;
            const isActive = activeModelId === model.id;

            return (
              <ModelRow
                key={model.id}
                name={model.name}
                accent={isActive ? "green" : "amber"}
                rail={isActive || model.recommended ? "strong" : undefined}
                className="opacity-0 animate-slide-up"
                style={{
                  animationDelay: `${0.05 + i * 0.04}s`,
                  animationFillMode: "forwards",
                }}
                badges={
                  <>
                    {model.bundled && <Badge tone="neutral">Included</Badge>}
                    {model.family === "parakeet" && <Badge tone="violet">Parakeet · CPU</Badge>}
                    {model.recommended && <Badge tone="amber">Recommended</Badge>}
                    {isActive && <Badge tone="green">Active</Badge>}
                  </>
                }
                description={model.description}
                meta={[
                  formatBytes(model.size_bytes),
                  model.quantization,
                  model.language_support === "multilingual" ? "Multilingual" : "English",
                  `${model.capability_tier} tier`,
                  `~${memoryLabel(model.estimated_memory_mb)} RAM`,
                ]}
                action={
                  model.is_downloaded ? (
                    isActive ? (
                      <span className="inline-flex items-center gap-1.5 text-xs font-medium text-success">
                        <Check size={13} strokeWidth={2} />
                        In use
                      </span>
                    ) : (
                      <Button
                        size="sm"
                        variant="secondary"
                        loading={isActivating}
                        onClick={() => handleActivate(model.id)}
                      >
                        {isActivating ? "Loading…" : "Activate"}
                      </Button>
                    )
                  ) : (
                    <Button
                      size="sm"
                      variant="primary"
                      icon={<Download strokeWidth={2} />}
                      loading={isDownloading}
                      onClick={() => handleDownload(model.id)}
                    >
                      {isDownloading ? "Downloading" : "Download"}
                    </Button>
                  )
                }
              />
            );
          })}
        </div>
      )}

      {/* Hardware info — only shown in the Whisper tab since the
          "Recommended" callout references a Whisper model id. */}
      {hardware && (
        <Card
          className="mt-5 p-4 opacity-0 animate-slide-up"
          style={{ animationDelay: "0.35s", animationFillMode: "forwards" }}
        >
          <div className="mb-2.5 flex items-center gap-2">
            <Cpu size={13} strokeWidth={1.75} className="text-text-muted" />
            <span className="eyebrow">
              Hardware
            </span>
          </div>

          <div className="flex flex-wrap gap-x-8 gap-y-1 text-sm">
            <div>
              <span className="text-text-muted">CPU threads: </span>
              <span className="font-mono tabular-nums text-text-secondary">
                {hardware.cpu_cores}
              </span>
            </div>
            <div>
              <span className="text-text-muted">Active backend: </span>
              <span className="font-mono uppercase tabular-nums text-text-secondary">
                {hardware.compute_backend === "unknown"
                  ? "Not loaded"
                  : hardware.compute_backend}
              </span>
            </div>
            {hardware.measured_asr_tier && (
              <div>
                <span className="text-text-muted">Measured tier: </span>
                <span className="font-medium capitalize text-text-secondary">
                  {hardware.measured_asr_tier}
                </span>
              </div>
            )}
            <div>
              <span className="text-text-muted">Recommended: </span>
              <span className="font-medium text-amber-300">
                {models.find((m) => m.id === hardware.recommended_model)?.name ??
                  hardware.recommended_model}
              </span>
            </div>
          </div>
        </Card>
      )}
    </div>
  );
}
