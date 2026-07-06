# Plan: Command Mode → "Jarvis" — Roadmap to a Background-Capable Voice Assistant

> **For agentic workers:** This is the flagship roadmap from the 2026-07-06 audit (v0.4.0 / commit `335287d`). It is bigger than one session — phases A–C are conventional feature work; D–E are greenfield architecture. **Do phases in order**: A (safety/control) is a hard prerequisite for everything after it. Verify cited lines before editing. Read `docs/voice-command-mode-research.md` (the v0.4.0 research doc) for deep background — its Phase 2/3 items that went unbuilt are folded in here.
>
> **STATUS (2026-07-06, branch `claude/audit-implementation`):** Phase A — PARTIAL: keyboard confirm/cancel (Enter/Esc via the hook, 250ms debounce + 15s freshness, armed at every `pending_command` park site), Esc-cancels-command-capture, global stop (`AppState::command_abort` set by tier-0 spoken "stop"/"cancel" checked before matcher/LLM; honored between `run_chain` steps), and bare-OpenUrl grounding confirm (`url_grounded_in_utterance`) are DONE. **Still open in A: undo (last_action slot + spawn PID retention) and the editable send confirm.** The FloatingPill split (prereq) landed: orchestrator + useOverlayEvents.ts + QuickToggles.tsx + PillContent.tsx. Phases B–F — untouched.

**Product vision (from the owner):** a Jarvis on your computer. Ask it to split off a background task, do something while you keep working in the foreground, or prepare something for your next task — all local-first.

---

## 1. Where Command Mode stands today

Shipped in v0.4.0. Right-Ctrl push-to-talk (separate FSM from dictation, `hotkey.rs:110-111`), shared mic/Whisper via `CaptureMode` mutual exclusion (`state.rs:25-27`).

**Two-tier brain** (`pipeline.rs:1228-1322`):
1. Deterministic matcher (`actions/matcher.rs`) — exact/synonym tables + open-verb prefixes; defers to tier 2 on " and "/" then " (`matcher.rs:103`).
2. Qwen LLM fallback — grammar-constrained (`resources/grammars/command_intent_v1.gbnf`: JSON array ≤5 of `{action ∈ closed enum of 27, target: string}`), throwaway KV session so Structured Mode's warm cache survives (`engine.rs:230-233`), all-or-nothing chain parsing (`intent.rs:180-199`).

**Closed intent enum** (`actions/intent.rs:87-109`) — the safety boundary: `OpenApp, KeyChord(10 chords), Media(6), Window(Min/Max), WebSearch, OpenUrl, CloseWindow, TypeText{submit}`.

**Execution** (`actions/executor.rs`, Windows-only): enigo chords, raw VK media keys, `ShowWindow`, graceful `WM_CLOSE`, `shell:AppsFolder\<AUMID>` launch, clipboard-verified type-text, default-browser URL/search. Chains do focus-retargeting after launches with blind-fire refusal when focus is unverified (`pipeline.rs:1616-1697`).

**Safety today:** confidence-graded app launch (FLOOR 0.50 / AUTO 0.94 / ambiguity margin, `app_index.rs:37-45`), confirm pill for fuzzy opens, CloseWindow, and any submitting TypeText chain (`pipeline.rs:1297-1307, 1419-1459`); single-slot `PendingCommand` (`state.rs:36`).

**What does NOT exist** (the gaps this plan closes): keyboard confirm / global stop, undo, real focus-if-running, UIA element actuation, screen-aware commands, any background/async task model, cross-utterance memory, per-app integrations, wake word / always-on listening, user-extensible command registry.

Supporting assets already in-repo: `examples/command_ab.rs` (labeled A/B harness for swapping the command brain), `screen_context/` full UIA text walk (text-only, skips `claude.exe`/`codex.exe`/webviews — `mod.rs:80-86`), `test_command` dry-run (`pipeline.rs:1827`), `.agent-collab/parakeet-research.workflow.js` (prior Parakeet/Jarvis research).

---

## 2. Architecture principle for everything below

**Extend the closed-enum + confirm-pill safety model; never bypass it.** Every new capability is (a) a new enum variant with grammar + prompt + executor + confirm policy, or (b) a new *engine* (task engine, tool registry) whose individual operations still resolve to enumerated, policy-checked primitives. A hallucinating model must remain physically unable to invoke a verb that wasn't enumerated.

