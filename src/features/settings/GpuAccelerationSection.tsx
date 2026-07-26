import { useCallback, useEffect, useState } from "react";
import { Loader2 } from "lucide-react";
import {
  getActiveModel,
  setActiveModel,
  getActiveLlmModel,
  setActiveLlmModel,
  getGpuSupport,
} from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { Card, Toggle } from "@/components/ui";

/* ─────────────────── GPU Acceleration Section ─────────────── */

export function GpuAccelerationSection({
  enabled,
  onToggle,
}: {
  enabled: boolean;
  onToggle: (enabled: boolean) => void | Promise<unknown>;
}) {
  const [reloading, setReloading] = useState(false);
  // null = still probing. Fail open so a probe error never hides a working toggle.
  const [supported, setSupported] = useState<boolean | null>(null);

  useEffect(() => {
    getGpuSupport().then(setSupported).catch(() => setSupported(true));
  }, []);

  const handleToggle = useCallback(async () => {
    const next = !enabled;
    setReloading(true);
    try {
      // Persist FIRST and await it — the model reloads re-read gpu_acceleration
      // from the DB, so they must not race the settings write.
      await onToggle(next);
      // Reload the active Whisper model so it picks up the new backend.
      try {
        const active = await getActiveModel();
        if (active) await setActiveModel(active.id);
      } catch (e) {
        console.error("Failed to reload Whisper model after GPU toggle:", e);
      }
      // Reload the Structured-Mode LLM too — otherwise Whisper switches backend
      // while the LLM keeps running on the old one until the next model change.
      try {
        const activeLlm = await getActiveLlmModel();
        if (activeLlm) await setActiveLlmModel(activeLlm.id);
      } catch (e) {
        console.error("Failed to reload LLM after GPU toggle:", e);
      }
    } catch (e) {
      // The settings write (onToggle) failed — surface it instead of letting it
      // escape as an unhandled rejection. The model reloads are correctly skipped
      // (there's no new setting to reload for).
      console.error("Failed to save GPU setting:", e);
    } finally {
      setReloading(false);
    }
  }, [enabled, onToggle]);

  const unsupported = supported === false;

  return (
    <Card
      className={cn(
        "animate-slide-up p-5 transition-colors",
        enabled ? "border-amber-500/20" : "hover:border-border-hover"
      )}
      style={{ opacity: 0, animationDelay: "0.08s", animationFillMode: "forwards" }}
    >
      <div className="mb-2.5">
        <span className="eyebrow">
          Performance
        </span>
      </div>

      <p className="mb-1.5 text-[13.5px] font-medium text-text-primary">
        GPU acceleration
      </p>
      <p className="mb-4 max-w-[400px] text-xs text-text-muted">
        Offload Whisper inference to your GPU via Vulkan for significantly faster
        transcription. Works with both AMD and NVIDIA GPUs.
      </p>

      <div className="flex items-center gap-3">
        <Toggle
          checked={enabled}
          onChange={handleToggle}
          disabled={reloading || unsupported}
          aria-label="GPU acceleration"
        />
        <span className="text-sm text-text-secondary">
          {unsupported ? (
            "Not available in this build"
          ) : reloading ? (
            <span className="flex items-center gap-1.5">
              <Loader2 size={13} strokeWidth={2} className="animate-spin text-amber-300" />
              Reloading models…
            </span>
          ) : enabled ? (
            "Enabled"
          ) : (
            "Disabled"
          )}
        </span>
      </div>
    </Card>
  );
}
