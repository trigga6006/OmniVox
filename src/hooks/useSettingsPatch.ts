import { useCallback, useRef } from "react";
import { patchSettings as patchSettingsIpc, type AppSettings } from "@/lib/tauri";

type SettingsPatch =
  | Partial<AppSettings>
  | ((current: AppSettings) => Partial<AppSettings>);

export function useSettingsPatch(onChange?: (settings: AppSettings) => void) {
  const settingsRef = useRef<AppSettings | null>(null);
  const revisionRef = useRef<number | undefined>(undefined);

  const replaceSettings = useCallback(
    (settings: AppSettings, revision?: number) => {
      settingsRef.current = settings;
      if (revision !== undefined) revisionRef.current = revision;
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
        const snapshot = await patchSettingsIpc(patchValue, revisionRef.current);
        settingsRef.current = snapshot.settings;
        revisionRef.current = snapshot.revision;
        onChange?.(snapshot.settings);
        return snapshot.settings;
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
