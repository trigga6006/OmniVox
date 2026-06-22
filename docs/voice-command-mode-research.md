# Voice Command ("Jarvis") Mode — Research & Design Memo

**Status:** Research / proposal (not yet scheduled)
**Date:** 2026-06-20
**Author:** Investigation for the OmniVox solo dev
**TL;DR:** NVIDIA Parakeet is real, permissively licensed (CC-BY-4.0), and genuinely
runnable in this exact Rust/Tauri/Windows stack — but it is the *easy 20%*. The hard
80% is the assistant/command layer (trigger → intent → action → safety), and OmniVox
already has skeletal versions of nearly every piece. **Recommendation: build "Command
Mode" on the existing Whisper engine first; treat Parakeet as a later, decoupled ASR
swap, not a prerequisite. Do not let "switch the ASR" block "build the assistant."**

Every architectural claim below is grounded in the actual codebase; every research
claim was independently fact-checked (corrections applied inline).

---

## Part 1 — NVIDIA Parakeet: verified findings

### 1.1 The model family

| Model | Size | Languages | License | Avg WER (Open ASR LB) | Notes |
|---|---|---|---|---|---|
| **parakeet-tdt-0.6b-v2** | 600M | English | **CC-BY-4.0** | **6.05%** | Punctuation, caps, word/segment timestamps. Recommended for English. |
| **parakeet-tdt-0.6b-v3** | 600M | 25 European | **CC-BY-4.0** | 6.34% (en) | Auto language detect. Recommended for multilingual. |
| parakeet-tdt-1.1b (+ rnnt/ctc) | 1.1B | English | CC-BY-4.0 | 7.02% | **Avoid** — bigger, slower, *no* punctuation/timestamps, no accuracy win. Legacy. |
| parakeet-unified-en-0.6b, nemotron-speech-streaming, parakeet_realtime_eou_120m | 0.12–0.6B | English | **NVIDIA Open Model License (varies!)** | — | True low-latency *streaming* (down to 160 ms). License must be checked **per model** — several are NOT CC-BY-4.0. |
| Canary-1b-v2 / Canary-Qwen-2.5b | 1B+ | Multilingual + translation | CC-BY-4.0 | — | Sibling family (attention decoder). Heavier/slower. Not for low-latency commands. |

- **Architecture:** FastConformer encoder + **TDT** (Token-and-Duration Transducer) decoder.
  Because it's a transducer, **it stays silent during silence** — a pause becomes a pause
  in the transcript. Whisper's encoder-decoder can *hallucinate* text on leading/trailing
  silence (mitigated, never zeroed, by VAD). For short, pause-bounded **commands**, this is
  the single most valuable property.
- **License is clean:** v2 and v3 are CC-BY-4.0 in both the YAML header *and* the model
  card "GOVERNING TERMS" body — commercial use allowed, attribution required (add one line
  to an about/licenses screen). Verified against the raw `README.md` on Hugging Face.

Sources: `huggingface.co/nvidia/parakeet-tdt-0.6b-v2`, `…-v3`, `…-1.1b`;
`huggingface.co/nvidia/parakeet-unified-en-0.6b`; Open ASR Leaderboard;
NeMo `docs/source/asr/models.rst`; arXiv 2509.14128.

### 1.2 Running it locally on Windows from Rust — **proven, not theoretical**

