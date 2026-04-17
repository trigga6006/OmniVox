import { useEffect, useRef } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

/**
 * Subscribe to a Tauri event for the lifetime of the component.
 *
 * Callback identity is pinned via a ref so inline arrow functions passed as
 * `callback` don't cause the effect to tear down and re-install the IPC
 * listener on every render.  The old implementation had `[listenFn, callback]`
 * as deps, so any parent re-render that created a fresh callback would cross
 * the Tauri IPC boundary twice (unlisten + re-listen) — wasteful during
 * recording when parents re-render every 100-150 ms.
 */
export function useTauriEvent<T>(
  listenFn: (callback: (payload: T) => void) => Promise<UnlistenFn>,
  callback: (payload: T) => void
) {
  // Keep the ref always pointing at the latest callback.  Updating a ref
  // during render is safe (no reactive effect, no double-invoke in Strict Mode).
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    let unlistenFn: UnlistenFn | null = null;
    let cancelled = false;

    // Install the listener with a stable forwarder that always reads the
    // current callback from the ref.  Identity-stable across renders.
    listenFn((payload) => callbackRef.current(payload)).then((fn) => {
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
    // Intentionally omit `callback` from deps — it's routed through the ref.
    // Including it would defeat the whole optimization.
  }, [listenFn]);
}
