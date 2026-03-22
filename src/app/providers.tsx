import { useEffect, type ReactNode } from "react";
import { useSettingsStore } from "@/stores/settingsStore";
import { getSettings } from "@/lib/tauri";

export function Providers({ children }: { children: ReactNode }) {
  const theme = useSettingsStore((s) => s.theme);
  const setSettings = useSettingsStore((s) => s.setSettings);

  // Load persisted theme on mount
  useEffect(() => {
    getSettings()
      .then((s) => {
        setSettings({ theme: s.theme || "dark" });
      })
      .catch(() => {});
  }, []);

  // Apply theme attribute whenever it changes
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    try {
      localStorage.setItem("omnivox-theme", theme);
    } catch {}
  }, [theme]);

  return <>{children}</>;
}
