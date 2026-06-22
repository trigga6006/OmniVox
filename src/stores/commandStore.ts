import { create } from "zustand";

/**
 * Command-Mode UI state, driven by the backend's `command-state-change`,
 * `command-confirm`, and `command-result` events.  Kept separate from the
 * dictation `recordingStore` because the two captures are mutually exclusive
 * and the command pill is a visually distinct surface.
 */
export type CommandUiState =
  | "idle"
  | "listening" // Right Ctrl held, capturing
  | "recognizing" // transcribing + matching
  | "confirm" // low-confidence match awaiting Yes/No
  | "done" // executed successfully (transient)
  | "error"; // not recognized / failed (transient)

interface CommandState {
  state: CommandUiState;
  /** Result/confirm summary text, e.g. "Opened Spotify" or "Open Spotify?". */
  summary: string | null;
  setState: (state: CommandUiState, summary?: string | null) => void;
  reset: () => void;
}

export const useCommandStore = create<CommandState>((set) => ({
  state: "idle",
  summary: null,
  setState: (state, summary = null) => set({ state, summary }),
  reset: () => set({ state: "idle", summary: null }),
}));
