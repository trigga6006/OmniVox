import { X } from "lucide-react";
import { useToastStore } from "@/stores/toastStore";

const levelStyles = {
  error: "border-red-500/30 bg-red-500/10 text-red-300",
  warn: "border-amber-500/30 bg-amber-500/10 text-amber-300",
  info: "border-blue-500/30 bg-blue-500/10 text-blue-300",
};

const actionStyles = {
  error: "bg-red-500/20 hover:bg-red-500/30 text-red-200",
  warn: "bg-amber-500/20 hover:bg-amber-500/30 text-amber-200",
  info: "bg-blue-500/20 hover:bg-blue-500/30 text-blue-200",
};

export function ToastContainer() {
  const toasts = useToastStore((s) => s.toasts);
  const removeToast = useToastStore((s) => s.removeToast);

  if (toasts.length === 0) return null;

  return (
    <div className="fixed bottom-4 right-4 z-50 flex flex-col gap-2 max-w-sm">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`flex flex-col gap-2 rounded-lg border px-3 py-2.5 shadow-lg backdrop-blur-sm animate-in slide-in-from-right-2 ${levelStyles[toast.level]}`}
        >
          <div className="flex items-start gap-2">
            <p className="flex-1 text-xs leading-relaxed">{toast.message}</p>
            <button
              onClick={() => removeToast(toast.id)}
              className="shrink-0 opacity-60 hover:opacity-100 transition-opacity"
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
