import { create } from "zustand";

export type Page = "dictation" | "history" | "analytics" | "dictionary" | "modes" | "notes" | "commands" | "models" | "settings";

const PAGES: Page[] = ["dictation", "history", "analytics", "dictionary", "modes", "notes", "commands", "models", "settings"];

const PAGE_KEY = "omnivox_current_page";

function getInitialPage(): Page {
  try {
    const saved = localStorage.getItem(PAGE_KEY);
    if (saved && PAGES.includes(saved as Page)) return saved as Page;
  } catch {
    // localStorage unavailable — fall through to default
  }
  return "dictation";
}

interface AppState {
  currentPage: Page;
  sidebarCollapsed: boolean;
  setPage: (page: Page) => void;
  toggleSidebar: () => void;
}

export const useAppStore = create<AppState>((set) => ({
  currentPage: getInitialPage(),
  sidebarCollapsed: false,
  setPage: (page) => {
    try {
      localStorage.setItem(PAGE_KEY, page);
    } catch {
      // best-effort persistence
    }
    set({ currentPage: page });
  },
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
}));
