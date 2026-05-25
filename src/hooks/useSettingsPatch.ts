import { useCallback, useRef } from "react";
import { updateSettings, type AppSettings } from "@/lib/tauri";

type SettingsPatch =
  | Partial<AppSettings>
  | ((current: AppSettings) => Partial<AppSettings>);

export function useSettingsPatch(onChange?: (settings: AppSettings) => void) {
  const settingsRef = useRef<AppSettings | null>(null);

  const replaceSettings = useCallback(
    (settings: AppSettings) => {
      settingsRef.current = settings;
      onChange?.(settings);
    },
    [onChange]
  );

  const patchSettings = useCallback(
    async (patch: SettingsPatch): Promise<AppSettings> => {
      const current = settingsRef.current;
      if (!current) {
        throw new Error("settings not loaded");
      }

      const patchValue = typeof patch === "function" ? patch(current) : patch;
      const updated: AppSettings = { ...current, ...patchValue };
      settingsRef.current = updated;
      onChange?.(updated);

      try {
        await updateSettings(updated);
        return updated;
      } catch (error) {
        settingsRef.current = current;
        onChange?.(current);
        throw error;
      }
    },
    [onChange]
  );

  return { settingsRef, replaceSettings, patchSettings };
}
