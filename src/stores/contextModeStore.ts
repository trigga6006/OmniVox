import { create } from "zustand";
import type { ContextMode } from "@/lib/tauri";

interface ContextModeState {
  modes: ContextMode[];
  activeMode: ContextMode | null;
  isLoaded: boolean;
  setModes: (modes: ContextMode[]) => void;
  setActiveMode: (mode: ContextMode | null) => void;
  setLoaded: (loaded: boolean) => void;
}

export const useContextModeStore = create<ContextModeState>((set) => ({
  modes: [],
  activeMode: null,
  isLoaded: false,
  setModes: (modes) => set({ modes }),
  setActiveMode: (mode) => set({ activeMode: mode }),
  setLoaded: (loaded) => set({ isLoaded: loaded }),
}));
