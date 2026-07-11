# Command Mode / Jarvis + Pipeline Hardening Audit — 2026-07-10

> **Scope:** `335287d^..HEAD` (v0.4.0 Voice Command Mode → HEAD; 197 files). Focus: the control/trust
> boundary (Command Mode / "Jarvis") and the text-cleaning + LLM pipeline. Verify cited lines before editing.
>
> **Method:** five independent lenses — a first-hand read (Claude), three parallel Opus deep-reads
> (command-execution safety / LLM+text pipeline / frontend), and a **Codex `gpt-5.6-sol` @ xhigh** read-only
> review. Every finding was cross-checked against the code; two Claude↔Codex disagreements were adjudicated
> against the source (below).

## Verdict

The **static** safety boundary is genuinely well-built (closed enum, GBNF, all-or-nothing parse, graceful
degrade). Several **runtime** invariants do **not** hold: which window an action lands in, whether a confirm is
fresh/identity-bound, and whether "stop" actually stops. There is also one repro-confirmed crash on the
*everyday dictation path* (unrelated to Command Mode).

---

## Batch 1 — shipped 2026-07-10 (surgical, unit-tested)

| ID | Sev | Fix | Files |
|----|-----|-----|-------|
| C1 | 🔴 Critical | UTF-8 slice panic in sentence splitter + list-connector strip → boundary-safe `rsplit`/`get` | `postprocess/formatter.rs` (+tests) |
| H7 | 🟠 High | `to_lowercase()`/original-index desync (silent corruption) → `find_ascii_ci` raw-byte ASCII search at all 3 sites | `postprocess/processor.rs` (+tests) |
| M2 | 🟡 Med | `Prewarm` no longer creates a `BusyReset` (was clearing a concurrent request's busy slot) | `llm/runner.rs` |
| M5 | 🟡 Med | Transcript + foreground-app title sanitized (`<|`/`|>`/control chars) before ChatML — matches screen-token treatment | `llm/prompt.rs` |
| M1 | 🟡 Med | URL grounding applied to **every** OpenUrl in a chain; grounds on the registrable label (anti-subdomain-spoof); `run_open_url` rejects userinfo `@` | `pipeline.rs`, `actions/executor.rs` (+tests) |
| M4 | 🟡 Med | `confirm_pending_command` emits a terminal `idle` when nothing is parked → pill can't wedge in "confirm" | `pipeline.rs` |
| H4 | 🟠 High (partial) | Enter/Esc confirm-hijack window 15 s → 6 s (exposure bound; full fix in Batch 2) | `hotkey.rs` |

**Codex `gpt-5.6-sol` re-reviewed the Batch 1 diff** and caught two real defects (now fixed + tested):
- **The M1 fix retained a UTF-8 panic** — a rejected *non-ASCII* dictionary match (`éclair` in `xéclair`) advanced
  `abs_pos + 1` (one byte into a multibyte char). Fixed: advance by `char::len_utf8`; added a panic-regression test.
- **M5 was only half-applied** — the *structured* prompt was sanitized but `format_command_prompt` still
  interpolated the raw utterance. Fixed: `sanitize_prompt_field` now covers the Command Mode prompt too.
Re-review confirmed formatter/runner/executor fixes correct. Remaining Codex notes fold into Batch 2: URL grounding
still needs real `url::Url` + Public-Suffix-List parsing (`github.evil.com.au` defeats the registrable-label
heuristic; substring match still grounds `app.com` by "application"); confirmation needs ID/generation binding
(and the M4 emit-on-None should be generation-gated to avoid clobbering a newer command's `listening`); dictionary
matching is now ASCII-case-insensitive (accented triggers match exact-case — the previous Unicode behavior was
itself broken by the desync bug). Net: **325 tests green, clippy clean.**

---

## Findings by severity (full)

Attribution: **Claude** = my first-hand read; **A/B/C** = Opus subagents (cmd-safety / pipeline / frontend); **Codex** = gpt-5.6-sol.

### 🔴 Critical
- **C1 — UTF-8 slice panic crashes the core plain-dictation path.** `formatter.rs:262` (`&before[word_start..]`,
  `rfind(..).map(|p|p+1)` splits mid-char on curly apostrophe/em-dash/ellipsis; `"I don't."` panics) and the same
  class at `:350` (`trimmed[..prefix.len()]`). Runs on every non-structured dictation; `panic=unwind` silently
  drops the dictation and can wedge the state machine. *B (repro) + Claude (verified).* **FIXED (Batch 1).**

### 🟠 High
- **H1 — Command target isn't bound; a concurrent dictation redirects a confirmed send.** `prev_foreground` is a
  global mutable slot written by both command (`pipeline.rs:1255`) and dictation (`:205`) capture and re-read by
  `run_chain` at execution (`:1978`). Park "send … to Teams" → dictate into Notepad (overwrites slot) → confirm →
  pastes+Enters into Notepad. *Codex #1 + A/Claude (stale-HWND).* **Deferred → Batch 2.**
- **H2 — "Foreground is sacred" not enforced at the primitive.** `SetForegroundWindow` result ignored
  (`focus.rs:172/181`); Enigo fires after a fixed sleep regardless; a `None` target fires at whatever's foreground
  (`pipeline.rs:1851/1901`). *Codex #2.* **Deferred → Batch 2.**
- **H3 — `settle_after_launch` accepts any window as the launched app.** Returns the first non-own foreground
  window, no PID/AUMID check (`pipeline.rs:2086-2096`); a notification/Alt-Tab during settle hijacks the chain and
  poisons undo (`WM_CLOSE` an unrelated window). *Codex #3 + A/Claude.* **Deferred → Batch 2.**
- **H4 — 15 s global Enter/Esc hijack executes a parked confirm from any app** (`hotkey.rs:274-293`). *Claude + A +
  Codex #6.* **Mitigated (window→6 s, Batch 1); full identity-bound fix → Batch 2.**
- **H5 — Default-on inline voice commands bypass the confirm gate.** `router.rs:367-461` `run_command` fires
  `Send`(Enter), `MouseClick`, `KeyCombo`, and `LaunchApp(command_line)` with no pill; `"sent"→"send"` mishearing
  auto-submits. *Codex #5 + Claude (verified).* **Deferred → Batch 2 (product decision).**
- **H6 — "Stop" is a racy cancellation boundary.** `run_chain` clears `command_abort` on entry, discarding a
  "stop" spoken during classification; single intents never check it (`pipeline.rs:1970/1993`). *Codex #4 —
  refines Claude's earlier "abort holds".* **Deferred → Batch 2.**
- **H7 — `to_lowercase()`/original-index desync corrupts text.** `processor.rs` dictionary/snippet + 2 filler
  loops. *B (repro) + Claude (verified).* **FIXED (Batch 1).**
- **H8 — Global-hotkey dictation into an open StructuredPanel orphans the recording.** Ref lag closes the panel
  and drops the in-flight recording (`useOverlayEvents.ts:225` vs `FloatingPill.tsx:127`). *C (plausible).*
  **Deferred → Batch 3 (frontend).**

### 🟡 Medium
- **M1** URL grounding bypassable (lone-OpenUrl only; first-label substring; `github.com@evil.example` userinfo).
  *Claude/A/Codex #7.* **FIXED (Batch 1).**
- **M2** `Prewarm` over-clears the LLM busy slot → breaks single-flight backpressure. *Codex #8 (adjudicated).*
  **FIXED (Batch 1).**
- **M3** Model activation not single-flight → concurrent double GGUF load can exhaust RAM/VRAM. *Codex #9.*
  **Deferred.**
- **M4** Command pill has no client-side timeout; `confirm_pending_command` emitted nothing on `pending==None` →
  wedge. *C.* **FIXED (Batch 1, backend emit); UI watchdog still recommended.**
- **M5** ChatML injection via transcript + other-app window title (`parse_special=true`; screen tokens were
  sanitized, these weren't). *B + Codex #10.* **FIXED (Batch 1).**
- **M6** Structured input cap is char-based not token-based → dense/CJK silently disables Structured Mode and
  thrashes the KV cache. *B + Codex #11.* **Deferred.**
- **M7** Success flash is dead code in the overlay (`lastTranscription` only set in the main window). *C.*
  **Deferred → Batch 3.**
- **M8** Stale/recycled HWND at unbounded mouse-confirm/undo time (no `IsWindow`/PID re-check). *Claude/A/Codex #6.*
  **Deferred → Batch 2 (covered by the identity-verification work).**

### 🔵 Low
non-submit type-text unconfirmed (A); latent stuck-success timer, exposed once M7 is fixed (C); editable-confirm
textarea missing `autoFocus` (C); grammar can exceed the 384-token budget → needless degrade (B/Codex #11);
worker-thread panic wedges the runner, no self-heal (B); hook-thread mutex latent foot-gun — invariant to preserve
(Claude/A); `resizeOverlay` unhandled rejections + ley-line popup asymmetry (C); `explorer.exe` url arg, mitigated
by the `https://` prefix (Claude); meaning-changing filler removal — design choice (B).

---

## Claude ↔ Codex disagreements (resolved against the code)
1. **LLM busy slot.** *Claude (subagent B):* "no leak." *Codex:* "`Prewarm` over-clears." → **Both correct**,
   different failure modes. B verified no *stuck-true* leak (and that the "drop-before-send" fix is complete);
   Codex found an *over-clear* — `prewarm()` sends via `try_send` bypassing `submit` (never sets `busy`), yet the
   worker built a `BusyReset` for it (`runner.rs:161`) whose drop cleared a concurrent extraction's flag. Net: a
   real Medium bug (M2), now fixed.
2. **Cancellation.** *Claude:* "abort invariant holds." *Codex:* "clear-on-entry discards a stop during
   classification." → **Codex correct** on the finer point; Claude's claim only covered the stale-stop-from-a-
   previous-run case. Net: a real cancellation race (H6), deferred to Batch 2.

## Verified genuinely solid (regression-guard these)
Closed 27-action enum is load-bearing (unknown actions rejected even if the grammar were bypassed); all-or-nothing
chain parse; grammar↔schema↔struct aligned; GBNF applied as the first sampler; command/extraction session
isolation + KV prefix contract; graceful degrade on bad JSON; **no ReDoS**; **no shell-arg injection in app
launch** (AUMID from `Get-StartApps`); non-http schemes neutralized by the `https://` prefix; editable
send-confirm sends the *edited* text (confirmed 3×); no frontend listener leaks / no injection; busy-slot has no
stuck-true leak and the drop-before-send race fix is correct.

---

## Batch 2 — deferred (needs a design nod; not done while owner was away)

**The root cause of H1/H2/H3/M8 is one thing: the command target and the confirm are unbound, mutable, and
re-read late.** A single change retires them together:

- Introduce an **immutable per-capture `CommandContext { id, target_hwnd, target_pid, captured_at }`**, created at
  command capture and carried through ASR → classify → pending → execute → undo (replacing reads of the shared
  `prev_foreground` slot at execution time).
- **Re-verify identity before every side-effecting primitive:** `IsWindow(hwnd)` + `GetForegroundWindow()==target`
  + PID match before any paste/Enter/`WM_CLOSE`/focus-restore; re-check between paste and the submitting Enter.
- **`settle_after_launch`:** correlate the new foreground window's PID/AUMID with the launched AppsFolder entry;
  refuse focus-dependent steps + skip the undo record if identity can't be proven.
- **Confirmation:** bind an unguessable confirm ID + deadline to the `CommandContext`; the UI/hook must present a
  matching, fresh ID; atomically consume only that entry (completes H4 — the hook only hijacks Enter when the
  arm-time foreground is still current).
- **H6:** replace the shared `command_abort` bool with a monotonic cancellation generation / per-command token;
  never clear it inside `run_chain`; check at the final sink boundary.
- **H5 (product decision):** route the inline dictation voice-commands `Send`/`LaunchApp`/pointer/disruptive-key
  actions through the same target-binding + confirm policy, or make the consequential ones opt-in with a dedicated
  gesture.

**Batch 3 (frontend):** H8 panel-race, M7 dead success flash, and the Low frontend items.
**Also deferred:** M3 (single-flight model load), M6 (token-based input cap), the runner self-heal, and the UI
confirm watchdog.

## Secondary surface — brief pass (Fable, 2026-07-10)

A lighter sweep of the *rest* of the diff (storage/migrations, `lib.rs` startup/watchdog, analytics, UI page
rewrites). **Verified clean:** `database.rs` migrations (rename guarded both ways, idempotent, no data loss —
old command layer never let users write `llm_prompt`); `context_modes` cascade-delete + orphan purge; settings
serialization; `analytics/compute.ts` (DST-safe day math, streaks, heatmap, WPM); monitor work-area math (fixes
the old hardcoded-taskbar bug); `router.rs` `with_modifier` always-release; `launch_app` spawns without a shell.

Notable findings (deferred — outside the original deep scope):
- **SS1 [Medium] Renamed built-in voice commands resurrect on every launch.** `storage/voice_commands.rs:119-158`
  `seed_missing_builtins` keys on the mutable `phrase`; the UI lets users rename built-ins, so a rename is silently
  re-inserted (and duplicated) at next startup. Fix: key the backfill on the encoded `action` / a stable built-in
  key, not `phrase`. *CONFIRMED.*
- **SS2 [Medium] Navigating away mid-remap suspends the global hotkey hook app-wide.** `settings/HotkeySection.tsx:32-77`
  enters "listening" → `suspendHotkey(true)`; only Cancel/Save resume, so unmounting mid-remap kills push-to-talk
  (dictation *and* command) until remap/restart. Fix: `suspendHotkey(false)` in an unmount cleanup. *CONFIRMED.*
- **SS3 [Low-Med] `Row` defined inside `VoiceCommandsPage` remounts the rename input each keystroke.**
  `features/commands/VoiceCommandsPage.tsx:224` — caret jumps to end, mid-string edits unusable. Fix: hoist `Row`
  to module scope. *CONFIRMED.*
- **SS4 [Low] In-app dictation can write into a detached DOM node** if the target editable unmounts mid-record
  (`hooks/useInAppDictation.ts:84-93`); suppresses the fallback → dictation lost. Fix: check `target.isConnected`.
  *PLAUSIBLE.*
- **SS5 [Low] Autostart reconcile can't fix path drift** — `is_enabled()` checks Run-key presence, not path
  correctness (`lib.rs:544-561`); a moved exe stays stale. Fix: call `enable()` unconditionally when `want`.
  *PLAUSIBLE.*
- **SS6 [Info] Overlay watchdog re-asserts `HWND_TOPMOST` + `show()` every 3 s unconditionally** (`lib.rs:139-215`)
  — sound (no spin, `SWP_NOACTIVATE`), but by design fights fullscreen apps/games. Judgment call.

## Batch 2 Codex review — follow-up fixes (round 2)

Codex gpt-5.6-sol @ xhigh reviewed the Batch 2 diff and found real defects (green tests can't catch these).
M6 fully resolved; runner self-heal + Rust↔TS contract correct. To fix:

- **B2-1 [High] cancellation floor** (`pipeline.rs:1471`): uses `store(ctx.id)` — an older delayed stop can LOWER a
  newer floor; also clears the pending slot unconditionally (incl. newer commands); not re-checked before the
  actual launch (`:1970`). Fix: `fetch_max`; clear only pending ids `<= stop_id`; re-check cancellation inside
  every side-effecting sink.
- **B2-2 [High] confirm-ID TOCTOU** (`pipeline.rs:2451/2465`): peek under one lock, then re-lock and take whatever
  is present → a stale confirm can consume a newer command. Hook spawns `confirm_id=None` matching ANY pending
  (`hotkey.rs:318`). Fix: compare-and-take atomically under one guard; arm the hook with the exact pending id;
  enforce a deadline; generation-tag `emit_command_idle_if_free`.
- **B2-3 [High] inline Send blind-fire** (`router.rs:390` proceeds when target `None`; `:421` sleeps 150 ms then
  Enter): capture immutable `WindowTarget{hwnd,pid}` at dictation START (`pipeline.rs:1076` captures pid late);
  reject absent targets for consequential actions; re-verify immediately before Enter / each pointer primitive.
- **B2-4 [High] launched-app identity** (`pipeline.rs:2393`): accepts any real non-own window; never correlates
  PID/AUMID with the launched app. Fix: launch returns expected pid/package; accept only matching windows; else
  abort focus steps + skip undo. (Hard for AppsFolder launches — do best-feasible + document residual.)
- **B2-5 [High] URL grounding uses a home-grown suffix subset** (`pipeline.rs:1699`): `com.my` etc. omitted →
  `github.evil.com.my` grounds. Fix: use the `publicsuffix` crate (embedded PSL) for eTLD+1.
- **B2-6 [Med] focus identity** (`focus.rs:28` accepts `expected_pid=None`; `:314` sends deselection arrows before
  verifying `:316`): require pid for Windows primitives; verify BEFORE the arrow keys and immediately before the
  primitive.
- **B2-7 [Med] hook-thread Win32** (`hotkey.rs:305` `GetForegroundWindow` on the WH_KEYBOARD_LL thread): it's a
  cheap non-blocking read (unlike the banned UIA/GetWindowText), but violates the stated invariant — mitigate via a
  WinEvent-hook atomic snapshot, or document why the cheap read is acceptable.
- **B2-8 [Med] M3 async-safety** (`commands/llm.rs:216`): `std::sync::Mutex` held across the blocking load+join from
  async paths (stalls tokio workers); double-check returns any runner without checking the active model id. Fix:
  run wait/load on blocking infra; key single-flight on the canonical model id.
- **B2-9 [Low] runner init panic** (`runner.rs:129`): initial `new_session_for` is outside `catch_unwind`. Wrap
  init / recreate on init-panic.
- **B2-10 [Low] LaunchApp opt-in** (`settings.rs:290` reads a hidden key nothing writes; UI still creates Launch
  actions): add a real typed setting + UI toggle. **OWNER DECISION: default ON** (owner prefers features full-on;
  the B2-3 identity-binding makes inline commands safe) — flag for confirmation.
- **Contract nit:** TS confirm helpers accept `number | null`; tighten to `number`.

## Batch 2 Codex sign-off — closing fixes (round 3)

Resolved by round 2: B2-4 (fail-closed), B2-5 (PSL), B2-7 (documented), B2-9, M6. Closing round:
- **B2-11 [High]** dictation target lives in shared `state.dictation_target` and is reread late (`pipeline.rs:1104`) — an overlapping dictation can overwrite it → A sends into B's window. Thread the `WindowTarget` through the stopping dictation's LOCAL context; drop the late pid recapture fallback.
- **B2-12 [High]** only Send/Enter/KeyCombo/pointer verify identity; `DeleteLastWord`/Cut/Undo/Redo/SelectAll/Tab/Escape don't (`router.rs:402`). Verify EVERY focus-dependent inline primitive.
- **B2-13 [High]** cancellation floor not checked INSIDE the `spawn_blocking` closures for KeyChord/Media/Window/WebSearch/OpenUrl/TypeText (`pipeline.rs:2343+`); stop between paste and Enter not consulted; undo bypasses the floor. Pass the cancel token into every closure + recheck before each primitive.
- **B2-14 [High, narrow]** hook reads foreground then ID as separate atomics (`hotkey.rs:327`) — a sub-ms window lets a newly-armed pending be confirmed by a stale Enter. Pack the hook state / `compare_exchange` the id. (Very narrow; document if a clean pack isn't cheap.)
- **B2-15 [Med]** `restore_foreground_window` verifies only HWND before the deselection arrows (`focus.rs:332`); a recycled HWND in another process gets arrows before PID rejection. Verify HWND+PID before deselection.
- **B2-16 [Med]** `runner_for_model` reads `active_llm_model_id` + `llm_runner` under separate mutexes (`llm.rs:216`) → a concurrent switch returns X for Y. Store `(model_id, Arc<runner>)` atomically under one mutex; remove the fast-path that bypasses the keyed check.
- **B2-17 [Med]** the LaunchApp toggle gates only inline custom Launch, not Command Mode OpenApp (`pipeline.rs:1122`) — but gating "open Spotify" would cripple the core feature, so RELABEL the toggle to state it controls custom Launch voice-commands only (owner prefers OpenApp stays on).

## Post-test fix — B2-4 relaxation (2026-07-10)

Live testing showed B2-4's fail-closed refusal broke the flagship "tell <app> to …" flow: AppsFolder/Store
launches (Claude, etc.) go through `explorer.exe`, which never exposes the child pid, so `settle_after_launch`
always returns `identity_proven=false` and `run_chain` refused the send ("couldn't confirm the launched app's
window"). Relaxed: retarget to the settled window regardless of `identity_proven`, and let the per-primitive
execution-time verify (`execute_intent_now` / `run_type_text`: foreground+pid immediately before firing) be the
gate; only refuse when NO window stabilizes. `identity_proven` still gates undo recording. Verified: "open Notepad
and type hello world" lands. **Residual:** apps whose text input isn't focused on activation (e.g. Claude's
composer — Electron, UIA-skipped) still receive keystrokes into an unfocused field; the rigorous fix is AUMID
window-identity correlation + composer focusing.

## Verification baseline
`cd src-tauri && cargo test` · `cargo clippy --all-targets` · `npm run build` · manual: dictate `"I don't."` into
Notepad (Batch-1 crash regression); Right-Ctrl "open notepad and type hello".
