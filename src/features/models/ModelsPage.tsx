import { useCallback, useEffect, useState } from "react";
import { Download, Check, Cpu, Loader2 } from "lucide-react";
import { listModels, getHardwareInfo, downloadModel, setActiveModel } from "@/lib/tauri";
import { formatBytes, cn } from "@/lib/utils";
import type { ModelInfo, HardwareInfo } from "@/lib/tauri";

export function ModelsPage() {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [hardware, setHardware] = useState<HardwareInfo | null>(null);
  const [downloadingId, setDownloadingId] = useState<string | null>(null);
  const [activeModelId, setActiveModelId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [m, hw] = await Promise.all([listModels(), getHardwareInfo()]);
      setModels(m);
      setHardware(hw);
    } catch (err) {
      console.error("Failed to load models:", err);
    }
  }, []);

  useEffect(() => {
    refresh();
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
    <div className="flex h-full flex-col p-6 overflow-y-auto">
      {/* Header */}
      <div
        className="opacity-0 animate-slide-up"
        style={{ animationDelay: "0.05s", animationFillMode: "forwards" }}
      >
        <h1 className="font-display text-2xl text-text-primary">Models</h1>
        <p className="text-sm text-text-muted mt-1">
          Whisper speech recognition models
        </p>
      </div>

      {/* Model grid */}
      <div className="mt-6 grid gap-4 grid-cols-1 lg:grid-cols-2">
        {models.map((model, i) => {
          const isDownloading = downloadingId === model.id;
          const isActive = activeModelId === model.id;

          return (
            <div
              key={model.id}
              className={cn(
                "bg-surface-1 rounded-xl border border-border p-5 transition-colors hover:border-border-hover opacity-0 animate-slide-up",
                model.recommended && "border-l-[3px] border-l-amber-600"
              )}
              style={{
                animationDelay: `${0.1 + i * 0.08}s`,
                animationFillMode: "forwards",
              }}
            >
              {/* Name row: name + badges */}
              <div className="flex items-center gap-2 flex-wrap">
                <span className="font-medium text-text-primary">
                  {model.name}
                </span>

                {model.bundled && (
                  <span className="text-[10px] font-medium uppercase tracking-wider text-text-muted bg-surface-3 px-1.5 py-0.5 rounded">
                    Included
                  </span>
                )}

                {model.recommended && (
                  <span className="text-[10px] font-medium uppercase tracking-wider text-amber-400 bg-amber-500/10 px-1.5 py-0.5 rounded">
                    Recommended
                  </span>
                )}

                {isActive && (
                  <span className="text-[10px] font-medium uppercase tracking-wider text-success bg-success/10 px-1.5 py-0.5 rounded">
                    Active
                  </span>
                )}
              </div>

              {/* Size */}
              <p className="font-mono text-xs text-text-muted mt-1">
                {formatBytes(model.size_bytes)}
              </p>

              {/* Description */}
              <p className="text-sm text-text-secondary mt-3 leading-relaxed">
                {model.description}
              </p>

              {/* Footer: quantization + action button */}
              <div className="flex items-center justify-between mt-4 pt-4 border-t border-border">
                <span className="bg-surface-3 text-text-muted text-xs px-2 py-0.5 rounded-full">
                  {model.quantization}
                </span>

                {model.is_downloaded ? (
                  isActive ? (
                    <span className="inline-flex items-center gap-1.5 text-success text-sm font-medium">
                      <Check size={14} strokeWidth={2} />
                      In use
                    </span>
                  ) : (
                    <button
                      onClick={() => handleActivate(model.id)}
                      className="inline-flex items-center gap-1.5 bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors"
                    >
                      Activate
                    </button>
                  )
                ) : (
                  <button
                    onClick={() => handleDownload(model.id)}
                    disabled={isDownloading}
                    className={cn(
                      "inline-flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-sm font-medium transition-colors",
                      isDownloading
                        ? "bg-surface-3 text-text-muted cursor-not-allowed"
                        : "bg-amber-500/10 text-amber-400 hover:bg-amber-500/20"
                    )}
                  >
                    {isDownloading ? (
                      <>
                        <Loader2 size={14} className="animate-spin" />
                        Downloading…
                      </>
                    ) : (
                      <>
                        <Download size={14} strokeWidth={2} />
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

      {/* Hardware info */}
      {hardware && (
        <div
          className="mt-6 bg-surface-1 rounded-xl border border-border p-5 opacity-0 animate-slide-up"
          style={{ animationDelay: "0.45s", animationFillMode: "forwards" }}
        >
          <div className="flex items-center gap-2 mb-3">
            <Cpu size={16} strokeWidth={1.75} className="text-text-muted" />
            <span className="text-xs font-medium text-text-muted uppercase tracking-wider">
              Hardware
            </span>
          </div>

          <div className="flex flex-wrap gap-x-8 gap-y-2 text-sm">
            <div>
              <span className="text-text-muted">CPU threads: </span>
              <span className="font-mono text-text-secondary">
                {hardware.cpu_cores}
              </span>
            </div>
            <div>
              <span className="text-text-muted">Recommended model: </span>
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
