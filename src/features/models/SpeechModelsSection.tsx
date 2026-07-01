import { useCallback, useEffect, useState } from "react";
import { Download, Check, Cpu } from "lucide-react";
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
import { formatBytes, cn } from "@/lib/utils";
import { Button, Card, Badge } from "@/components/ui";

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
  const [activeModelId, setActiveModelId] = useState<string | null>(null);

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
    } catch (err) {
      console.error("Failed to load models:", err);
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

  const handleActivate = async (modelId: string) => {
    try {
      await setActiveModel(modelId);
      setActiveModelId(modelId);
    } catch (err) {
      console.error("Failed to activate model:", err);
    }
  };

  return (
    <div className="flex flex-col">
      {/* Whisper model list */}
      <div className="flex flex-col gap-2">
        {models.map((model, i) => {
          const isDownloading = downloadingId === model.id;
          const isActive = activeModelId === model.id;

          return (
            <Card
              key={model.id}
              className={cn(
                "flex items-center gap-4 px-5 py-3.5 opacity-0 transition-all duration-200 hover:border-border-hover animate-slide-up",
                isActive && "border-l-[3px] border-l-success/75",
                model.recommended && !isActive && "border-l-[3px] border-l-amber-500/70"
              )}
              style={{
                animationDelay: `${0.05 + i * 0.04}s`,
                animationFillMode: "forwards",
              }}
            >
              {/* Left: name + badges */}
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="text-[14px] font-medium text-text-primary">
                    {model.name}
                  </span>

                  {model.bundled && <Badge tone="neutral">Included</Badge>}

                  {model.recommended && <Badge tone="amber">Recommended</Badge>}

                  {isActive && <Badge tone="green">Active</Badge>}
                </div>

                <p className="mt-0.5 line-clamp-1 text-xs leading-relaxed text-text-muted">
                  {model.description}
                </p>
              </div>

              {/* Center: size + quant */}
              <div className="flex shrink-0 items-center gap-3">
                <span className="w-[70px] text-right font-mono text-xs tabular-nums text-text-muted">
                  {formatBytes(model.size_bytes)}
                </span>
                <span className="w-[40px] rounded-full bg-surface-2 px-1.5 py-0.5 text-center text-[10px] text-text-muted">
                  {model.quantization}
                </span>
              </div>

              {/* Right: action button */}
              <div className="flex w-[110px] shrink-0 justify-end">
                {model.is_downloaded ? (
                  isActive ? (
                    <span className="inline-flex items-center gap-1.5 text-xs font-medium text-success">
                      <Check size={13} strokeWidth={2} />
                      In use
                    </span>
                  ) : (
                    <Button
                      size="sm"
                      variant="secondary"
                      onClick={() => handleActivate(model.id)}
                    >
                      Activate
                    </Button>
                  )
                ) : (
                  <Button
                    size="sm"
                    variant="primary"
                    icon={<Download strokeWidth={2} />}
                    loading={isDownloading}
                    disabled={isDownloading}
                    onClick={() => handleDownload(model.id)}
                  >
                    {isDownloading ? "Downloading" : "Download"}
                  </Button>
                )}
              </div>
            </Card>
          );
        })}
      </div>

      {/* Hardware info — only shown in the Whisper tab since the
          "Recommended" callout references a Whisper model id. */}
      {hardware && (
        <Card
          className="mt-5 p-4 opacity-0 animate-slide-up"
          style={{ animationDelay: "0.35s", animationFillMode: "forwards" }}
        >
          <div className="mb-2.5 flex items-center gap-2">
            <Cpu size={13} strokeWidth={1.75} className="text-text-muted" />
            <span className="font-mono text-[10px] font-semibold uppercase tracking-[0.2em] text-text-muted">
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
