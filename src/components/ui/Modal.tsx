import { useEffect, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

interface ModalProps {
  open: boolean;
  onClose: () => void;
  title?: string;
  description?: string;
  children?: ReactNode;
  /** Right-aligned action row at the bottom (usually Buttons). */
  footer?: ReactNode;
  className?: string;
}

/**
 * Portaled dialog: dimmed + blurred backdrop, centered panel, Esc + click-outside
 * to close, scroll lock. The app's first real shared modal/dialog primitive.
 */
export function Modal({ open, onClose, title, description, children, footer, className }: ModalProps) {
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    const prev = document.body.style.overflow;
    document.body.style.overflow = "hidden";
    return () => {
      window.removeEventListener("keydown", onKey);
      document.body.style.overflow = prev;
    };
  }, [open, onClose]);

  if (!open) return null;

  return createPortal(
    <div className="fixed inset-0 z-[100] flex items-center justify-center p-6" role="dialog" aria-modal="true">
      <div
        className="absolute inset-0 animate-fade-in bg-black/60 backdrop-blur-[2px]"
        onClick={onClose}
        aria-hidden="true"
      />
      <div
        className={cn(
          "relative z-10 w-full max-w-md animate-scale-in rounded-2xl border border-border-hover",
          "bg-surface-1 p-5 opacity-0 shadow-lg",
          className
        )}
      >
        {title && (
          <div className="mb-1 flex items-start justify-between gap-4">
            <h3 className="text-[15px] font-semibold text-text-primary">{title}</h3>
            <button
              onClick={onClose}
              aria-label="Close"
              className="-mr-1 -mt-1 rounded-md p-1 text-text-muted transition-colors hover:bg-surface-2 hover:text-text-primary"
            >
              <X size={16} />
            </button>
          </div>
        )}
        {description && (
          <p className="mb-4 text-[13px] leading-relaxed text-text-secondary">{description}</p>
        )}
        {children}
        {footer && <div className="mt-5 flex justify-end gap-2">{footer}</div>}
      </div>
    </div>,
    document.body
  );
}
