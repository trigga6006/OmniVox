import { useEffect } from "react";
import { getSettings, onSettingsChanged, type AppSettings } from "@/lib/tauri";

/**
 * One wiring point for "load settings now and stay in sync".
 *
 * Fetches the current AppSettings once on mount and re-invokes `apply` on
 * every `settings-changed` broadcast (the two webview windows run isolated
 * JS runtimes, so this event is how they stay coherent).  Pass a stable
 * callback (useCallback) — the subscription re-arms when it changes.
 */
export function useSettingsSync(apply: (s: AppSettings) => void) {
  useEffect(() => {
    let disposed = false;
    getSettings()
      .then((s) => {
        if (!disposed) apply(s);
      })
      .catch(() => {});

    const unlisten = onSettingsChanged((s) => apply(s));
    return () => {
      disposed = true;
      unlisten.then((fn) => fn());
    };
  }, [apply]);
}
