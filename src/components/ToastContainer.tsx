import { X } from "lucide-react";
import { useToastStore } from "@/stores/toastStore";

const levelStyles = {
  error: "border-error/30 bg-error/[0.10] text-error",
  warn: "border-amber-400/30 bg-amber-500/[0.10] text-amber-200",
  info: "border-border bg-surface-1/95 text-text-primary",
};

const actionStyles = {
  error: "bg-error/20 hover:bg-error/30 text-error",
  warn: "bg-amber-500/20 hover:bg-amber-500/30 text-amber-200",
  info: "bg-surface-2 hover:bg-surface-3 text-text-primary",
};

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  const removeToast = useToastStore((s) => s.removeToast);

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex max-w-sm flex-col gap-2">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`flex flex-col gap-2 rounded-xl border px-3.5 py-3 shadow-lg backdrop-blur-md animate-slide-up ${levelStyles[toast.level]}`}
        >
          <div className="flex items-start gap-2">
            <p className="flex-1 text-[13px] leading-relaxed">{toast.message}</p>
            <button
              onClick={() => removeToast(toast.id)}
              className="shrink-0 rounded-md p-0.5 opacity-60 transition-opacity hover:bg-white/[0.05] hover:opacity-100"
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
              className={`self-start rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${actionStyles[toast.level]}`}
            >
              {toast.action.label}
            </button>
          )}
        </div>
      ))}
    </div>
  );
}
