/**
 * Maps KeyboardEvent.code values to Windows Virtual Key codes and display labels.
 *
 * Used by the hotkey recording UI to convert browser key events into the VK
 * codes the Rust low-level keyboard hook expects.
 */

export interface VkEntry {
  vk: number;
  label: string;
}

export const CODE_TO_VK: Record<string, VkEntry> = {
  // ── Modifiers ────────────────────────────────────────────
  ControlLeft:  { vk: 0xa2, label: "LCtrl" },
  ControlRight: { vk: 0xa3, label: "RCtrl" },
  AltLeft:      { vk: 0xa4, label: "LAlt" },
  AltRight:     { vk: 0xa5, label: "RAlt" },
  ShiftLeft:    { vk: 0xa0, label: "LShift" },
  ShiftRight:   { vk: 0xa1, label: "RShift" },
  MetaLeft:     { vk: 0x5b, label: "LWin" },
  MetaRight:    { vk: 0x5c, label: "RWin" },

  // ── Function keys ────────────────────────────────────────
  F1:  { vk: 0x70, label: "F1" },
  F2:  { vk: 0x71, label: "F2" },
  F3:  { vk: 0x72, label: "F3" },
  F4:  { vk: 0x73, label: "F4" },
  F5:  { vk: 0x74, label: "F5" },
  F6:  { vk: 0x75, label: "F6" },
  F7:  { vk: 0x76, label: "F7" },
  F8:  { vk: 0x77, label: "F8" },
  F9:  { vk: 0x78, label: "F9" },
  F10: { vk: 0x79, label: "F10" },
  F11: { vk: 0x7a, label: "F11" },
  F12: { vk: 0x7b, label: "F12" },
  F13: { vk: 0x7c, label: "F13" },
  F14: { vk: 0x7d, label: "F14" },
  F15: { vk: 0x7e, label: "F15" },
  F16: { vk: 0x7f, label: "F16" },
  F17: { vk: 0x80, label: "F17" },
  F18: { vk: 0x81, label: "F18" },
  F19: { vk: 0x82, label: "F19" },
  F20: { vk: 0x83, label: "F20" },

  // ── Letters ──────────────────────────────────────────────
  KeyA: { vk: 0x41, label: "A" },
  KeyB: { vk: 0x42, label: "B" },
  KeyC: { vk: 0x43, label: "C" },
  KeyD: { vk: 0x44, label: "D" },
  KeyE: { vk: 0x45, label: "E" },
  KeyF: { vk: 0x46, label: "F" },
  KeyG: { vk: 0x47, label: "G" },
  KeyH: { vk: 0x48, label: "H" },
  KeyI: { vk: 0x49, label: "I" },
  KeyJ: { vk: 0x4a, label: "J" },
  KeyK: { vk: 0x4b, label: "K" },
  KeyL: { vk: 0x4c, label: "L" },
  KeyM: { vk: 0x4d, label: "M" },
  KeyN: { vk: 0x4e, label: "N" },
  KeyO: { vk: 0x4f, label: "O" },
  KeyP: { vk: 0x50, label: "P" },
  KeyQ: { vk: 0x51, label: "Q" },
  KeyR: { vk: 0x52, label: "R" },
  KeyS: { vk: 0x53, label: "S" },
  KeyT: { vk: 0x54, label: "T" },
  KeyU: { vk: 0x55, label: "U" },
  KeyV: { vk: 0x56, label: "V" },
  KeyW: { vk: 0x57, label: "W" },
  KeyX: { vk: 0x58, label: "X" },
  KeyY: { vk: 0x59, label: "Y" },
  KeyZ: { vk: 0x5a, label: "Z" },

  // ── Digits ───────────────────────────────────────────────
  Digit0: { vk: 0x30, label: "0" },
  Digit1: { vk: 0x31, label: "1" },
  Digit2: { vk: 0x32, label: "2" },
  Digit3: { vk: 0x33, label: "3" },
  Digit4: { vk: 0x34, label: "4" },
  Digit5: { vk: 0x35, label: "5" },
  Digit6: { vk: 0x36, label: "6" },
  Digit7: { vk: 0x37, label: "7" },
  Digit8: { vk: 0x38, label: "8" },
  Digit9: { vk: 0x39, label: "9" },

  // ── Special keys ─────────────────────────────────────────
  Space:      { vk: 0x20, label: "Space" },
  Tab:        { vk: 0x09, label: "Tab" },
  CapsLock:   { vk: 0x14, label: "CapsLock" },
  Escape:     { vk: 0x1b, label: "Esc" },
  Backspace:  { vk: 0x08, label: "Backspace" },
  Enter:      { vk: 0x0d, label: "Enter" },
  Insert:     { vk: 0x2d, label: "Insert" },
  Delete:     { vk: 0x2e, label: "Delete" },
  Home:       { vk: 0x24, label: "Home" },
  End:        { vk: 0x23, label: "End" },
  PageUp:     { vk: 0x21, label: "PageUp" },
  PageDown:   { vk: 0x22, label: "PageDown" },
  Pause:      { vk: 0x13, label: "Pause" },
  ScrollLock: { vk: 0x91, label: "ScrollLock" },
  PrintScreen:{ vk: 0x2c, label: "PrtSc" },
  NumLock:    { vk: 0x90, label: "NumLock" },

  // ── Arrow keys ───────────────────────────────────────────
  ArrowUp:    { vk: 0x26, label: "Up" },
  ArrowDown:  { vk: 0x28, label: "Down" },
  ArrowLeft:  { vk: 0x25, label: "Left" },
  ArrowRight: { vk: 0x27, label: "Right" },

  // ── Punctuation / OEM ────────────────────────────────────
  Backquote:    { vk: 0xc0, label: "`" },
  Minus:        { vk: 0xbd, label: "-" },
  Equal:        { vk: 0xbb, label: "=" },
  BracketLeft:  { vk: 0xdb, label: "[" },
  BracketRight: { vk: 0xdd, label: "]" },
  Backslash:    { vk: 0xdc, label: "\\" },
  Semicolon:    { vk: 0xba, label: ";" },
  Quote:        { vk: 0xde, label: "'" },
  Comma:        { vk: 0xbc, label: "," },
  Period:       { vk: 0xbe, label: "." },
  Slash:        { vk: 0xbf, label: "/" },

  // ── Numpad ───────────────────────────────────────────────
  Numpad0:    { vk: 0x60, label: "Num0" },
  Numpad1:    { vk: 0x61, label: "Num1" },
  Numpad2:    { vk: 0x62, label: "Num2" },
  Numpad3:    { vk: 0x63, label: "Num3" },
  Numpad4:    { vk: 0x64, label: "Num4" },
  Numpad5:    { vk: 0x65, label: "Num5" },
  Numpad6:    { vk: 0x66, label: "Num6" },
  Numpad7:    { vk: 0x67, label: "Num7" },
  Numpad8:    { vk: 0x68, label: "Num8" },
  Numpad9:    { vk: 0x69, label: "Num9" },
  NumpadAdd:      { vk: 0x6b, label: "Num+" },
  NumpadSubtract: { vk: 0x6d, label: "Num-" },
  NumpadMultiply: { vk: 0x6a, label: "Num*" },
  NumpadDivide:   { vk: 0x6f, label: "Num/" },
  NumpadDecimal:  { vk: 0x6e, label: "Num." },
  NumpadEnter:    { vk: 0x0d, label: "NumEnter" },
};
