import { useCallback, useEffect, useState } from "react";
import { ArrowLeft, Download, Check, Sparkles, Loader2, Trash2 } from "lucide-react";
import {
  listLlmModels,
  downloadLlmModel,
  deleteLlmModel,
  setActiveLlmModel,
  getActiveLlmModel,
  onLlmDownloadProgress,
  onLlmModelLoaded,
  type LlmModelInfo,
} from "@/lib/tauri";
import { formatBytes, cn } from "@/lib/utils";

interface Props {
  onBack: () => void;
}

/**
 * Dedicated manager for Structured Mode LLM models.
 *
 * Parallel to `ModelsPage` for Whisper.  Kept as a separate page instead of
 * sharing because the two catalogs carry different metadata (GGUF quant vs.
 * whisper tier) and share no row-level UI.
 */
export function LlmModelsPage({ onBack }: Props) {
  const [models, setModels] = useState<LlmModelInfo[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [downloading, setDownloading] = useState<Record<string, number>>({});

  const refresh = useCallback(async () => {
    try {
      const [m, active] = await Promise.all([listLlmModels(), getActiveLlmModel()]);
      setModels(m);
      setActiveId(active?.id ?? null);
    } catch (err) {
      console.error("Failed to load LLM models:", err);
    }
  }, []);

  useEffect(() => {
    refresh();
    const unlistenProgress = onLlmDownloadProgress((p) => {
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
      unlistenProgress.then((fn) => fn());
      unlistenLoaded.then((fn) => fn());
    };
  }, [refresh]);

  const handleDownload = async (id: string) => {
    setDownloading((prev) => ({ ...prev, [id]: 0 }));
    try {
      await downloadLlmModel(id);
    } catch (err) {
      console.error("LLM download failed:", err);
    }
  };

  const handleActivate = async (id: string) => {
    try {
      await setActiveLlmModel(id);
      setActiveId(id);
    } catch (err) {
      console.error("Activate LLM failed:", err);
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await deleteLlmModel(id);
      await refresh();
    } catch (err) {
      console.error("Delete LLM failed:", err);
    }
  };

  return (
    <div className="flex h-full flex-col p-6 overflow-y-auto">
      <div className="flex items-center gap-3 mb-5">
        <button
          onClick={onBack}
          className="p-1.5 rounded-md hover:bg-white/5 text-text-muted hover:text-text-primary transition-colors"
        >
          <ArrowLeft size={16} />
        </button>
        <div>
          <h1 className="font-display font-semibold text-2xl text-text-primary flex items-center gap-2">
            <Sparkles size={18} className="text-violet-400" />
            Structured Mode Models
          </h1>
          <p className="text-sm text-text-muted mt-0.5">
            Local LLMs for slot-filled prompt output.
          </p>
        </div>
      </div>

      <div className="flex flex-col gap-2">
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
                animationDelay: `${0.05 + i * 0.04}s`,
                animationFillMode: "forwards",
              }}
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 mb-0.5">
                  <p className="font-medium text-text-primary">{m.name}</p>
                  {m.is_default && (
                    <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-violet-500/15 text-violet-300 uppercase tracking-wider">
                      Default
                    </span>
                  )}
                  {isActive && (
                    <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-emerald-500/15 text-emerald-300 uppercase tracking-wider flex items-center gap-1">
                      <Check size={8} /> Active
                    </span>
                  )}
                </div>
                <p className="text-xs text-text-muted leading-snug">
                  {m.description}
                </p>
                <p className="text-[10px] text-text-muted/70 mt-1">
                  {formatBytes(m.size_bytes)} · {m.quantization} ·{" "}
                  {(m.context_length / 1024).toFixed(0)}k context
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

              <div className="shrink-0 flex items-center gap-1.5">
                {!m.is_downloaded && !isDownloading && (
                  <button
                    onClick={() => handleDownload(m.id)}
                    className="flex items-center gap-1 px-2.5 py-1.5 text-[11px] rounded-md bg-violet-500/15 hover:bg-violet-500/25 border border-violet-400/30 text-violet-200 transition-colors"
                  >
                    <Download size={11} />
                    Download
                  </button>
                )}
                {isDownloading && (
                  <div className="flex items-center gap-1 px-2.5 py-1.5 text-[11px] text-violet-200">
                    <Loader2 size={11} className="animate-spin" />
                    {Math.round(progress ?? 0)}%
                  </div>
                )}
                {m.is_downloaded && !isActive && (
                  <button
                    onClick={() => handleActivate(m.id)}
                    className="flex items-center gap-1 px-2.5 py-1.5 text-[11px] rounded-md bg-white/5 hover:bg-white/10 border border-white/10 text-text-primary/80 transition-colors"
                  >
                    Use
                  </button>
                )}
                {m.is_downloaded && (
                  <button
                    onClick={() => handleDelete(m.id)}
                    className="p-1.5 rounded-md hover:bg-red-500/10 text-text-muted hover:text-red-300 transition-colors"
                    title="Delete"
                  >
                    <Trash2 size={12} />
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
