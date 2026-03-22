import { create } from "zustand";

export type Page = "dictation" | "history" | "dictionary" | "notes" | "models" | "settings";

interface AppState {
  currentPage: Page;
  sidebarCollapsed: boolean;
  setPage: (page: Page) => void;
  toggleSidebar: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  currentPage: "dictation",
  sidebarCollapsed: false,
  setPage: (page) => set({ currentPage: page }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
}));
