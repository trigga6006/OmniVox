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
                "bg-surface-1 rounded-xl border border-border px-5 py-3.5 transition-colors hover:border-border-hover opacity-0 animate-slide-up flex items-center gap-4",
                isActive && "border-l-[3px] border-l-success/60",
                model.recommended && !isActive && "border-l-[3px] border-l-amber-600"
              )}
              style={{
                animationDelay: `${0.05 + i * 0.04}s`,
                animationFillMode: "forwards",
              }}
            >
              {/* Left: name + badges */}
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="font-medium text-text-primary text-sm">
                    {model.name}
                  </span>

                  {model.bundled && (
                    <span className="text-[9px] font-medium uppercase tracking-wider text-text-muted bg-surface-3 px-1.5 py-0.5 rounded">
                      Included
                    </span>
                  )}

                  {model.recommended && (
                    <span className="text-[9px] font-medium uppercase tracking-wider text-amber-400 bg-amber-500/10 px-1.5 py-0.5 rounded">
                      Recommended
                    </span>
                  )}

                  {isActive && (
                    <span className="text-[9px] font-medium uppercase tracking-wider text-success bg-success/10 px-1.5 py-0.5 rounded">
                      Active
                    </span>
                  )}
                </div>

                <p className="text-xs text-text-muted mt-0.5 leading-relaxed line-clamp-1">
                  {model.description}
                </p>
              </div>

              {/* Center: size + quant */}
              <div className="flex items-center gap-3 shrink-0">
                <span className="font-mono text-xs text-text-muted w-[70px] text-right">
                  {formatBytes(model.size_bytes)}
                </span>
                <span className="bg-surface-3 text-text-muted text-[10px] px-1.5 py-0.5 rounded-full w-[38px] text-center">
                  {model.quantization}
                </span>
              </div>

              {/* Right: action button */}
              <div className="shrink-0 w-[100px] flex justify-end">
                {model.is_downloaded ? (
                  isActive ? (
                    <span className="inline-flex items-center gap-1.5 text-success text-xs font-medium">
                      <Check size={13} strokeWidth={2} />
                      In use
                    </span>
                  ) : (
                    <button
                      onClick={() => handleActivate(model.id)}
                      className="inline-flex items-center gap-1 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 rounded-lg px-3 py-1 text-xs font-medium transition-colors"
                    >
                      Activate
                    </button>
                  )
                ) : (
                  <button
                    onClick={() => handleDownload(model.id)}
                    disabled={isDownloading}
                    className={cn(
                      "inline-flex items-center gap-1 rounded-lg px-3 py-1 text-xs font-medium transition-colors",
                      isDownloading
                        ? "bg-surface-3 text-text-muted cursor-not-allowed"
                        : "bg-amber-500/10 text-amber-400 hover:bg-amber-500/20"
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
          className="mt-5 bg-surface-1 rounded-xl border border-border p-4 opacity-0 animate-slide-up"
          style={{ animationDelay: "0.35s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-2">
            <Cpu size={14} strokeWidth={1.75} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Hardware
            </span>
          </div>

          <div className="flex flex-wrap gap-x-8 gap-y-1 text-sm">
            <div>
              <span className="text-text-muted">CPU threads: </span>
              <span className="font-mono text-text-secondary">
                {hardware.cpu_cores}
              </span>
            </div>
            <div>
              <span className="text-text-muted">Recommended: </span>
              <span className="text-amber-400 font-medium">
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