---

## Phase A — Control & safety foundations (prerequisite)

Trust is the product. A Jarvis that can't be interrupted or undone will get disabled.

**Files:** `src/features/overlay/FloatingPill.tsx`, `CommandPill.tsx`, `src-tauri/src/pipeline.rs`, `src-tauri/src/hotkey.rs`, `src-tauri/src/actions/app_index.rs`, `src-tauri/src/state.rs`

- [ ] **Keyboard confirm/cancel.** The confirm pill is mouse-only today (`CommandPill.tsx:114-135` onMouseDown; zero Enter/Esc handling in `FloatingPill.tsx` — grep confirms). The overlay window is `focused(false)`, so DOM key events won't arrive: wire Enter/Esc through the existing low-level hook (`hotkey.rs`) — when `pending_command` is set, a dedicated hook path swallows Enter→confirm / Esc→cancel. Careful: only while a confirm is actually pending, and never when the user is typing in another app — gate on pending state + a short freshness window.
- [ ] **Global stop.** (1) Esc during `listening/recognizing` cancels the capture; (2) an `AtomicBool` abort flag checked between chain steps in `run_chain` (`pipeline.rs:1616-1697`) and before firing any executor primitive; (3) a spoken tier-1 phrase "stop"/"cancel" in `matcher.rs` zero-arg table that sets the same flag. Research doc promised this; it's the cheapest trust win available.
- [ ] **Undo (minimum viable).** Track what's undoable per executed intent in a small `last_action` slot: launched app → keep the spawned child PID (today dropped at `app_index.rs:261` `.spawn().map(|_| ())`) → undo = WM_CLOSE its top window; Minimize/ShowDesktop → restore; TypeText(no-submit) → Ctrl+Z into same target. Surface as "undo that" tier-1 phrase + an Undo affordance on the `done` pill state (auto-reset currently 2.6 s, `FloatingPill.tsx:333` — extend when undoable). Submitted messages are not undoable — that's what the confirm gate is for.
- [ ] **Editable send confirm.** `TypeText{submit:true}` confirm shows the message only inside the summary string; a mishearing sends verbatim on Yes. Reuse `StructuredPanel`'s edit-textarea pattern for a "review message" confirm variant (payload = editable text + target window title).
- [ ] **Confirm web actions minimally:** `WebSearch`/`OpenUrl` currently fire unconfirmed with unbounded LLM-produced targets (`executor.rs:203-267`). Keep search auto (low risk), confirm bare `OpenUrl` when the URL didn't appear in the utterance.

**Verify:** manual matrix — Esc cancels at each pill state; "stop" aborts a 3-step chain mid-flight; undo reverses open/minimize; a mis-dictated "tell Slack..." can be edited before send.

## Phase B — Action vocabulary depth

The uniform 5-touch extension seam (enum `intent.rs` → `from_llm` → grammar token → prompt example → executor + `run_intent` arm) makes each of these small. **Keep the grammar/prompt/enum triplication in sync** — consider generating the GBNF action list and prompt enum from a single Rust const table first (`grammar action enum is triplicated across command_intent_v1.gbnf:5, prompt.rs:160-172, intent.rs:119-165`).

