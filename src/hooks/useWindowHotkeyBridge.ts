import { useEffect } from "react";
import { feedHotkeyEvent } from "@/lib/tauri";

/**
 * Bridges hotkey presses that happen while an OmniVox window is focused.
 *
 * The backend's global keyboard hook (WH_KEYBOARD_LL on Windows) receives no
 * key events while our own WebView has focus — the WebView consumes them before
 * the OS hook runs — so dictation/command hotkeys never fire when the user is
 * inside the app.  Here we listen for the modifier keys the WebView *does* see
 * and forward them to the same backend state machine.  The OS hook handles the
 * other-app-focused case; this handles the ours-focused case.  They're mutually
 * exclusive by focus.
 */

// DOM `KeyboardEvent.code` → Windows VK code, for the modifier keys that make up
// hotkey combos (default dictation = LCtrl+LAlt, default command = RCtrl).
const CODE_TO_VK: Record<string, number> = {
  ControlLeft: 0xa2,
  ControlRight: 0xa3,
  AltLeft: 0xa4,
  AltRight: 0xa5,
  ShiftLeft: 0xa0,
  ShiftRight: 0xa1,
  MetaLeft: 0x5b,
  MetaRight: 0x5c,
};

export function useWindowHotkeyBridge() {
  useEffect(() => {
    // Serialize forwarded events so the backend processes them in DOM order.
    // The state machine relies on a key-down's "claim" completing before the
    // matching key-up's "stop"; separate async commands could otherwise reorder
    // on a fast tap and leave a recording stuck on.
    let chain: Promise<unknown> = Promise.resolve();
    const enqueue = (vk: number, down: boolean) => {
      chain = chain.then(() => feedHotkeyEvent(vk, down).catch(() => {}));
    };

    const onDown = (e: KeyboardEvent) => {
      if (e.repeat) return; // ignore auto-repeat; the state machine only needs edges
      const vk = CODE_TO_VK[e.code];
      if (vk !== undefined) enqueue(vk, true);
    };
    const onUp = (e: KeyboardEvent) => {
      const vk = CODE_TO_VK[e.code];
      if (vk !== undefined) enqueue(vk, false);
    };

    window.addEventListener("keydown", onDown, true);
    window.addEventListener("keyup", onUp, true);
    return () => {
      window.removeEventListener("keydown", onDown, true);
      window.removeEventListener("keyup", onUp, true);
    };
  }, []);
}
