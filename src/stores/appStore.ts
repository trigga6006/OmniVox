import { create } from "zustand";

export type Page = "dictation" | "meetings" | "history" | "analytics" | "dictionary" | "modes" | "notes" | "commands" | "models" | "settings";

const PAGES: Page[] = ["dictation", "meetings", "history", "analytics", "dictionary", "modes", "notes", "commands", "models", "settings"];

const PAGE_KEY = "omnivox_current_page";

function getInitialPage(): Page {
  try {
    // Deep-link / headless-capture override: ?page=<name> wins over the
    // persisted page so screenshots and links can target a specific view.
    const requested = new URLSearchParams(window.location.search).get("page");
    if (requested && PAGES.includes(requested as Page)) return requested as Page;
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
