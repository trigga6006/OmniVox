# Changelog

## v0.6.0

### Features

- **Meeting Mode.** OmniVox can now sit in on your calls. Start a meeting from the new **Meetings** page (sidebar → Capture), and OmniVox captures both your microphone and Windows system audio for Google Meet, Zoom, Teams, Skype, Discord, or Webex. Audio is written as recoverable 20-second chunks and transcribed locally by Whisper; each chunk is deleted the moment its text lands. Deferred transcription keeps Whisper idle during the call so the meeting itself stays smooth, or switch to live transcription when you want text as it happens. While recording, a small **widget docks to the right edge of your screen** — a dithered orb that doubles as the recording indicator and the button to open your notes, with pause/resume/stop sliding up on hover. Notes live in a **floating drawer** you can open over any app (or in the full Meetings page): rough notes, searchable transcript, markers, speaker labels, tags, and favorites, plus reusable meeting setups for title, agenda, and participants. Closing the drawer hides it rather than destroying it, so reopening is instant.
- **OmniVox notices when you're in a call.** A background check every 25 seconds watches which apps hold the microphone — the signal that catches a backgrounded browser tab that never puts "meeting" in its window title — and corroborates it against open windows. When it's confident, a small **"Meeting detected"** card slides into the top-right corner offering **Start recording** or **Not now**. Every rule requires two pieces of evidence, so a web app that merely holds microphone permission for dictation never triggers it, and Discord (which holds the mic whenever it runs) additionally requires you to be looking at it. Dismissing the card means no for that call — it will not resurface until the call actually ends — and a card you simply ignore reminds you again after ten minutes. Nothing records until you say so.
- **Optional AI meeting notes, through your own OpenRouter account.** Bring your own OpenRouter API key and OmniVox will turn a transcript into structured summaries, decisions, action items, and grounded follow-up questions — with timestamp-linked evidence back to the transcript, bounded local notes-version history with safe restore, and a cross-meeting follow-up inbox that is searchable, owner-filterable, due-aware, and can carry recurring-meeting items forward as editable tasks. Before every paid request OmniVox checks model availability, structured-output support, context capacity, live prices, your per-meeting limit, and a monthly guardrail; the in-app catalog does duration-aware cost planning and can rank models by quality-per-dollar, and provider routing can prefer lowest cost, fastest output, or fastest first token. Exports come in two clearly separated shapes: a polished note for sharing (title, date, participants, agenda, edited notes) and an explicitly labeled full archive (raw transcript, markers, provider, cost) — plus WebVTT/SRT captions. **This is entirely opt-in and inactive until you enter a key.** Audio is never uploaded; your key is stored in Windows Credential Manager, not in the app database.
- **Parakeet TDT — a second speech engine.** NVIDIA's Parakeet TDT 0.6B V2 (int8, 631 MB) now sits in the **Models → Speech Recognition** list alongside the Whisper models, marked with a violet **"Parakeet · CPU"** badge. Picking it is the same Download → Activate you already use; the whole engine swaps behind the scenes. It punctuates and capitalizes natively, doesn't hallucinate on silence, and on the public Open ASR leaderboard it makes roughly a quarter fewer word errors than Large V3 Turbo (6.05% vs 7.83% average WER). Two things to know before you switch: it is **English only and CPU only** (the GPU toggle doesn't apply to it), and **your vocabulary bias and Screen Context do not condition it** the way they condition Whisper — those are Whisper prompt features. Dictionary replacement and phonetic correction still apply to its output. Whisper remains the default and the recommended pick for every hardware tier.
- **AI Transcript Cleanup.** A new **Cleanup** tab on the Models page runs your dictation through a small local model that rewrites it as clean written text: fillers dropped, spoken self-corrections resolved ("send it Friday — I mean Saturday" → Saturday), punctuation and casing applied, and numbers, dates, and email addresses written out properly. It runs *before* everything downstream, so Structured Mode, list formatting, your output, and your history all see the cleaned text — and it composes with Structured Mode rather than replacing it. Powered by **S1-mini by Superwhisper** (462 MB, Q4_K_M), which downloads and activates in a single click the first time you turn the toggle on. **Off by default. English only. Runs entirely on CPU, on your device** — deliberately, so it doesn't stack another 1.1 GB onto the VRAM Whisper and Structured Mode already share. Transcripts over 3,000 characters skip cleanup rather than being chunked, and any failure falls back silently to your original text. There's a **Test cleanup** panel on the same tab if you want to see what it does before committing.
- **Distil Large V3.5.** The distil-whisper team's newest release joins the speech-model catalog — the same 5x-speed profile as Distil Large V3, with improved training. Distil Large V3 remains the auto-recommended pick for High-tier hardware.
- **Choose how long history is kept.** Settings → Privacy gains a **"Keep history for"** picker: *Until I delete it*, *7 days*, *30 days*, or *90 days*. Older entries are removed in the background when you change it. New installs default to 30 days; existing installs keep whatever policy they already had. Audio is never stored.

### Improvements

- **The interface, rebuilt.** This is the largest visual pass since the app shipped. Ten new shared primitives replace the ad-hoc controls that had accumulated — the three native checkboxes that rendered Windows' own control and ignored every theme token, the two native `<select>` dropdowns that did the same, five separate progress-bar implementations, and `window.confirm()` dialogs whose only button said "OK" and told you nothing about what was about to happen. Pages now load with **skeletons shaped like their real content** instead of blocking behind one centered spinner and popping in whole — and the skeleton rows are sized to the rows they stand in for, so lists no longer jump as they fill. The **dictation home is optically centered**, with the record button holding one y-position whether or not a result card is on screen, and a new **bottom dock** that reads out your word count, your milestone progress as a hairline, and a live status line for your active model, microphone, and writing style. The hotkey prompt is now real keycaps that breathe while idle. The **sidebar is grouped** into Capture / Library / System instead of nine flat items, with the Commands icon swapped off the macOS ⌘ glyph. Motion was retuned across the board — entrances went from 400–550 ms down to 200–240 ms, because entrances should answer, not perform — and reduced-motion now suppresses staggered delays too, so the page arrives at once instead of crawling in.
- **Fonts ship with the app.** Geist and Geist Mono were being fetched from Google Fonts on every launch, from three separate windows. They're now bundled (296 KB, six weights, SIL OFL). A local-first dictation app shouldn't phone a CDN to draw its own text — and the app now renders correctly with no network at all.
- **The light theme is legible again.** The dark color ramp was being used verbatim on the light paper canvas. Command-mode blue badges were rendering at roughly **1.7:1** contrast — effectively invisible. Green "success" badges sat at 3.16:1, under the accessible floor. The recording red, the command blue, eight semantic role colors, and the success green are all remapped for light mode now, with measured contrast between 4.6:1 and 14.9:1. Separately, **every shadow in the app was silently dead in light mode**: Tailwind inlines theme shadow values into the utilities it generates, so the light-theme remap could never be read and every surface carried its near-black dark-mode shadow on paper. Fixed.
- **The overlay follows the app's palette.** The floating pill and Structured panel between them hand-typed about 180 raw `rgba()` literals; they now read the same design tokens as everything else. Every keyframe, duration, easing curve, and transform is byte-for-byte unchanged — the pill moves exactly as it did. The one visible difference: Structured Mode's Ley Line popup was a dusty mauve that belonged to no palette, and is now the app's actual violet.
- **OmniVox holds far less memory when you walk away.** The Structured Mode model's **weights** are now released after 45 minutes idle — roughly a gigabyte that previously stayed resident for the whole app session even if you never dictated again — on top of the 5-minute KV-cache release that already existed. Whisper's retained decode state (~500 MB) is released on the same 5-minute schedule instead of being held for the session. The next dictation after a long gap pays one cold load; that's the trade.
- **"8 seconds" now means 8 seconds.** With both Cleanup and Structured Mode enabled, the two stages each used to claim a full LLM timeout, so an 8-second setting could hold a dictation for 16. They now split one shared budget — cleanup takes at most 3 seconds or half the budget, whichever is smaller, and Structured Mode gets the rest. Cold model-load time isn't charged against it.
- **Model downloads are verified, and Parakeet's four files show one progress bar.** Every model artifact is now pinned to an upstream revision with an expected size and SHA-256, streamed to a temporary file, and verified before it is published under its real name — a truncated or tampered download can no longer masquerade as an installed model. Models installed by older releases are verified once on upgrade. Parakeet ships as four separate files (encoder, decoder, joiner, tokens); their progress is summed into a single monotonic bar rather than four restarts, already-verified files count as complete before their download begins, and a partial set never reports itself as installable.
- **Failures on the Models page say what happened.** A failed catalog read used to log to the developer console and leave a blank tab — indistinguishable from "you have no models." It now shows the error. Activating a model is a multi-second backend swap, and the button now owns that wait with a spinner instead of the page looking frozen.
- **Audio ducking is cheaper.** The Windows audio-device enumerator is cached per worker thread instead of paying a COM class-factory lookup on every duck and unduck. The default endpoint is still re-resolved every time, so switching output devices mid-session still ducks the device you're actually listening to.
- **The model-load diagnostic log can't grow forever.** `%AppData%\omnivox\model-load.log` was append-only; it now uses the same ~1 MiB size-capped rotating writer as the LLM diagnostic log.

### Security & hardening

- **Releases are code-signed, and the installer verifies it.** The release workflow now imports a signing certificate, signs the NSIS installer, verifies the Authenticode signature before uploading, and publishes a `SHA256SUMS.txt` alongside it. `install.ps1` refuses to execute anything it hasn't verified: it pins the exact expected asset names, rejects any download URL that isn't a github.com release download, checks the downloaded size against the release metadata, verifies the SHA-256 against the published manifest, verifies the Authenticode signature, prints the publisher, and cleans up its temp directory on every path.
- **Each window now gets only the permissions it needs.** The single `default` capability that granted `core:default` plus event-emit and full window control to the main window, the overlay, and the scratchpad is gone, replaced by six per-window capability manifests. The main window can read the app version and listen for events — nothing more. The overlay can only listen. Only the windows you actually drag or resize hold the dragging permissions.
- **Structured Mode output can't be redirected.** Each emitted structured result now carries a random capability bound to the capture generation and an immutable window-handle/process snapshot, with a 30-minute TTL, consumed on paste. The overlay may edit the Markdown; it can never choose or re-resolve the destination window.
- **History deletion respects the live setting.** If you turn history off while a transcription is still being processed, the finished result no longer races the purge and re-creates the content afterward. A poisoned settings lock fails private — nothing is persisted — while delivery still works.
- **CI is pinned and least-privilege.** Every GitHub Action is pinned to a commit SHA, the Rust toolchain is pinned to 1.94.0, the audit tools are pinned to exact versions, the build job runs with `contents: read`, and publishing happens in a separate job that only downloads the signed artifact. Health checks now also run `cargo fmt --check` and lint the **Vulkan** backend that actually ships, rather than a no-feature build.

### Bug Fixes

- **Dismissing the meeting suggestion now sticks.** The "Meeting detected" card identified a call by its window title, and Discord/Meet titles change with every channel or tab you click — so each click looked like a brand-new call and the banner came back past both the cooldown and your dismissal. Calls are now identified by app, and an explicit **Not now** suppresses that call until it actually ends.
- **A failed privacy purge no longer looks like a success.** If clearing local transcript history fails, a non-dismissing error toast now says so explicitly and tells you how to retry. Silently leaving the user believing their local transcripts were removed is the one failure this feature must not have.
- **You can select your own typing again.** The global `user-select: none` (there to stop the app feeling like a web page) was leaking into inputs, textareas, and editable regions.
- **The focus ring stopped squaring off round controls.** It forced a 4px radius onto everything it touched — toggles, segmented controls, icon buttons.
- **Cleanup can't accidentally send a message.** The cleanup model can introduce or move a trailing "send" you never spoke, which would press Enter in whatever app is focused. The send command is now re-derived from the pre-cleanup transcript and dropped if it wouldn't have fired.
- **"View raw" in History shows the real transcript after cleanup.** It already did for Structured Mode; it now also preserves the pre-cleanup text whenever cleanup rewrote the transcript, for the same reason — the saved text is no longer verbatim what you said.
- **Progress bars retarget instead of restarting.** A download jumping 12% → 47% now animates to the new value rather than snapping back to zero.

### Internal

- Five release-mode benchmark harnesses (`asr_bench`, `asr_optimization_ab`, `command_ab`, `command_routing_ab`, `extraction_ab`) behind `npm run bench:*`, emitting schema-versioned JSON with source-revision and dirty-state provenance, model SHA-256, requested-vs-verified backend, and p50/p95 latency. Documented in [docs/BENCHMARKING.md](docs/BENCHMARKING.md), with a baseline results record.
- Pipeline tracing (`perf.rs`) records per-generation stage timings from stop to visible delivery.
- Frontend test suite grew to 109 tests across 16 files, with a shared vitest setup for jsdom's missing layout APIs; `npm run check` / `check:quick` / `check:rust` / `check:vulkan` wrap the full local preflight.
- Dependency bumps: `reqwest` 0.12 → 0.13.4, `cpal` 0.15 → 0.17.3, plus `sha2`, `sysinfo`, and `sherpa-onnx` 1.13.5. Node engine range and npm version are now declared; `@tauri-apps/api` and `@tauri-apps/cli` are pinned to 2.10.1. Unused macOS `cocoa` and `core-foundation` dependencies removed.
- Capture state is now owned per generation (`CaptureSession`) with an explicit Starting/Live/Stopping phase machine, so a worker from an older capture cannot observe or overwrite a newer one's state.

## v0.5.0

### Features

- **Scratchpad.** A small always-on-top pad you can dictate into from anywhere. Open it from the overlay pill (hover the idle pill and click the bud) or by voice ("open the scratchpad"). Two views — Cards (each dictation becomes its own entry) and Note (one growing text block) — with a live voice-reactive mic orb, copy-one / copy-all, and "Send to Notes." Turn on the crosshair "capture" toggle and your dictation hotkey routes text *into the pad* even while you're reading in another window, so you can read in one place and collect answers in another. The pad remembers its size and position, survives close as a hidden window for instant reopen, and its content persists across restarts.
- **Command Mode grows up (Jarvis Phase A).** The confirm pill is now driven from the keyboard — Enter confirms, Esc dismisses — even though the overlay never takes focus, with guard rails so an Enter finishing your typing can't confirm and a forgotten pill stops swallowing keys after a few seconds. A submitting message ("tell Claude to…") now shows an **editable** confirm seeded with what was heard, so a mishearing is fixed before it sends (Ctrl+Enter sends, Esc cancels). Say **"stop" / "cancel" / "never mind"** to abort a command mid-run, and **"undo that"** to reverse the assistant's own last action (re-close a launched app, restore a minimized window, Ctrl+Z a non-submitted paste). Submitted messages stay non-undoable by design — the confirm gate is their protection.
- **Structured Mode profiles.** Structured Mode is no longer one fixed shape: choose a profile per context mode — agent-prompt, email, or notes-outline — each with its own extraction schema and formatting. One warmed LLM session is reconciled to the selected profile with a background re-warm on switch, so switching stays fast.
- **Dictate lists by voice.** New built-in voice commands turn speech into real lists in plain dictation mode — no LLM needed: "bullet point" / "next bullet" starts a `- ` item, "number item" / "next number" starts an auto-counting `1.` `2.` `3.` item, and "end list" closes the list (resets numbering, breaks to a new line before further prose). Items are cleaned up as you'd expect — the comma before a marker is dropped, the first letter is capitalized (except in very-casual style), and a marker you trail off on ("...bullet point" with nothing after) is discarded instead of pasted dangling. The whole list lands in the target app as a single atomic paste instead of one paste per segment, which also makes multi-part dictations faster. All commands appear on the Commands page with individual toggles, including on existing installs.
- **Counted lists work on short dictations, and spoken ordinals become numbered lists.** "I need three things. Milk. Eggs. Bread." now formats as a list even though it's short — an explicit count is an explicit signal, so the 40-word minimum no longer applies to it. And when the items themselves start with spoken ordinals ("these three steps: First, ... Second, ..."), the list is rendered as `1.` `2.` `3.` with the redundant ordinal words stripped, instead of dash bullets. Ordinal sentences *without* a counted header still stay prose — that guard against surprise bullets is unchanged.
- **Filler removal is now a setting.** Settings → Writing style gains a "Filler removal" toggle. On (the default) keeps today's behavior — "um", "you know", stutter repeats, and stray "basically" are dropped. Off transcribes verbatim, for users dictating quotes or who want every word kept.

### Improvements

- **Refreshed interface.** A visual pass across the whole app — pages, settings, and the overlay pill — onto a consistent dark graphite surface with a single amber accent. Tighter typography and spacing, unified primitives (page headers, spinners, fields), and a cleaner overlay.
- **Dictation into OmniVox's own windows now stops the instant you release the hotkey.** Holding the hotkey to dictate into the scratchpad (or another OmniVox window) could keep recording after you let go, because the key-release event is unreliable while our own window is focused — it looked like an unwanted long-running session. A release watchdog now detects the physical key-up and stops, transcribes, and pastes as expected. Long-running dictation is still available on purpose — via the pad's mic button or a double-tap of the hotkey.
- **The overlay pill no longer disappears over a long session.** Its always-on-top was set once and never re-asserted, so Windows could demote it (fullscreen apps, UAC, explorer restart) or a monitor sleep/unplug could park it off-screen. A watchdog now re-asserts topmost and pulls it back onto a connected monitor automatically, instead of needing a manual "Reset Pill."
- **Long dictations no longer silently lose content in Structured Mode.** The LLM input cap rises from 1,600 to 4,000 characters (the context window grows to match), and when a dictation still exceeds it the panel now shows a warning with the number of characters that weren't structured — the Raw view always carries the full transcript. Previously everything past the cap was dropped with no indication.
- **Structured Mode is ready when you are.** The LLM's warmed prompt cache is released after 5 idle minutes to save memory; previously the next dictation paid the multi-second re-warm on its critical path. The session is now rebuilt in the background the moment you start recording, so the prefill overlaps with your speech. Toggling Structured Mode on also no longer freezes the Settings page while the model loads — the load happens in the background.
- **Deleting a context mode with vocabulary words works now.** The delete cascade was missing vocabulary entries, and since they hold an enforced foreign key, deleting any mode that had mode-scoped vocabulary failed outright with a database error. Vocabulary now deletes with the mode (dictionary and snippets already did), and a startup repair purges any orphaned rows left by databases that predate foreign-key enforcement.

### Security & hardening

- **Command execution is now identity-bound and cancellable.** A two-batch safety refactor from a multi-agent audit: every side-effecting action (paste, Enter, keyboard/media/window control, app launch) is tied to an immutable per-command target (window handle + owning process) and re-verifies that window is still foreground and the same process before firing — a recycled or refocused window fails closed instead of acting on the wrong app. A monotonic cancellation floor is re-checked inside every blocking step (including between paste and Enter), the confirm gate is bound to the exact pending command id, and app launches prove the launched app's identity before allowing focus-dependent follow-ups.
- **Voice-command URLs are grounded.** Opening a site now requires the host to have been named in what you actually said (validated with a public-suffix parser, userinfo/non-web URLs rejected); an ungrounded URL parks on the confirm pill instead of opening blind.
- **Untrusted text is sanitized before it reaches the LLM**, and two crash/corruption bugs were fixed — a UTF-8 slice panic in the list formatter, and a lowercase/index desync that could corrupt post-processed text.
- **Sending no longer refuses when a Store/AppsFolder app can't be process-proven.** Some packaged apps can't have their owning PID verified through the launch path; the send path now falls back to the verified foreground window instead of refusing outright.

### Internal

- Frontend CI: a new `frontend-health` workflow runs `tsc` + `vite build` on every PR — TypeScript/Rust payload drift previously shipped unchecked (the `context` slot was missing from the TS `SlotExtraction` mirror; fixed).
- `LlmConfig` sizing (context window, output budget, threads) now has a single source of truth in `LlmConfig::default()` instead of being duplicated in the loader.
- Removed the never-read `auto_punctuate` processor flag; the overlay pill's noise-reduction quick-toggle no longer defaults to the wrong state before settings load.
- Deleted the dead list-detection scaffolding in the formatter (patterns that were hard-disabled behind `false` gates); the doc comment now describes what actually runs.
- Fixed a Linux build error in `audio/ducking.rs` (`DEFAULT_DUCK_FACTOR` referenced outside its cfg), so the Rust suite can build and run on non-Windows dev machines.

## v0.4.0

### Features

- **Voice Command Mode.** Speak a command and OmniVox carries it out instead of typing it: launch or switch to an app ("open Spotify"), run a web search or open a site, and fire common keyboard/media/window shortcuts (copy, paste, save, play/pause, minimize, show desktop, and more). A single utterance can chain steps ("open Spotify and play it"), and low-confidence or consequential actions (a fuzzy app match, closing a window) are held behind a confirm pill you accept with Enter or dismiss with Esc.
- **Type and send messages by voice.** Two new commands let you put text into whatever app is focused: "type ..." pastes the text and leaves it for you to review, while "tell Claude to ...", "send a message to Slack saying ...", or "ask ..." pastes the message and presses Enter to send it. Because sending presses Enter in another app, every send is routed through the confirm pill first so a message is never fired blind. Text goes in through the same clipboard-verified paste path as dictation, and your prior clipboard is restored afterward.
- **Voice-command library page.** A new Commands page lists every supported command with per-command toggles, so you can enable or disable individual actions and see at a glance what Command Mode understands.

## v0.3.1

### Features

- **Vocabulary words now actually correct mis-transcriptions.** Adding a word to your vocabulary previously only *biased* Whisper toward it — probabilistic, and "Claude" still regularly came out as "cloud" or "clod". A new phonetic correction pass now runs after every transcription: any word (or word pair) that sounds like one of your vocabulary entries — same consonant skeleton, small edit distance, same first letter — is replaced with the entry exactly as you wrote it, casing included. Split compounds fuse too ("omni cue" → "OmniCue"). Guard rails keep it from misfiring: short entries (under 5 letters) don't participate, sentence boundaries are respected, and near-misses that are spelled too differently ("clot") are left alone. Works with both global and mode-scoped vocabulary, takes effect immediately when you edit the list.

### Improvements

- **Structured Mode is much faster.** The local LLM now keeps its system prompt (~1,900 tokens) cached in the KV cache across dictations instead of re-processing it from scratch on every extraction. Logs from real use showed extractions taking 2–7 seconds (and sometimes hitting the 8s timeout) — the bulk of that was redundant prompt prefill. After the first warm-up (done in the background right when the model loads), each extraction only processes your actual words. The cache is self-healing on errors and is released automatically after 5 minutes of inactivity so it doesn't hold memory while idle.
- **Cleaner audio into Whisper.** Microphone capture (typically 48 kHz on Windows) was downsampled to 16 kHz with plain linear interpolation, which folds all high-frequency content — hiss, keyboard clatter, sibilant energy above 8 kHz — back into the speech band as aliasing noise. Capture now runs through a proper windowed-sinc anti-aliasing filter before decimation, so Whisper sees the speech spectrum it was trained on. This improves transcription robustness on every recording.
- **Faster stop-to-text with Live Preview on.** Stopping a recording no longer waits (up to 1.5s) for an in-flight preview transcription to finish — the preview pass is aborted immediately via whisper.cpp's abort callback, so final transcription starts right away.

### Bug Fixes

- **Long dictations no longer silently break Structured Mode.** The LLM context window (2048 tokens) was too small to hold the system prompt plus a long dictation plus the model's output — extractions on longer dictations could fail mid-generation and degrade to plain text. The context is now sized (3072) to fit the system prompt, the full 1,600-character input cap, and a complete JSON response.
- **Structured Mode works again for pre-v0.2.10 installs.** The v0.2.10 catalog rename to the official Q8_0 files orphaned previously downloaded Q4_K_M models *and* the saved model selection — every extraction since then failed with "model is not downloaded" and silently fell back to plain dictation. Old model IDs now map to the current catalog entries, and previously downloaded legacy files are recognized as installed (labeled with their actual quantization) so no re-download is needed. Deleting a model now also removes any legacy file.
- **No more invisible CPU fallback after app launch.** When the GPU load failed transiently at startup (Vulkan not ready right after login, VRAM briefly busy), Whisper silently fell back to CPU — the UI showed the correct model while transcription ran several times slower, until you re-selected the model. The loader now retries the GPU once after 1.5 s before falling back; if it still ends up on CPU, the overlay shows a clear "running on CPU (slower)" banner instead of staying silent. Every model load (Whisper and LLM) is also recorded with its backend and duration in `%AppData%\omnivox\model-load.log`, so "the app feels slow today" can be checked against what actually loaded.

### Internal

- Analytics, history search, and history export now run on blocking threads so a large transcription history can't stall the UI/dictation runtime.
- Context-mode cascade deletes are wrapped in a transaction; per-mode dictionary/snippet/vocabulary lookups are indexed.
- Removed dead frontend code (`useAudioLevel` hook, `contextModeStore`).

## v0.3.0

### Improvements

- **Settings, redesigned.** The Settings page is reorganized into grouped cards with clean, scannable rows (label + description + control) instead of one oversized card per option. The most important controls now lead: the push-to-talk **Shortcut** and **Performance (GPU acceleration)** sit at the top instead of the bottom. Everything is laid out in a width-filling two-column layout, roughly halving the scroll. All controls are preserved — segmented toggles, input-device picker, ducking slider, screen-context sub-toggle, and the voice-commands reference.
- **Steady record button.** On the dictation screen, the record button no longer shifts upward when you start recording or jumps during transcription. The mic, stop, and processing icons now share one fixed center point, and the processing ring is properly centered.
- **Sidebar order.** Analytics moved to the end of the main tabs, and Settings is pinned to the bottom, visually separated from the primary navigation.
- **Tighter page headers.** Reduced the dead space around page titles across History, Dictionary, Modes, Notes, Models, and Analytics.

### Internal

- Applied a `cargo fmt` formatting pass across the Rust backend (no behavior changes).

## v0.2.11

### Features

- **Analytics page.** A dedicated Analytics view (new sidebar entry) turns your dictation history into stats. A single overview card splits into _Output_ — lifetime words, characters, estimated tokens, dictations, sessions (grouped by 30-minute activity gaps), and total time recorded — and _Consistency_ — current and longest day streaks, active days, average speaking pace (words per minute), words per active day, and your most-used model. Below it: a GitHub-style contribution heatmap of daily activity across the year, a peak-hours histogram, a models-used breakdown, and a 30-day words-per-day trend, all with hover tooltips. Everything is computed locally from existing transcription history through a new lightweight backend query that keeps full transcript text in the backend — only per-record counts cross the IPC boundary.

### Bug Fixes

- **Amber accents are legible in light mode.** The brand amber scale was tuned for dark surfaces, so amber text and accents (active nav, labels, highlights, streak figures, heatmap, charts) washed out to near-invisible on the light "warm paper" theme. The light theme now remaps the lighter amber shades to a darker, saturated honey-gold, fixing contrast across the whole app at once — not just the Analytics page. Dark mode is unchanged, and the always-dark floating pill overlay is unaffected.

## v0.2.10

### Bug Fixes

- **Structured Mode model downloads work again.** The LLM catalog pointed at Qwen `Q4_K_M` GGUF filenames that do not exist in the official HuggingFace repos, so clicking Download failed immediately and the UI snapped back to its idle state. The catalog now uses the official `Q8_0` files, with corrected sizes and quantization labels, and includes a regression test for those filenames.
- **LLM download failures now stay visible.** If a Structured Mode model download fails again, the model row keeps an inline error message instead of silently resetting after logging to the developer console.

## v0.2.9

### Bug Fixes

- **Codex/browser-use freeze guard.** Screen Context now skips unsafe agent and browser-host processes (`codex.exe`, `claude.exe`, and `msedgewebview2.exe`) before touching Windows UI Automation. These apps can become unresponsive when OmniVox walks their accessibility tree while browser automation is active, which made dictated messages into Codex threads freeze the entire Codex app. Dictation still works normally; OmniVox just avoids cross-process UIA capture for those targets.
- **Screen Context fail-safe default for the local install.** For the hotfix install path, both Screen Context toggles are disabled in the user settings DB so even older binaries avoid the risky UIA path until the packaged `v0.2.9` build is installed.

## v0.2.8

### Bug Fixes

- **Recover-from-lost-pill escape hatch in the tray.** The floating pill could vanish for several reasons — ghost mode accidentally toggled on (pill is opacity-0 with a 56×26 px invisible click target, effectively unrecoverable by clicking blindly), a connected monitor unplugged with the pill parked on it, a fullscreen app stealing the always-on-top z-order, or a long-running WebView2 process going stale. None of these had a recovery path: the tray only had "Show OmniVox" (which shows the main window, not the pill) and "Quit". The tray now has a **Reset Pill** item that force-disables ghost mode in the DB, broadcasts the settings change so the overlay un-ghosts, repositions the pill to the primary monitor's center-bottom via `SetWindowPos`, and re-asserts `show` + `always_on_top` so it climbs back out of any z-order pit. Survives every reason the pill might "disappear" except an outright destroyed window (rare WebView2 process death — that still needs a full app restart).

## v0.2.7

### Bug Fixes

- **Type output mode now preserves your pre-copied clipboard.** Every output mode (Clipboard, Type, Both) was silently writing the dictation to your clipboard and leaving it there, so a snippet you copied before dictating would get clobbered the moment the transcription landed. Type now captures the clipboard before pasting, waits out the 250 ms deferred-read guard that protects against apps which read the clipboard *after* Ctrl+V returns, then restores whatever you had copied. Both still keeps the dictation on the clipboard by design (that's the "I want a copy too" mode). Clipboard is unchanged. The three modes now have genuinely distinct semantics — previously Type and Both were the same code path.
- **Mode descriptions added to Settings → Output.** A one-liner under the toggle group spells out what each mode does, so the difference between Type and Both is obvious without reading the source.

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