- [ ] **Single-source the action enum** (codegen or const-table + build-time check) before adding verbs.
- [ ] **`FocusApp` for real** — find-and-activate a running process (EnumWindows → match process name via `focus.rs:121` helpers → `SetForegroundWindow`), falling back to launch. Today `focus_app` collapses to `OpenApp` = relaunch (`intent.rs:120`).
- [ ] **Named-app close** ("close Spotify") — resolve name via app index + running-window match, WM_CLOSE, always confirm. Today the prompt actively forces these to `none` (`prompt.rs:179`).
- [ ] **Parameterized actions** — the grammar's flat `target: string` blocks "set volume to 30" / "snap notepad left". Widen the grammar object with an optional `arg` string; parse per-action. Volume set (ISimpleAudioVolume/keybd fallback), window snap (Win+arrows), "type it N times"—no; keep scope to volume + snap.
- [ ] **OmniVox-internal actions** — "take a note: ..." (append to Notes via existing storage, `storage/notes.rs`), "switch to programming mode" (context mode activation — plumbing exists in `commands/context_modes.rs`), "read back my last dictation" (from history). These make the assistant feel alive at near-zero risk and exercise the internal-tool pattern Phase E needs.
- [ ] **User-extensible aliases** — SQLite table `phrase → enumerated action + target` mirroring `mode_app_bindings` (researched, unbuilt). Tier-1 matcher consults `builtin().chain(user_rows())`. This is the safe personalization channel (users can't define new *verbs*, only new phrases for existing ones).

## Phase C — Screen awareness (the "look at my screen" half of Jarvis)

- [ ] **`capture_actionable`** — fork `screen_context/windows.rs` UIA walk to retain `IUIAutomationElement` handles for invokable controls (buttons, links, tabs: `InvokePattern`/`SelectionItemPattern`) alongside their names, instead of flattening to text (`mod.rs:5-6`). Cache per foreground window with the existing <250 ms watchdog + graceful-empty degradation.
- [ ] **`ClickElement(name)` intent** — resolve spoken name against captured element names with the same scoring/confidence tiers as `app_index::score` (`app_index.rs:123-170`); confirm below AUTO. `InvokePattern.Invoke()` = no focus steal, no coordinates, DPI-proof. The research doc's marquee: "click send".
- [ ] **Screen-grounded LLM commands** — feed the *text* capture (already built) into `COMMAND_SYSTEM_PROMPT`'s user turn so tier-2 can resolve "reply to this email saying...", "search for the error on my screen". Mirror the injection-hardening used for structured mode (ChatML sanitization, token cap — `prompt.rs:121-128`).
- [ ] **Constraint to respect:** UIA skips `claude.exe`, `codex.exe`, `msedgewebview2.exe` (they hang under cross-process UIA while automating — `mod.rs:80-86`, v0.2.9 fix). "Control the AI app via UIA" is off the table; controlling those apps goes through keystroke/paste primitives (already how `send_message` works) or Phase E process-level integration.

## Phase D — The task engine (background execution, the core Jarvis piece)

Everything today is synchronous fire-once inside `stop_and_run_command`: chains ≤5 steps, no persistence, no progress, no cancellation mid-step, no memory between utterances, single-flight LLM. Background work is a new layer, not a patch.

**Design (greenfield — write a short spec doc before coding):**

- [ ] **`Task` abstraction** above `CommandIntent`: `{id, title, steps: Vec<CommandIntent> | internal job, status: queued|running|waiting_confirm|done|failed|cancelled, progress, created_from_utterance, result_summary}`. SQLite table for persistence across restarts (follow ad-hoc migration pattern, `storage/database.rs:180-259`).
- [ ] **Executor model:** a tokio task registry in `AppState` (the `Mutex<Option<PendingCommand>>` single-slot becomes a queue). Foreground-affecting steps (keystrokes, focus, paste) MUST still run only when the user grants the foreground — a background task that needs the screen parks in `waiting_confirm` and pings the pill; it never steals focus mid-typing. Pure background steps (LLM jobs, file/note composition, web fetch via browser open — no) run freely.
- [ ] **Progress surface:** `command-task-update` event stream → a task strip on the pill (collapsed count + expandable list, cancel buttons) and optionally a Tasks page. The `_ = app_handle` placeholder in `classify_command_via_llm` (`pipeline.rs:1361`) was explicitly reserved for this.
- [ ] **"Prepare something for my next task"** — the first background job type should be **local LLM composition jobs**: "draft a reply to this email while I finish here", "summarize my last three dictations", "turn my note X into a checklist". Inputs: history (`raw_transcript` preserved), notes, screen-text capture at request time. Output: a Note or a ready-to-paste buffer surfaced via the StructuredPanel pattern. This exercises queue + progress + LLM without any OS-automation risk. **LLM contention:** the runner is single-flight (`runner.rs:214-232`) and shared with Structured Mode + command classify; background jobs must run at *lower* priority — job worker acquires the runner only when idle and yields between generations (chunked max_tokens), or gets a second small model instance — measure memory before deciding.
- [ ] **Short-term session memory** — a rolling transcript of the last N command utterances + results fed to tier-2 classification so "open it again", "same thing but for Chrome" resolve. Keep it in-memory, minutes-scoped; this is not long-term memory. Long-term ("remember that I...") = explicit notes, not implicit profiling — fits the privacy story.
- [ ] **Cancellation & crash safety:** every task checkpoint honors the Phase-A abort flag; on startup, `running` tasks from a previous crash mark `failed(interrupted)` — never auto-resume OS-automation on boot.

## Phase E — Agent delegation ("split off a background task")

The owner's dream: "have it work on something in the background while I keep working." OmniVox should *orchestrate* local agents rather than become one.

- [ ] **Tool/worker registry (spec first):** a registry of delegable workers with per-worker policy: the local LLM (Phase D jobs), and **local agent CLIs** (Claude Code, Codex CLI) spawned as child processes on a named task — e.g. "have Claude look at the failing test in OmniVox repo while I write this doc" → spawn `claude -p "..."` in a configured workspace, stream/poll output into the task record, notify on completion via the pill. Process spawn/monitor is std; the hard part is **configuration UX** (which CLIs exist, which directories are allowed) and **policy** (see below).
- [ ] **Policy model:** each worker/action class carries `auto | confirm | off`, user-configurable on the Commands page. Delegation to an agent CLI defaults to `confirm` with the exact prompt shown (editable — reuse Phase A's editable confirm). Working-directory allowlist. No shell-string passthrough from the LLM: the command line is templated by OmniVox, only the natural-language task text is model-authored.
- [ ] **Local-first line in the sand:** the README's promise is "zero cloud, data never leaves your computer." Spawning a user-installed agent CLI that itself talks to its own cloud is the *user's* existing tool and choice, but OmniVox must present it as such (explicit per-worker consent screen, off by default). Do not add any cloud LLM calls inside OmniVox itself without a top-level product decision.
- [ ] **Completion hand-back:** task results land as Notes / notifications; "what's the status of my task" tier-1 phrase reads the task table. This closes the Jarvis loop: delegate → keep working → get pinged → paste result.

## Phase F — Always listening (wake word) & ASR upgrade

Deferred by design in v0.4.0 research; unlocks hands-free Jarvis but is heavy.

- [ ] **Streaming capture loop:** `audio/capture.rs` is batch push-to-talk; wake word needs an always-on ring buffer + VAD (Silero) + wake-word model, with an explicit privacy switch and pill "listening" indicator. Battery/CPU budget matters.
- [ ] **Parakeet ASR evaluation:** prior research exists (`.agent-collab/parakeet-research.workflow.js`, research doc Phase 3): decouple `Arc<dyn AsrEngine>` (`asr/engine.rs`), add an ONNX (`ort`/`transcribe-rs`) engine, format-aware model manager. Justify with measured latency: command utterances are short; Whisper-small on GPU may already be fast enough — **benchmark before building.**
- [ ] "Press-or-say" hybrid (Apple style): wake word OR Right-Ctrl, same downstream path.

---

## 3. Sequencing & sizing summary

| Phase | Size | Risk | Depends on |
|---|---|---|---|
| A Safety/control | S–M | low | — |
| B Vocabulary | M | low | A (confirm UX) |
| C Screen/UIA | M–L | medium (UIA quirks) | A |
| D Task engine | L | medium | A; B's internal actions helpful |
| E Agent delegation | L | high (policy/UX) | D |
| F Wake word/ASR | L | high (audio arch) | independent; benchmark first |

**Recommended order:** A → B → D(core queue + LLM composition jobs) → C → E → F. C and D are swappable; D delivers the owner's stated dream sooner.

## 4. Invariants

1. Closed enum + grammar constraint stays load-bearing; new verbs are enumerated, never free-form.
2. All-or-nothing chain parsing stays (`intent.rs:180-199`) — no partial execution of misunderstood utterances.
3. Command capture never blocks or corrupts dictation (shared `CaptureMode` discipline, `state.rs:25-27`; hook-thread synchronous claim, `hotkey.rs:140`).
4. Foreground is sacred: nothing types/clicks/focuses while the user is actively working without a confirm; blind-fire refusal (`pipeline.rs:1647-1657`) extends to every new executor.
5. Local-first by default; any worker that leaves the machine is user-installed, explicit, and off by default.
