import { create } from "zustand";

export type RecordingStatus =
  | "idle"
  | "recording"
  | "processing"
  | "structuring"
  | "error";

interface RecordingState {
  status: RecordingStatus;
  duration: number;
  audioLevel: number;
  lastTranscription: string | null;
  error: string | null;
  setStatus: (status: RecordingStatus) => void;
  setDuration: (duration: number) => void;
  setAudioLevel: (level: number) => void;
  setLastTranscription: (text: string) => void;
  setError: (error: string | null) => void;
  reset: () => void;
}

export const useRecordingStore = create<RecordingState>((set) => ({
  status: "idle",
  duration: 0,
  audioLevel: 0,
  lastTranscription: null,
  error: null,
  setStatus: (status) => set({ status }),
  setDuration: (duration) => set({ duration }),
  setAudioLevel: (level) => set({ audioLevel: level }),
  setLastTranscription: (text) => set({ lastTranscription: text }),
  setError: (error) => set({ error }),
  reset: () =>
    set({
      status: "idle",
      duration: 0,
      audioLevel: 0,
      lastTranscription: null,
      error: null,
    }),
}));
