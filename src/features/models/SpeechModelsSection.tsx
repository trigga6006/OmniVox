import { useCallback, useEffect, useState } from "react";
import { Download, Check, Cpu, Loader2 } from "lucide-react";
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
            <div
              key={model.id}
              className={cn(
                "flex items-center gap-4 rounded-xl border border-border bg-surface-1/85 px-5 py-3.5 opacity-0 transition-all duration-200 hover:border-border-hover hover:bg-surface-1 animate-slide-up",
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

                  {model.bundled && (
                    <span className="rounded-md border border-border bg-surface-2 px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.10em] text-text-muted">
                      Included
                    </span>
                  )}

                  {model.recommended && (
                    <span className="rounded-md border border-amber-400/25 bg-amber-500/[0.10] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.10em] text-amber-300">
                      Recommended
                    </span>
                  )}

                  {isActive && (
                    <span className="rounded-md border border-success/30 bg-success/[0.10] px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.10em] text-success">
                      Active
                    </span>
                  )}
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
                    <button
                      onClick={() => handleActivate(model.id)}
                      className="inline-flex items-center gap-1 rounded-lg border border-amber-400/25 bg-amber-500/[0.10] px-3 py-1 text-xs font-medium text-amber-300 transition-colors hover:border-amber-400/45 hover:bg-amber-500/[0.18]"
                    >
                      Activate
                    </button>
                  )
                ) : (
                  <button
                    onClick={() => handleDownload(model.id)}
                    disabled={isDownloading}
                    className={cn(
                      "inline-flex items-center gap-1 rounded-lg border px-3 py-1 text-xs font-medium transition-colors",
                      isDownloading
                        ? "cursor-not-allowed border-border bg-surface-2 text-text-muted"
                        : "border-amber-400/25 bg-amber-500/[0.10] text-amber-300 hover:border-amber-400/45 hover:bg-amber-500/[0.18]"
                    )}
                  >
                    {isDownloading ? (
                      <>
                        <Loader2 size={12} className="animate-spin" />
                        Downloading
                      </>
                    ) : (
                      <>
                        <Download size={12} strokeWidth={2} />
                        Download
                      </>
                    )}
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>

      {/* Hardware info — only shown in the Whisper tab since the
          "Recommended" callout references a Whisper model id. */}
      {hardware && (
        <div
          className="mt-5 rounded-xl border border-border bg-surface-1/80 p-4 opacity-0 animate-slide-up"
          style={{ animationDelay: "0.35s", animationFillMode: "forwards" }}
        >
          <div className="mb-2.5 flex items-center gap-2">
            <Cpu size={13} strokeWidth={1.75} className="text-text-muted" />
            <span className="text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
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
        </div>
      )}
    </div>
  );
}
