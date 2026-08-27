import { X } from "lucide-react";
import { useToastStore } from "@/stores/toastStore";
import { LEVEL_TONE, TONE_ACTION, TONE_EDGE, TONE_TEXT } from "@/components/ui/tones";
import { cn } from "@/lib/utils";

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  const removeToast = useToastStore((s) => s.removeToast);

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex max-w-sm flex-col gap-2">
      {toasts.map((toast) => {
        // One semantic source: the level picks a tone, the tone supplies the
        // hairline, the text colour and the action surface. The toast keeps a
        // single neutral raised ground so the blur reads the same every time.
        const tone = LEVEL_TONE[toast.level];
        return (
          <div
            key={toast.id}
            className={cn(
              "flex animate-slide-up flex-col gap-2 rounded-[var(--radius-l)] border px-3.5 py-3",
              "bg-surface-1/95 shadow-[var(--shadow-lg)] backdrop-blur-md",
              TONE_EDGE[tone],
              TONE_TEXT[tone]
            )}
          >
            <div className="flex items-start gap-2">
              <p className="flex-1 text-xs leading-relaxed">{toast.message}</p>
              <button
                onClick={() => removeToast(toast.id)}
                aria-label="Dismiss"
                className={cn(
                  "pressable shrink-0 rounded-[var(--radius-s)] p-0.5 opacity-60",
                  "transition-[opacity,background-color,transform] duration-[var(--dur-2)] ease-out",
                  "hover:bg-white/[0.05] hover:opacity-100"
                )}
              >
                <X size={12} />
              </button>
            </div>
            {toast.action && (
              <button
                onClick={() => {
                  toast.action!.onClick();
                  removeToast(toast.id);
                }}
                className={cn(
                  "pressable self-start rounded-[var(--radius-s)] px-2.5 py-1 text-xs font-medium",
                  "transition-[background-color,transform] duration-[var(--dur-2)] ease-out",
                  TONE_ACTION[tone]
                )}
              >
                {toast.action.label}
              </button>
            )}
          </div>
        );
      })}
    </div>
  );
}
