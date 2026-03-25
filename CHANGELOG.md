# Changelog

## v0.1.7

### New Features

- **Command Send voice command** — Say "send" at the end of your dictation to press Enter and submit. Works independently from Ship Mode, so you control exactly when messages go out.
- **Command Send toggle in Settings** — New sub-slider within the Voice Commands section lets you enable or disable the "send" command independently from other voice commands.
- **"Send" added to voice commands reference** — The View Commands modal in Settings now lists all four voice commands including "send".
- **Ship button right-click popup** — Right-click the Rocket button in the floating pill's quick-toggle menu to open a compact Command Send toggle popup. Quickly switch between auto-sending everything (Ship Mode) and only sending when you say "send".
- **New mode creation now includes dictionary, snippets, and app bindings** — Creating a new context mode now transitions directly into edit mode, so you can immediately add custom words, snippets, and app bindings without having to re-open the mode.
- **"Developed by Omni Impact" branding** — The Settings About section now displays the Omni Impact logo and attribution alongside the version info.
- **macOS cross-platform support** — Added microphone and accessibility permission prompts, macOS-compatible hotkey handling, and platform-aware audio ducking. Includes install script for macOS/Linux dependencies.
- **Enhanced toast notification system** — Toast notifications now support multiple concurrent toasts, auto-dismiss timers, and different severity levels (info, success, warning, error).

### Improvements

- **Smoother floating pill popup animation** — The Command Send popup uses CSS opacity and scale transitions instead of window resizing for a smooth, flicker-free open/close.
- **Ship button right-click no longer toggles Ship Mode** — Right-clicking the Rocket button only opens the popup menu; left-click still toggles Ship Mode as before.
- **Floating pill overlay pre-allocates popup width** — The overlay window reserves space for the popup when the mode selector opens, eliminating flash/jump artifacts when toggling the popup.
- **"New Mode" button alignment fixed** — The button in the Context Modes header now aligns to the top of the title block instead of floating mid-way next to the description.
- **Unified placeholder text** — Dictionary inputs now consistently use "Heard as…" / "Replace with…" and snippet inputs use "Word…" / "Expands to…" across both the global dictionary page and mode-scoped editors.
- **Cross-platform error handling** — Improved error types with platform-specific variants for better debugging on macOS and Linux.
- **Hotkey system overhaul** — Refactored hotkey registration to work across Windows and macOS with platform-specific key code mapping.

### Bug Fixes

- **Fixed OOM crash on 16GB laptops** — Reduced memory usage during dictation to prevent out-of-memory crashes on systems with 16GB RAM.
- **Fixed output router Send command** — The Send voice command now correctly presses Enter via the output router when using Type Simulation or Both output modes.

---

## v0.1.6

- Voice commands system (new line, new paragraph, delete last word)
- Performance optimizations and chunked typing fix
- Reliable clipboard paste output
- Auto-switch fallback and UI polish
