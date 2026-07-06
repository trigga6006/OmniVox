import { create } from "zustand";

// Theme is the only setting with a reactive main-window consumer: Providers
// applies `data-theme` to <html> whenever it changes.  Every other setting
// flows through AppSettings (getSettings / onSettingsChanged) — don't grow
// this store back into a parallel settings shape.
interface ThemeState {
  theme: string;
  setTheme: (theme: string) => void;
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: "dark",
  setTheme: (theme) => set({ theme }),
}));
