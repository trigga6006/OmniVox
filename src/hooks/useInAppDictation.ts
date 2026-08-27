import { useEffect, useRef } from "react";
import { onDictationInsert, onRecordingStateChange } from "@/lib/tauri";

type EditableEl = HTMLInputElement | HTMLTextAreaElement | HTMLElement;

const TEXT_INPUT_TYPES = ["text", "search", "url", "email", "tel", "password", "number"];

/** Can this element accept text inserted at its caret? */
function isEditable(el: Element | null): el is EditableEl {
  if (!el) return false;
  if (el instanceof HTMLTextAreaElement) return !el.readOnly && !el.disabled;
  if (el instanceof HTMLInputElement) {
    return TEXT_INPUT_TYPES.includes(el.type) && !el.readOnly && !el.disabled;
  }
  return el instanceof HTMLElement && el.isContentEditable;
}

/** Insert `text` at the caret, replacing any selection, and fire input events. */
function insertAtCaret(el: EditableEl, text: string) {
  el.focus();

  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const start = el.selectionStart ?? el.value.length;
    const end = el.selectionEnd ?? el.value.length;
    const next = el.value.slice(0, start) + text + el.value.slice(end);

    // Use the native value setter so React's controlled-input value tracker
    // sees the change and fires onChange (a plain `el.value = …` is silently
    // reverted on the next render).
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    const setter = Object.getOwnPropertyDescriptor(proto, "value")?.set;
    if (setter) setter.call(el, next);
    else el.value = next;

    const caret = start + text.length;
    el.setSelectionRange(caret, caret);
    el.dispatchEvent(new Event("input", { bubbles: true }));
    return;
  }

  // contenteditable — execCommand keeps the native undo stack intact.
  if (!document.execCommand("insertText", false, text)) {
    const sel = window.getSelection();
    if (sel && sel.rangeCount > 0) {
      const range = sel.getRangeAt(0);
      range.deleteContents();
      range.insertNode(document.createTextNode(text));
      range.collapse(false);
    }
  }
}

/**
 * Fired on `window` after a dictation was successfully inserted into a
 * focused editable in this window.  Same-window listeners that would
 * otherwise deliver the same dictation a second time (NotesPage's
 * append-to-open-note) use it to stand down.
 */
export const DICTATION_INSERTED_EVENT = "omnivox:dictation-inserted";

const dispatchInserted = (generation: number) => {
  window.dispatchEvent(
    new CustomEvent<number>(DICTATION_INSERTED_EVENT, { detail: generation })
  );
};

/**
 * Routes dictation aimed at OmniVox's own windows into the focused field.
 *
 * The backend can't reliably Ctrl+V into our WebView2 inputs, so when the
 * dictation target is one of our windows it emits `dictation-insert` instead.
 * We snapshot the focused editable element when recording starts (before any
 * UI change can blur it) and insert at its caret when the text arrives.
 */
export function useInAppDictation() {
  // Final ASR completions can arrive out of order after capture ownership has
  // already moved to a newer generation. Keep each generation's DOM target
  // independently so a late result cannot be dropped or redirected into the
  // newer capture's focused field.
  const targetsRef = useRef(new Map<number, EditableEl | null>());

  useEffect(() => {
    const unlisten = onRecordingStateChange((state, generation) => {
      if (state === "recording") {
        const el = document.activeElement;
        targetsRef.current.set(generation, isEditable(el) ? el : null);

        // Failed/empty captures have no insertion event to consume their
        // snapshot. Bound that exceptional residue without affecting normal
        // overlapping captures.
        if (targetsRef.current.size > 64) {
          const oldest = Math.min(...targetsRef.current.keys());
          targetsRef.current.delete(oldest);
        }
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = onDictationInsert(({ generation, text, target }) => {
      // Delivered to a DIFFERENT OmniVox window (e.g. the scratchpad). Stand
      // down here, and still fire DICTATION_INSERTED_EVENT so NotesPage's
      // append handler — which keys off it to avoid double-capturing — skips a
      // dictation that wasn't aimed at this window. (dictation-insert is emitted
      // before transcription-result, so the flag is set before Notes reacts.)
      if (target && target !== "main") {
        dispatchInserted(generation);
        targetsRef.current.delete(generation);
        return;
      }
      // The snapshotted editable can detach if its subtree unmounts mid-record
      // (e.g. navigating pages).  Inserting into a detached node silently drops
      // the dictation, so fall back to the live focused editable. (SS4)
      const snapshot = targetsRef.current.get(generation) ?? null;
      const newerCaptureExists = Array.from(targetsRef.current.keys()).some(
        (candidate) => candidate > generation
      );
      const el =
        snapshot && snapshot.isConnected
          ? snapshot
          : !newerCaptureExists && isEditable(document.activeElement)
            ? document.activeElement
            : null;
      if (el) {
        insertAtCaret(el, text);
        dispatchInserted(generation);
      }
      targetsRef.current.delete(generation);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);
}
