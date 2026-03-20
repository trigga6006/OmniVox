import { useEffect } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";

export function useTauriEvent<T>(
  listenFn: (callback: (payload: T) => void) => Promise<UnlistenFn>,
  callback: (payload: T) => void
) {
  useEffect(() => {
    const unlisten = listenFn(callback);
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [listenFn, callback]);
}
