# Changelog

## v0.2.5

### New Features

- **"Raw" paste button in the Structured panel** — drop-in escape hatch when the LLM misreads your dictation. Sits right of the primary Paste button, styled amber so it reads as a lighter-weight commit than the structured one. Pastes the exact pre-structuring ASR transcript via the existing `paste_structured_output` command (the parameter is misnamed "markdown" but accepts any string).
- **Full-bar dictation mode in the Structured panel** — when you fire a dictation into an open preview, the action bar now collapses every button except the mic entirely out of layout and expands the mic to fill the full row with a "Listening · click to stop" label. Replaces the earlier shrink-to-icons approach, which left awkward dead space between icon pills and the recording waveform.
- **Models page: tabbed layout + Structured Mode live here now** — the page is split into **Speech Recognition** and **LLM Structuring** tabs (amber / violet accents match the rest of the design system). Structured Mode config (enable toggle, min-chars slider, LLM timeout slider, Test prompt button) moved out of Settings into a compact strip above the LLM model list. Settings no longer has a Structured Mode card; one hub for "pick + tune your models."
- **More word-count milestones, up to one million** — the previous ladder topped out at "Prolific Author" (100k). Added 12 new tiers with real-book word-count references: The Great Gatsby × 2.5 (125k), Literary Luminary (150k), Fellowship Scribe (200k — *Fellowship of the Ring*), Moby-Dick Whisperer (250k), Epic Pen (300k — *Anna Karenina*), Saga Weaver (400k — *It*), Voice of an Era (500k — *Les Misérables*), Tolstoy's Peer (587k — *War and Peace*), Atlas Lifter (645k — *Atlas Shrugged*), Scripturist (783k — KJV), The Bard Incarnate (884k — complete Shakespeare), Million-Word Sage (1,000,000).

### Improvements

- **Pill animation polish pass** — three frame-level flickers hunted down:
  - The active-content opacity transition was symmetric (200 ms out / 200 ms in), but the 80 ms resize window cut the fade-out off at ~60 % opacity before flipping `showContent` back to true — so the content visibly dimmed and brightened on every expand. Hide is now instant; show is a 220 ms fade-in with a 40 ms grace delay. No more dim-then-brighten.
  - Idle → active previously transitioned the pill border from `0 px` to `1 px solid <state>/30`, which can't interpolate and snapped. Every state now carries `border border-transparent` as the base, so only the *color* changes; the 1 px border is always present and transitions smoothly over 200 ms.
  - Removed the 200 ms "expanded → idle fade-out wait" that was load-bearing back when the opacity fade ran in both directions. With the hide now instant, that delay was pure dead time — the pill snapped to idle-size while the window stayed expanded for a fifth of a second, leaving a tiny pill floating in an oversized transparent window. Resize is immediate on expanded → idle now.
- **Dismiss button tightened** — dropped the `ESC` kbd chip from the Structured panel's Dismiss button. Gained back ~28 px of horizontal space in the action bar so the mic button no longer gets clipped by the panel's right edge. Tooltip still surfaces the shortcut on hover.
- **About version auto-syncs** — Settings → About now reads the app version from `tauri.conf.json` via `getVersion()` instead of hardcoding. No more stale "v0.2.1" shown while the app is actually at 0.2.5.
- **LLM Structuring tab — no delete** — removed the Trash2 button from each LLM model row. Language models are download-only from the UI now; prevents the easy "oops, I just nuked my active Structured Mode model" mistake.
- **Voxify alias extension and more** — carry-over from v0.2.4.

### Bug Fixes

- **"No Structured Mode models in the catalog" empty state** — the new LlmModelsSection was using a `mountedRef` pattern that flipped to `false` during StrictMode's mount → cleanup → remount cycle and never reset, so every `setModels` / `setSettings` call was silently dropped. Added an explicit `mountedRef.current = true` on mount. (This was also why the section appeared unclickable — there were no rows to click.)

## v0.2.4

### Bug Fixes

- **Structured panel and paste stuck after LLM finishes** — v0.2.3 gated `StructuredPanel` (and the degraded banner) on `showContent` to cover a WebView2 composition race. That coupled the panel's mount to the resize effect's 80 ms timer, which the effect released via its cleanup function. Because `pipeline.rs` emits `structured-output-ready` and `recording-state-change: idle` back-to-back, the effect ran twice: the first run scheduled the `setShowContent(true)` timer; the second run's cleanup cancelled it, and its body returned early (`sizeChanged === false`) without rescheduling. Result: `showContent` stayed `false` forever, the panel never mounted, and since auto-paste is skipped whenever `structured.is_some()`, nothing landed in the focused app. The timer now lives in a ref that incidental re-runs leave alone; a dedicated unmount effect clears it.
- **Overlay reposition race** — Added the Windows-specific `SetWindowPos` atomic size+position apply that was intended for v0.2.3 but didn't make it into that commit. Without it, the fallback path runs `set_size` then `set_position` as two separate IPC calls and the overlay briefly exists at the old position with the new (much larger) size.

### Improvements

- **More Voxify aliases** — Added no-`i` variants (`voxfy`, `foxfy`, `boxfy`, `poxfy`, `woxfy`, `vexfy`, `vaxfy`, `oxfy`) to the trigger list. Whisper collapses the `/i/` between `/f/` and `/aɪ/` to a schwa when the user says "Voxify" quickly, and the no-`i` spelling is what lands in the transcript. All still non-lexical, so no false activations.

## v0.2.3

### Bug Fixes

