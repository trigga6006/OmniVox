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
  setStatus: (status: RecordingStatus) => void;
  setDuration: (duration: number) => void;
  setAudioLevel: (level: number) => void;
  setLastTranscription: (text: string) => void;
  reset: () => void;
}

export const useRecordingStore = create<RecordingState>((set) => ({
  status: "idle",
  duration: 0,
  audioLevel: 0,
  lastTranscription: null,
  setStatus: (status) => set({ status }),
  setDuration: (duration) => set({ duration }),
  setAudioLevel: (level) => set({ audioLevel: level }),
  setLastTranscription: (text) => set({ lastTranscription: text }),
  reset: () =>
    set({
      status: "idle",
      duration: 0,
      audioLevel: 0,
      lastTranscription: null,
    }),
}));
