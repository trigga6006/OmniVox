import { create } from "zustand";

import { importCancel, importTranscribe } from "@/lib/tauri";

/**
 * State machine for the Import Audio feature.
 *
 * Wrapped under a single `state` field (not spread at the top level) so
 * action fields don't sit next to the discriminant — that's the only
 * shape that preserves TypeScript narrowing on selectors and forces
 * listeners to write a complete variant rather than spreading partial
 * updates (see `docs/IMPORT_AUDIO_PLAN.md` §7.2).
 */
export type ImportState =
  | { kind: "idle" }
  | { kind: "decoding"; importId: string; filename: string; percent: number }
  | {
      kind: "transcribing";
      importId: string;
      filename: string;
      percent: number;
      durationMs: number;
    }
  | {
      // The user clicked Cancel but the backend task hasn't released
      // its resources yet — Whisper's abort callback is polled between
      // decode steps, so there's a 1–10 s latency depending on model
      // size.  Stays in this state until `import-cancelled` arrives.
      kind: "cancelling";
      importId: string;
      filename: string;
    }
  | {
      kind: "complete";
      importId: string;
      filename: string;
      durationMs: number;
      transcript: string;
    }
  | {
      kind: "error";
      importId: string | null;
      filename: string | null;
      message: string;
    };

interface ImportActions {
  startImport: (path: string) => Promise<void>;
  cancelImport: () => Promise<void>;
  /** Reset back to idle after the user dismisses a complete/error state. */
  reset: () => void;
  /** Edit the transcript before Save to Note / Copy. */
  updateTranscript: (text: string) => void;
  /** Called by global listeners in App.tsx; do not call from components. */
  applyState: (next: ImportState) => void;
}

type ImportStore = { state: ImportState } & ImportActions;

const basename = (path: string): string => {
  const idx = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return idx >= 0 ? path.slice(idx + 1) : path;
};

export const useImportStore = create<ImportStore>((set, get) => ({
  state: { kind: "idle" },

  startImport: async (path: string) => {
    const current = get().state;
    if (
      current.kind === "decoding" ||
      current.kind === "transcribing" ||
      current.kind === "cancelling"
    ) {
      // Don't clobber an in-flight import.  Cancelling is treated as
      // busy because the backend hasn't released its resources yet.
      return;
    }
    const importId = crypto.randomUUID();
    set({
      state: {
        kind: "decoding",
        importId,
        filename: basename(path),
        percent: 0,
      },
    });
    try {
      // Backend returns immediately after spawning the task — every
      // `import-progress` event from here on writes to the store via
      // the global listeners mounted in App.tsx.
      await importTranscribe(importId, path);
    } catch (e) {
      const message = typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
      set({
        state: {
          kind: "error",
          importId,
          filename: basename(path),
          message,
        },
      });
    }
  },

  cancelImport: async () => {
    const current = get().state;
    if (current.kind !== "decoding" && current.kind !== "transcribing") return;
    // Move to the cancelling state immediately for UI feedback, but
    // keep the importId so we stay "busy" (record button disabled,
    // start_recording rejected) until the backend `import-cancelled`
    // event arrives.  Whisper's abort callback can take 5–10 s on
    // large models — flipping to idle here would let the user click
    // record and get a confusing "Busy" error.
    set({
      state: {
        kind: "cancelling",
        importId: current.importId,
        filename: current.filename,
      },
    });
    try {
      await importCancel(current.importId);
    } catch (e) {
      console.error("import_cancel failed:", e);
      // If the cancel call itself failed (e.g., id mismatch because
      // task already finished), drop back to idle so the user isn't
      // stuck on the cancelling spinner forever.
      set({ state: { kind: "idle" } });
    }
  },

  reset: () => set({ state: { kind: "idle" } }),

  updateTranscript: (text: string) => {
    const current = get().state;
    if (current.kind !== "complete") return;
    set({ state: { ...current, transcript: text } });
  },

  applyState: (next) => set({ state: next }),
}));

/**
 * Convenience selector — true while the backend still has the import's
 * resources held.  Includes `cancelling` because the backend hasn't
 * released the gate / `active_import` until the terminal event arrives.
 */
export const selectImportBusy = (s: ImportStore): boolean =>
  s.state.kind === "decoding" ||
  s.state.kind === "transcribing" ||
  s.state.kind === "cancelling";
