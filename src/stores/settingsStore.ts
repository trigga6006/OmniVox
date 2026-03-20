import { create } from "zustand";

interface Settings {
  theme: string;
  language: string;
  autoStart: boolean;
  minimizeToTray: boolean;
  outputMode: string;
  sampleRate: number;
  activeModelId: string | null;
}

interface SettingsState extends Settings {
  isLoaded: boolean;
  setSettings: (settings: Partial<Settings>) => void;
  setLoaded: (loaded: boolean) => void;
}

export const useSettingsStore = create<SettingsState>((set) => ({
  theme: "dark",
  language: "en",
  autoStart: false,
  minimizeToTray: true,
  outputMode: "clipboard",
  sampleRate: 16000,
  activeModelId: null,
  isLoaded: false,
  setSettings: (settings) => set(settings),
  setLoaded: (loaded) => set({ isLoaded: loaded }),
}));