- **Right-click flicker on primary monitor** — Opening the context menu briefly flashed the pill/menu at the top-left of the newly-expanded region. `SetWindowPos` resizes the overlay atomically on the Windows thread, but WebView2 can paint the pre-resize React layout into the new window bounds for a single frame before re-laying-out. The `ModeSelector` / `StructuredPanel` / degraded banner are now gated on the existing `showContent` flag, which resets on every size change and flips back to true 80 ms later — long enough for WebView2 to settle.
- **Degraded banner no longer clips the context menu** — When the "dictation too short" banner was up and the user right-clicked the pill, the banner stayed and cut off the menu. Right-clicking now dismisses the banner and opens the menu. The menu is also allowed from the transient `success` / `error` pill states (not just `idle`), since the banner commonly shows while the pill is still in `success`.

## v0.2.2

### Bug Fixes

- **Windows release build fixed (take 2)** — CI was invoking `cargo tauri build` under `shell: bash`, which prepends Git Bash's `/usr/bin` to PATH. That shadowed MSVC's `link.exe` with GNU coreutils' `link` (a hardlink utility), producing `"/usr/bin/link: extra operand ..."` errors on every build-script link (proc-macro2, serde_core, zerocopy, …). The build step now runs under `pwsh` so the MSVC toolchain set up by `ilammy/msvc-dev-cmd` stays first on PATH.

## v0.2.1

### Bug Fixes

- **Windows release build fixed** — CI was using the Visual Studio / MSBuild generator which races on `llama-cpp-sys-2`'s `vulkan-shaders-gen` subproject (install step runs before build completes). The release workflow now uses the Ninja generator to match local dev, and explicitly sets up the MSVC environment and installs Ninja. Also meaningfully faster.

## v0.2.0

### New Features

- **Structured Mode** — Optional local-LLM pipeline that takes raw dictation and turns it into a slot-filled Markdown prompt tuned for agentic coding agents (Claude Code, Codex). Runs via llama.cpp with a GBNF grammar so the output is always shape-valid JSON. Degrades gracefully to plain output on timeout, missing model, or parse failure.
- **Structured Mode is intent-aware** — Three intent shapes recognised: implementation (goal / context / constraints / files / urgency / expected behavior), exploration (goal / context / questions), and advice (goal / context / options / constraints). The model picks the slots that fit the dictation instead of padding every field.
- **Structured preview panel** — After a structured run, a premium 420 px panel flows out of the pill showing the Markdown preview with metadata chips (urgency, file count), Edit / Paste (⌘↵) / Copy / Dismiss actions, a collapsible Raw Transcript drawer, and a built-in mic for dictating appends into the panel.
- **In-panel dictation** — Mic button on the preview panel lets you speak additions that get appended to the preview. Hotkey-triggered recordings while the panel is open also route into the textarea automatically.
- **The Ley Line toggle** — Vertical capsule button at the top-right of the pill's right-click menu that toggles Structured Mode on/off. Off state is a latent amber rune; on state is a quietly-lit violet conduit.
- **Voice-command gate ("Voxify")** — Right-click the Ley Line to open a Voice Command popup. When enabled, Structured Mode only runs if you end your dictation with the trigger word "Voxify" (or any of seven phonetic aliases — foxify, boxify, poxify, woxify, vexify, vaxify, oxify — so Whisper misreads still trigger). Otherwise the transcript stays plain.
- **LLM model manager** — Settings → LLM Models page for downloading Qwen3 Structured Mode models from HuggingFace with SHA-256-verified streaming and per-model on-disk tracking.
- **Fabrication defenses** — Raw-input grounding, files-must-appear-in-dictation check, short-input content-word guard, cross-slot dedupe, third-person-to-first-person rewrite, and strict no-padding rules in the prompt keep the LLM from inventing features, filenames, or constraints.
- **Context-mode menu stays open when switching** — Clicking between context modes in the pill menu now keeps the menu open; it only closes on click-outside or Esc.

### Improvements

- **Premium pill-overlay redesign** — Warm charcoal surfaces, atmospheric bloom + rim light + grain overlays, Archivo display typography for kickers, refined hover/active states across the ModeSelector, quick-toggle circles, and ship-send popup.
- **Unified pill-to-panel transition** — The structured panel sits flush above the pill with 4 px gap (matching the right-click menu spacing) and reveals via a clip-path morph from capsule → rounded rectangle, reading as the panel flowing out of the pill.
- **Overlay resize race fixed** — Right-clicking the pill occasionally expanded it horizontally without showing the menu. Consolidated the two competing resize effects into one that calls resizeOverlay exactly once per transition.
- **"Expected Behavior" slot** — Replaces the old Follow-up list with outcome-framed acceptance criteria ("I should be able to X", "X should always Y") better suited to coding-agent prompts.
- **Google Fonts loaded in the overlay** — Archivo / Outfit / IBM Plex Mono now render in the overlay window; previously fell back to system-ui.
- **CSP widened for data: URIs and Google Fonts** — Allows the panel grain textures and the font set without triggering browser policy errors.

### Bug Fixes

- **Trailing "voxify" no longer strips from plain output** — The trigger word is only removed when the voice-command gate is armed; without the gate it's treated as ordinary content.
- **Structured panel no longer clips when Raw Transcript opens** — Body max-height shrinks to keep total panel height inside the 480 px overlay window; full content still scrolls.
- **About panel shows the correct version** — Was hardcoded to v0.1.7, now matches release metadata.
- **Bundle identifier no longer ends with `.app`** — Changed from `com.omnivox.app` to `com.omnivox.desktop` to avoid macOS bundle-extension collision.

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
