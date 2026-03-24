import { create } from "zustand";

export interface Toast {
  id: string;
  message: string;
  code?: string;
  level: "error" | "warn" | "info";
  /** Optional action button shown in the toast. */
  action?: {
    label: string;
    onClick: () => void;
  };
  /** Auto-dismiss timeout in ms (default 6000, set 0 to keep until dismissed). */
  duration?: number;
}

interface ToastState {
  toasts: Toast[];
  addToast: (toast: Omit<Toast, "id">) => void;
  removeToast: (id: string) => void;
}

let nextId = 0;

export const useToastStore = create<ToastState>((set) => ({
  toasts: [],
  addToast: (toast) => {
    const id = String(++nextId);
    set((s) => ({ toasts: [...s.toasts.slice(-4), { ...toast, id }] }));
    // Auto-dismiss (default 6s, longer for toasts with actions)
    const duration = toast.duration ?? (toast.action ? 10000 : 6000);
    if (duration > 0) {
      setTimeout(() => {
        set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
      }, duration);
    }
  },
  removeToast: (id) =>
    set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
