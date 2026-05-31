import { useCallback, useState } from "react";
import { Loader2 } from "lucide-react";
import { getActiveModel, setActiveModel } from "@/lib/tauri";
import { cn } from "@/lib/utils";

/* ─────────────────── GPU Acceleration Section ─────────────── */

export function GpuAccelerationSection({
  enabled,
  onToggle,
}: {
  enabled: boolean;
  onToggle: (enabled: boolean) => void;
}) {
  const [reloading, setReloading] = useState(false);

  const handleToggle = useCallback(async () => {
    const next = !enabled;
    setReloading(true);
    onToggle(next);

    // Reload the active Whisper model so it picks up the new GPU setting.
    try {
      const active = await getActiveModel();
      if (active) {
        await setActiveModel(active.id);
      }
    } catch (e) {
      console.error("Failed to reload model after GPU toggle:", e);
    } finally {
      setReloading(false);
    }
  }, [enabled, onToggle]);

  return (
    <section
      className={cn(
        "bg-surface-1 rounded-xl border p-5 transition-colors animate-slide-up",
        enabled
          ? "border-amber-500/20"
          : "border-border hover:border-border-hover"
      )}
      style={{ opacity: 0, animationDelay: "0.08s", animationFillMode: "forwards" }}
    >
      <div className="mb-3">
        <span className="text-[10.5px] font-semibold uppercase tracking-[0.12em] text-text-muted">
          Performance
        </span>
      </div>

      <p className="text-[13.5px] font-medium text-text-primary mb-1.5">
        GPU acceleration
      </p>
      <p className="text-xs text-text-muted mb-4 max-w-[400px]">
        Offload Whisper inference to your GPU via Vulkan for significantly faster
        transcription. Works with both AMD and NVIDIA GPUs.
      </p>

      <div className="flex items-center gap-3">
        <button
          onClick={handleToggle}
          disabled={reloading}
          className={cn(
            "relative inline-flex h-[22px] w-10 items-center rounded-full transition-colors shadow-inner",
            enabled ? "bg-amber-500" : "bg-surface-3",
            reloading && "opacity-60"
          )}
        >
          <span
            className={cn(
              "inline-block h-[16px] w-[16px] rounded-full bg-white shadow-sm transition-transform duration-200",
              enabled ? "translate-x-[21px]" : "translate-x-[3px]"
            )}
          />
        </button>
        <span className="text-sm text-text-secondary">
          {reloading ? (
            <span className="flex items-center gap-1.5">
              <Loader2 size={13} strokeWidth={2} className="animate-spin text-amber-300" />
              Reloading model…
            </span>
          ) : enabled ? (
            "Enabled"
          ) : (
            "Disabled"
          )}
        </span>
      </div>
    </section>
  );
}