- **Direct precedent on our exact stack:** [Handy](https://github.com/cjpais/Handy)
  (cjpais, ~24k stars) is a **Tauri 2 + Rust** dictation app running Parakeet **fully
  offline on Windows** via the `transcribe-rs` crate + `ort` (ONNX Runtime), **no Python**.
  Same shape as OmniVox.
- **CPU-only is viable:** int8 ONNX runs **~17–30× real-time** on consumer x86 (i7-4790 →
  i7-12700K), **~2 GB RAM**, **~630 MB on disk** (v2 int8: encoder 622 MB + decoder ~9 MB).
  *Caveat: these speed figures are third-party self-reported on x86 desktops; they collapse
  to ~1× on weak ARM. Validate on the actual target machine.* int8 ≈ fp32 accuracy.
- **Integration options, by fit:**
  1. **`transcribe-rs`** (cjpais) — multi-engine STT via ONNX Runtime, CPU by default, no
     Python. Battle-tested in Handy. **Lowest friction.**
  2. **`parakeet-rs`** (altunenes, MIT/Apache) — pure-Rust via `ort`; CUDA/DirectML/WebGPU
     EPs; supports streaming/EOU + diarization. *Windows works (ort is cross-platform) but
     CI is Linux-only — spike it.*
  3. **`sherpa-onnx`** (k2-fsa) — mature C++ runtime, official Parakeet-v2-int8 support,
     Windows + C-API. Heavier C-FFI build; most portable fallback.
  4. **`parakeet.cpp`** (mudler) — C++17/**ggml** port, **prebuilt Windows x64 + Vulkan**,
     GGUF quant, **flat C-API for FFI**. Interesting because it mirrors our existing
     `llama.cpp` + Vulkan pattern. No native Rust crate yet (standard FFI works).
  - **Avoid:** NVIDIA NeMo (Python + heavy CUDA — disqualified for embedding);
    `parakeet-mlx` (Apple-Silicon only).

> **⚠️ Correction applied:** the research initially assumed OmniVox already uses `ort`.
> It does **not** — `src-tauri/Cargo.toml` has `whisper-rs` + `llama-cpp-2` only. Adding
> Parakeet means **introducing a new ONNX Runtime native dependency** and its Windows
> DLL-bundling story. Well-precedented, but a real (bounded) integration cost — not a drop-in.

Sources: `github.com/cjpais/Handy`, `…/transcribe-rs`; `github.com/altunenes/parakeet-rs`;
`k2-fsa.github.io/sherpa`; `github.com/mudler/parakeet.cpp`; OmniVox `src-tauri/Cargo.toml`.

### 1.3 Parakeet vs Whisper — which, for what

- **Accuracy:** at the **Whisper-small** tier (the realistic CPU tier for fast commands),
  Parakeet wins **both** accuracy (~6% vs ~8% avg WER) **and** CPU speed. Only at
  Whisper-large-v3 does accuracy tie (~6–7.4%) — but large-v3 is too slow for CPU-only
  real-time commands, so it's not the relevant comparison.
- **Speed:** Parakeet is several × faster than Whisper-small on CPU (TDT advances multiple
  encoder frames per decode step). *Headline "RTFx 3380" is GPU batch-128 — not a
  single-utterance CPU latency number.*
- **Decisive tradeoff — language coverage:** Parakeet v2 = English only, v3 = 25 European
  languages. Whisper = ~99 languages, MIT, deeper noisy/accented fine-tune ecosystem.
- **Verdict:** a **two-model end state** is ideal — Whisper for long-form/multilingual
  dictation, Parakeet for fast English commands (and its no-hallucination-on-silence
  property). If OmniVox is English-only, Parakeet can plausibly serve both.

> **One unvalidated risk:** all published Parakeet benchmarks are long-form/leaderboard.
> Short, command-length-utterance behaviour is **not** benchmarked. Run a quick local A/B
> (Parakeet vs Whisper) on a few dozen real command phrases before committing.

---

## Part 2 — Local voice control: verified findings

### 2.1 The universal architectural lesson: **two tiers**

Across Talon, Rhasspy, and Home Assistant Assist, the production-proven pattern is:
**fast deterministic command grammar first, LLM/NLU fallback only for the long tail.**
Do *not* route every utterance to the LLM — most commands are fixed phrasing
("new line", "open Spotify", "send"); match those in microseconds and fall through to the
LLM only on a miss.

- **Rhasspy** `fsticuffs` (FST) recognizes over *millions* of trained sentences in
  milliseconds, CPU-only — but only trained sentences (fast/inflexible = tier 1).
- **Home Assistant Assist** is the canonical two-tier reference: "Assist handles commands
  first; only what it can't understand goes to the AI" (`prefer_local_intents`).
- **Talon Voice** is the gold standard for OS/app control (per-app context grammars,
  captures/lists, command/dictation/sleep modes, non-speech "pop/hiss" triggers) — but it's
  **grammar-only** (no LLM tier). *Correction: its Conformer engine is **free**, not
  paywalled; it's the **closed/non-redistributable runtime** that you can't reuse — which is
  the real reason to own your own layer.*
- **Serenade** (voice coding) is **Apache-2.0** (client + engine + models, community fork
  Jan 2025) — *forkable*, not merely a pattern reference. *(Correction: research called it
  "abandoned / patterns only" — overstated.)*

OmniVox already has both halves: the registry concept lives in `voice_commands.rs`, the
constrained LLM lives in `llm/`.

### 2.2 Trigger model — RESOLVED (2026-06-20)

**Momentary push-to-talk on a dedicated command key. No wake word, no trigger word, no sticky mode.**
- **Dictation = Ctrl+Alt (left hand); Command = Right Ctrl (right hand)** — hold to talk,
  release to execute. Clean "left types, right commands" model.
- **Why not bare Ctrl/Alt or CapsLock:** bare left modifiers are shortcut prefixes — the hook
  fires + *swallows* on their key-down ([hotkey.rs:182](../src-tauri/src/hotkey.rs)), breaking
  every Ctrl/Alt shortcut. CapsLock toggles on key-**down**, so preserving normal caps use needs
  swallow-and-re-synthesize with a ~150 ms delay + a statistical tap/hold split. Right Ctrl has
  neither problem (you almost never *initiate* shortcuts with the right Ctrl) and leaves CapsLock
  fully untouched.
- **Momentary (held), not sticky toggle** — command mode exists only while held, so there is
  never a "which mode am I in" error.
- **Visibly distinct command pill** (different colour/icon) so you always see "this will execute,
  not type."
- **Confirmation scales with risk:** launch/focus → execute + undo toast; consequential
  (Enter/"send", later element-clicks) → quick confirm; a global "stop" always aborts.

Wake word ("Jarvis…") stays a *later, optional* layer feeding the **identical** pipeline (Apple
"press-or-say" model) — deferred because it needs an always-on capture loop `audio/capture.rs`
doesn't have today. Original push-to-talk rationale retained below for reference:

- **Push-to-talk** (a global hotkey, reusing `hotkey.rs`) = zero false-accepts, zero
  always-on privacy cost, matches the existing dictation UX. **The MVP.**
- **Wake word** ("Jarvis…") is a later phase and needs an *always-on capture loop*, which
  `audio/capture.rs` does **not** have today (it's strictly batch push-to-talk; `start()`
  clears the buffer). Engine choice:
  - **Use** [`livekit-wakeword`](https://github.com/livekit/livekit-wakeword) — Apache-2.0
    **including trained models**, native Rust crate, ~100× fewer false positives than
    openWakeWord (self-reported, brand-new ~Apr 2026 → expect API churn).
  - **Avoid** Picovoice Porcupine — Rust SDK deprecated 2025-07-15, free tier ends
    2026-06-30, then opaque enterprise pricing.
  - **Avoid** openWakeWord's *bundled* models — code is Apache-2.0 but pretrained models are
    **CC-BY-NC** (non-commercial). Self-trained oww models are the escape hatch.
- **VAD** for endpointing/always-on: **Silero VAD** (MIT, ~2 MB, <1 ms/chunk via `ort`).
  Rust crates exist (`silero-vad-rs`, etc.) but are early-stage (0.1.x).
- **Latency:** humans expect ~300–500 ms turn-taking; VAD/wake add only tens of ms — the
  existing **Whisper + Qwen** inference dominates. Endpointing aggressiveness is the main
  perceived-snappiness lever.

### 2.3 Windows OS automation from Rust

Well-supported with permissive crates:
- **Launch apps:** `ShellExecuteW` (via the `windows` crate, already a transitive dep) +
  `shell:AppsFolder\<AUMID>` for Store/UWP apps; resolve names via App Paths registry +
  Start-Menu `.lnk` scan, fuzzy-matched with the existing `phonetic.rs`.
- **Window control:** `SetForegroundWindow`/`ShowWindow`.
- **Keys/mouse:** `enigo` (already in use).
- **Click named controls:** UI Automation — the `uiautomation` crate (Apache-2.0), or fork
  the existing `screen_context/windows.rs` UIA walk to retain element handles + use
  `InvokePattern`.

**Hard constraints to design around (not fight):**
- **UIPI:** a normal-integrity app cannot send input to/click **elevated** (admin) windows.
  Fine for Spotify/Chrome/Office/Notepad. Don't engineer around it; document it.
- **`SetForegroundWindow` is focus-/rate-limited** — programmatic focus-stealing is
  unreliable. Prefer acting *from* foreground and **UIA `Invoke` (no focus needed)** over
  "focus then type."
- **DPI:** coordinate clicks are fragile on multi-monitor/HiDPI. Prefer UIA element actions;
  reserve coordinate clicks for elements with no accessibility provider.

### 2.4 Speech → intent with the local LLM

- **Reuse the existing Qwen + GBNF stack** (`llm/grammar.rs` already compiles in
  `slot_extraction_v1.gbnf` and `engine.rs` applies `LlamaSampler::grammar`). Add a second
  grammar, `command_intent_v1.gbnf`, enumerating a **closed action enum**
  (`open_app | focus_window | type_text | press_key | new_line | send | no_op`) + a bounded
  target string.
- **Three non-negotiables:**
  1. The grammar guarantees *structural* validity but does **not** inject the schema into the
     prompt — you must *also* describe the action enum in a command-router system prompt
     (`llm/prompt.rs`). Grammar constrains output; it doesn't teach the model.
  2. **Closed enum, never free-text** — the model physically cannot emit a verb you didn't
     enumerate (key safety property).
  3. **KV-cache:** switching between the dictation prompt and a command prompt per utterance
     thrashes the warmed prefix cache. Keep command mode on a separate short session, or
     accept cold prefill on mode switches.
- Qwen3-1.7B (already the model family, Apache-2.0) is strong for intent *judgment* — the
  restraint to return `no_op` when something is dictation, not a command.

---

## Part 3 — Proposed architecture: "Command Mode"

Reuses existing primitives. Everything below names a real seam in the codebase.

### 3.1 Speech-to-intent — hybrid

```
transcript ──▶ ProcessorChain::process (clean text, phonetic vocab)
           ──▶ CommandRegistry::match  ──hit──▶ Intent ──▶ execute
                       │ miss
                       ▼
            Qwen (llm/runner classify) + command_intent_v1.gbnf
                       │
                  CommandIntent ──▶ execute   (or no_op → fall back to dictation)
```

- **Fast path:** registry of `{ phrase_pattern, Intent }`. Zero-arg verbs reuse the existing
  `const COMMANDS` mechanism in `voice_commands.rs`. Arg-carrying verbs ("open `<X>`") need a
  **new prefix-match branch** in `parse_commands_inner` that captures the remainder as a
  slot. Reuse `phonetic.rs::sounds_like` so "open spot if I" still resolves to Spotify.
- **LLM fallback:** generalize `llm/engine.rs::generate` (currently hardcodes
  `SLOT_EXTRACTION_V1`) to accept grammar+root params; add `runner.rs::classify_with_timeout`
  cloning `extract_with_context_and_timeout`.

### 3.2 Action registry + execution

Add a typed action layer rather than overloading `VoiceCommand` (keystroke-only today):
add an `OutputSegment::Action(Intent)` variant (or parallel enum) + a **new executor**
(`output/os_actions.rs` or `actions/mod.rs`):
- Keystroke verbs → existing `output/router.rs` arms (free).
- `open_app` → new `focus.rs::launch_process` (`ShellExecuteW` + AUMID resolver).
- `focus_window` → `FindWindow`/`SetForegroundWindow` (accept flakiness).
- `click <element>` (later) → fork `screen_context/windows.rs::capture_inner` into
  `capture_actionable` retaining element handles + `InvokePattern.Invoke()`.

### 3.3 Confirmation / undo UX

Clone `overlay/StructuredPanel.tsx` → `CommandPanel` (it already has preview + Paste/Cancel/
ESC + cross-window event plumbing). Backend emits `command-detected {action,args,needs_confirm}`
(mirror `structured-output-ready`); add a `command` variant to `recordingStore.RecordingStatus`
+ a pill state in `FloatingPill.tsx`. **Confirmation scales with criticality:** keystroke
verbs auto-execute; `open_app`/`focus` execute with an undo toast; destructive actions block
on explicit confirm.

### 3.4 Data-flow walkthroughs

**"Open Spotify"** (push-to-talk command hotkey held):
1. `hotkey.rs` parallel combo → `pipeline::start_command_capture` → `audio/capture.rs::start`.
2. Release → stop → `WhisperEngine::transcribe` → `"open spotify"`.
3. `ProcessorChain::process` cleans it; `CommandRegistry::match` prefix-matches `open <…>` →
   `Intent::OpenApp("spotify")`. **No LLM call.**
4. Resolver: phonetic-match "spotify" against Start-Menu/AppsFolder index → AUMID →
   `focus::launch_process` → `ShellExecuteW`.
5. Emit `command-executed`; overlay shows "Opened Spotify" with undo.

**"Send this email"** (ambiguous — Enter? click Send button?):
- *Today:* trailing "send" is already special-cased → `VoiceCommand::Send` → Enter (works in
  Gmail/Slack send-on-Enter fields).
- *Jarvis version:* registry misses → Qwen + `command_intent_v1.gbnf` → likely
  `Intent::ClickElement("Send")`. Because sending an email is consequential, route through the
  **confirmation panel**: "Click 'Send' in `<app>`?" → confirm → UIA `InvokePattern` on the
  Send button (no focus-steal, DPI-proof), restoring focus via `focus.rs` first.

---

## Part 4 — Phased delivery

### Phase 0 — MVP, ships in days (Whisper only, **no new ML deps**)
Goal: spoken "open `<app>`" launches the app; existing keystroke commands keep working; all
behind one new hotkey + settings toggle.

**Resolved spec (2026-06-20):**
- **Command set:** `open`/`launch <app>` (focus-if-running else launch), `switch to`/`focus <app>`;
  key chords (copy/paste/cut, undo/redo, select all, save, new tab/close tab, screenshot); media
  (play-pause, next/prev track, mute); window (minimize/maximize). All reversible → auto-execute +
  undo toast. **Deferred:** close app/window, arbitrary type/press, lock/shutdown.
- **`CommandIntent` enum (closed):** `OpenApp(String) | FocusApp(String) | KeyChord(..) | Media(..) | Window(..)`.
  Verbs fixed in code; only the `<app>` argument is dynamic — so "open anything" works with no user table.
- **Dedicated matcher:** Command Mode uses `match_command(utterance) -> Option<CommandIntent>` (exact
  zero-arg table + `<verb> <arg>` prefix table, phonetic-tolerant via `phonetic.rs`). It does **not**
  reuse `parse_commands_inner` (that stays for inline dictation formatting). Only the Intent→executor
  layer is shared/new.
- **Registry:** code-defined static table for v1; a constrained user-editable SQLite alias table
  (rows = phrase → closed-enum action + target — *data, not code*; mirrors `app_bindings`) is a later
  additive increment via `builtin_commands().chain(user_rows())`.

**App-name resolver (resolved 2026-06-20):**
- **Source = `shell:AppsFolder`** enumerated via COM (`windows` crate) at startup → cached
  `{DisplayName → AUMID}` index. Single source unioning Win32 + Store/UWP apps (what the Start
  menu "All apps" shows). Refresh on miss/timer.
- **Launch = `ShellExecuteW("shell:AppsFolder\<AUMID>")`** — uniform for all app types.
- **Match pipeline** (deterministic/offline, on the ASR'd name minus the verb): normalize →
  exact → substring/token (vendor prefixes: "chrome"→"Google Chrome") → phonetic (reuse
  `phonetic.rs`) → fuzzy edit-distance (`strsim` or hand-rolled) → confidence score.
- **Confidence drives safety:** strong → launch + undo toast; weak/ambiguous → pill *confirms*
  first; below threshold → "no app called X", **never guess-launch**.
- **"open" vs "switch to":** v1 "open" just launches (Windows focuses most single-instance apps
  on re-launch); true focus-if-running via `FocusApp` window-find is a later nicety.
- Resolution is deterministic — the LLM is NOT used here (reserved for Phase 1 free-form intents).

**Pill UI/UX (resolved 2026-06-20):** new `command` variant in `recordingStore.RecordingStatus`
rendered in `FloatingPill.tsx`, driven by `command-state-change` + `command-result {summary,
undoable, pid?}` events (mirrors the structured-output flow).
- **Distinct identity:** amber/violet accent + ⚡/command glyph + "Command" label (vs dictation's
  blue mic) — so it's unmistakable that this executes, won't type.
- **States:** `listening` (R-Ctrl held, waveform) → `recognizing` (spinner) → matched preview
  ("▶ Open Spotify") → `done` ("✓ Opened Spotify · undo", ~4s) | `confirm` ("⚠ Open 'X'? ⏎/esc")
  | `error` ("✗ no app called 'X'").
- **Confirmation = max(action risk, match uncertainty)** — v1 actions are all reversible, so only
  a low-confidence app match promotes to confirm. Confirm mechanism built now; deferred
  consequential verbs reuse it later.
- **Undo** scoped to launches (track launched PID → close on undo); chords/media show "✓ Done",
  no fake undo. **Esc always cancels** a pending/confirm (the kill-switch).

- `storage/{types,settings}.rs`: add `command_mode` bool (one field + one tuple in the
  settings `pairs` array; bump the count).
- `postprocess/voice_commands.rs`: add an arg-carrying variant + prefix-match branch for
  `open <X>`; reuse `phonetic.rs`.
- `focus.rs`: add `launch_process` (`ShellExecuteW` + first-cut PATH + Start-Menu `.lnk`
  resolver; AUMID later).
- `output/router.rs` (or new `output/os_actions.rs`): execute the new action variant.
- `pipeline.rs`: branch at the existing `if voice_commands_enabled && structured.is_none()`
  seam (~793–844).
- `hotkey.rs`: add a **second independent hotkey** (default **Right Ctrl**, hold) routing
  `fire_start`/`fire_stop` to a command-capture path; dictation stays Ctrl+Alt. The state
  machine holds one `HOTKEY_PACKED` today ([hotkey.rs:55](../src-tauri/src/hotkey.rs)) — extend
  to a second packed value + parallel KEYS_DOWN/RECORDING.
- Frontend: `lib/tauri.ts` flag; `SettingsPage.tsx` toggle (mirror the Voice Commands group);
  toast on `transcription-result`.

### Phase 1 — LLM fallback for free-form commands
- `resources/grammars/command_intent_v1.gbnf` + `CommandIntent` struct beside `llm/schema.rs`;
  export consts in `llm/grammar.rs`.
- Generalize `llm/engine.rs::generate` to take grammar+root; add `runner.rs::classify_with_timeout`;
  add a command-router prompt in `llm/prompt.rs`.
- `pipeline.rs`: on registry miss → classify → map `CommandIntent` → executor; `no_op` →
  fall back to dictation.

### Phase 2 — Confirmation UX + window/UIA actions
- `overlay/CommandPanel.tsx` (clone `StructuredPanel`); `command-detected`/`command-executed`
  events; `command` pill in `recordingStore` + `FloatingPill`.
- Fork `screen_context/windows.rs` → `capture_actionable`; UIA `InvokePattern` for
  "click `<element>`"; `focus_window`.

### Phase 3 — Parakeet ASR + wake word (decoupled, optional, largest)
- **Make `AsrEngine` real:** widen `state.engine` to `Arc<dyn AsrEngine>`; lift preview/prompt
  methods into traits; branch `commands/models.rs::load_and_activate_model` on format; make
  `models/{manager,downloader}.rs` format-aware. Add `asr/parakeet.rs` via `transcribe-rs`
  (proven on Windows by Handy) or `parakeet-rs` (spike the Windows/DirectML path). Adds the
  `ort` dependency + its Windows DLL bundling.
- **Wake word** via `livekit-wakeword` + an always-on, VAD-gated capture loop with a 0.5–2 s
  in-RAM pre-roll ring buffer (generalize the preview worker / `audio/capture.rs` from batch
  into a streaming listener — the biggest single new piece).

**Effort honesty:** Phase 0 = days. Phase 1 = days–week. Phase 2 = week+ (UIA + new UX).
Phase 3 = the largest chunk and the *least* essential to a working Jarvis — sequence it last,
behind a feature flag.

---

## Part 5 — Risks, safety, open questions

**Safety:**
- Treat the LLM's action plan as **untrusted input** before it drives mouse/keyboard.
  Validate every `CommandIntent` against the closed enum + a resolved, known target first.
- **Closed action enum, never free-text.**
- Confirmation scales with criticality; provide a global **"stop" kill-switch** that aborts
  any pending action, and **undo** for reversible ones.
- Never persist the wake-word pre-roll buffer to disk; show a clear mic-active indicator.

**Open questions the dev must decide:**
1. **English-only or multilingual?** Decides whether Parakeet can ever *replace* Whisper or
   only *supplement* it (and v2 vs v3).
2. **Wake word in scope, or is push-to-talk enough?** Push-to-talk avoids the entire
   always-on capture rebuild and all wake-word tuning.
3. **How far does "control the computer" go?** Launch + focus + keystrokes is days. Arbitrary
   "click any button in any app" (UIA actuation) is materially more work + UIPI/accessibility
   limits. Scope explicitly.
4. **Separate Qwen session for command prompts, or accept KV-cache thrash on mode switch?**
   Affects perceived command latency.
5. **Data-driven command registry (SQLite table, mirroring `mode_app_bindings`) or
   code-defined?** Data-driven lets users add "open `<their app>`" without a rebuild but adds
   a table + migration (`storage/database.rs`, bump `user_version`). Code-defined ships faster.

---

## Appendix — key source URLs

- Parakeet: `huggingface.co/nvidia/parakeet-tdt-0.6b-v2` · `…-v3` · Open ASR Leaderboard · arXiv 2509.14128
- Rust/Windows runnability: `github.com/cjpais/Handy` · `…/transcribe-rs` · `github.com/altunenes/parakeet-rs` · `k2-fsa.github.io/sherpa` · `github.com/mudler/parakeet.cpp`
- Voice arch: `github.com/livekit/livekit-wakeword` · `github.com/snakers4/silero-vad` · `github.com/dscripka/openWakeWord` · `picovoice.ai/docs/quick-start/porcupine-rust`
- Patterns: `talonvoice.com/docs` · `rhasspy.readthedocs.io` · `home-assistant.io/blog/2025/09/11/ai-in-home-assistant` · `github.com/serenadeai/serenade`
- OS automation: `windows` + `uiautomation` + `enigo` crates
