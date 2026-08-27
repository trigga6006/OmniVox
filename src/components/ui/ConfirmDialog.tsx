import { useEffect, type ReactNode } from "react";
import { Button } from "./Button";
import { Modal } from "./Modal";

interface ConfirmDialogProps {
  open: boolean;
  title: string;
  /**
   * Verb-first and specific — "Delete meeting", "Discard draft".
   * Required on purpose: there is no default, so no dialog can ship "OK".
   */
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
  description?: string;
  /** Extra body content between the description and the buttons. */
  children?: ReactNode;
  cancelLabel?: string;
  tone?: "default" | "danger";
  /** Spins the confirm button and blocks re-entry while the action runs. */
  loading?: boolean;
}

/**
 * The replacement for `window.confirm()`, which rendered an OS dialog in the
 * middle of custom chrome with a button labelled "OK" that said nothing about
 * what was about to happen.
 */
export function ConfirmDialog({
  open,
  title,
  confirmLabel,
  onConfirm,
  onCancel,
  description,
  children,
  cancelLabel = "Cancel",
  tone = "default",
  loading,
}: ConfirmDialogProps) {
  // Escape must dismiss only the TOP dialog. Modal listens on `window` in the
  // bubble phase, and a modal underneath registered its listener FIRST, so both
  // would fire and the confirm would take the parent dialog (and whatever the
  // user had typed in it) down with it. Claim Escape here in the capture phase,
  // before either window listener sees it, and close only this dialog.
  //
  // Consequence: an interactive child that needs its own Escape (a Select, say)
  // cannot live inside a ConfirmDialog — none does; it is a title/description/
  // confirm dialog by construction.
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      onCancel();
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [open, onCancel]);

  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={title}
      description={description}
      footer={
        <>
          <Button variant="ghost" onClick={onCancel} disabled={loading}>
            {cancelLabel}
          </Button>
          <Button
            variant={tone === "danger" ? "danger" : "primary"}
            onClick={onConfirm}
            loading={loading}
            autoFocus
          >
            {confirmLabel}
          </Button>
        </>
      }
    >
      {children}
    </Modal>
  );
}
