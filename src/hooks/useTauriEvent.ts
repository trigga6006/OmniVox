import { useEffect } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

export function useTauriEvent<T>(
  listenFn: (callback: (payload: T) => void) => Promise<UnlistenFn>,
  callback: (payload: T) => void
) {
  useEffect(() => {
    let unlistenFn: UnlistenFn | null = null;
    let cancelled = false;

    listenFn(callback).then((fn) => {
      if (cancelled) {
        // Component unmounted before listener was set up — clean up immediately
        fn();
      } else {
        unlistenFn = fn;
      }
    });

    return () => {
      cancelled = true;
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [listenFn, callback]);
}
