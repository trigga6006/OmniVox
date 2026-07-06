import { useEffect, type ReactNode } from "react";
import { useThemeStore } from "@/stores/themeStore";
import { getSettings } from "@/lib/tauri";

export function Providers({ children }: { children: ReactNode }) {
  const theme = useThemeStore((s) => s.theme);
  const setTheme = useThemeStore((s) => s.setTheme);

  // Load persisted theme on mount
  useEffect(() => {
    getSettings()
      .then((s) => {
        setTheme(s.theme || "dark");
      })
      .catch(() => {});
  }, []);

  // Apply theme attribute whenever it changes.  The localStorage write is
  // read by the inline flash-prevention script in index.html.
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("omnivox-theme", theme);
    } catch {}
  }, [theme]);

  return <>{children}</>;
}
