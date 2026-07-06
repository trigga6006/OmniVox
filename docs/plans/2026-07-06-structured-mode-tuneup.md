# Plan: Structured Mode Tune-Up

> **For agentic workers:** Implementation guide from the 2026-07-06 codebase audit (v0.4.0 / commit `335287d`). Verify cited lines before editing. Phases are independently shippable, ordered by value/risk. Run `cargo test` in `src-tauri/` per task. Note: `docs/structured-mode-plan.md` is the ORIGINAL design doc and is **stale** (it still says "not yet implemented" and describes FunctionGemma/`output_format` slots that never shipped) — trust this doc and the code, not that one.
>
> **STATUS (2026-07-06, branch `claude/audit-implementation`):** Phase 1 — DONE (config dedupe, n_ctx 4096, cap 4000 chars, `truncated_chars` surfaced in the panel); chunking not built (deliberately deferred). Phase 2 — DONE differently than written: eager load on toggle/startup already existed in v0.4.0 (audit missed it); the real gaps fixed were the idle-dropped KV session (new `LlmRunner::prewarm()` fired on recording start) and the toggle-time load blocking the settings command (now spawn_blocking). The `llm-status` event remains open. Phases 3–5 — open. Also fixed: TS `SlotExtraction` missing `context` (Phase 4 item).

**Goal:** Make Structured Mode faster to first-use, robust on long dictations, observable when it misbehaves, and useful beyond its current single "prompt for a coding agent" persona — while preserving its excellent anti-fabrication and graceful-degradation properties.

---

## 1. Current state (audit findings)

### 1.1 Architecture

- Trigger: `pipeline.rs:735-928`. Gates: `structured_mode` on, optional per-utterance "Voxify" gate (`structured_voice_command`), input ≥ `structured_min_chars` (default 40).
- Engine: llama.cpp via `llama-cpp-2`; catalog is Qwen3 0.6B / **1.7B Q8_0 (default)** (`llm_models/manager.rs:155-184`).
- Prompt: ~1,900-token frozen system prompt (`llm/prompt.rs:13-83`), intent-aware (implementation / exploration / advice shapes), 4 few-shot examples, ChatML + `/no_think`. **Byte-identical every call — this is load-bearing for the KV cache.**
- Decoding: GBNF grammar `slot_extraction_v1.gbnf` forces a minified JSON object — `goal` required; optional `context, constraints, files, urgency, expected_behavior, questions, options`; arrays ≤9 items; strings ≤520 chars; greedy sampling, temperature 0 (hardcoded).
- Post-processing (`llm/schema.rs:94-180`): dedupe across slots, drop ungrounded `files`, short-input fabrication guard, third→first person rewrite. **This is the maturity center — don't weaken it.**
- Output: deterministic markdown render (`llm/template.rs:8-105`) → `structured-output-ready` event → `StructuredPanel.tsx` (Paste / Raw / Copy / Edit / Dismiss / dictate-append). No auto-paste; panel is the sole commit point.
- KV cache: persistent session keeps the system-prompt prefix warm (`llm/engine.rs:236-365`); background warm-up on model activation; **5-min idle unload** (`llm/runner.rs:94,119-128`); single-flight worker with busy-rejection (`runner.rs:214-232`).
- Degradation: every failure (timeout 8s, no model, too short, busy) falls back to plain dictation + `structured-mode-degraded` banner. Invariant: **structured mode must never block dictation** (`pipeline.rs:739-741`).

### 1.2 Tuning parameters (single source of truth table)

| Param | Value | Where |
|---|---|---|
| n_ctx | 3072 | `llm/types.rs:37` **AND duplicated** `commands/llm.rs:173` |
| max_tokens | 384 | `llm/types.rs:41` **AND** `commands/llm.rs:174` |
| temperature | 0.0 hardcoded | `llm/types.rs:5-7` |
| timeout | 8 s (setting) | `storage/types.rs:202` |
| min_chars | 40 (setting) | `storage/types.rs:203` |
| input cap | **1600 chars, silent truncation** | `pipeline.rs:825-839` |
| idle unload | 5 min | `runner.rs:94` |
| grammar caps | 9 items / 520 chars | `resources/grammars/slot_extraction_v1.gbnf` |

### 1.3 Known weaknesses (from audit)

1. **Silent 1600-char truncation** — long dictations lose everything past the cap with only a diag-log note. ~2.5 min of speech hits this.
2. **Lazy model load on the hot path** — first structured dictation (and first after idle unload) blocks up to many seconds while a 1.8 GB model loads inside the pipeline (`pipeline.rs:803-823`).
3. **No streaming** — panel appears only when generation completes (`engine.rs:374-421` accumulates a String).
4. **Single persona** — prompt/template are hardcoded for "dictate a prompt to a coding agent" (## Files / Components, ## Expected Behavior). Context modes carry an `llm_prompt` column that is **vestigial** — never consumed by extraction; the General mode even clears it on launch (`storage/context_modes.rs:74-79`).
5. **Config duplication** — `LlmConfig` values hardcoded in two places (drift risk).
6. **Observability requires an env var** (`OMNIVOX_STRUCTURED_MODE_LOG=1`, `llm/diaglog.rs:13-39`) — impossible to ask end users for on a GUI app.
7. TS/Rust type drift: TS `SlotExtraction` in `src/lib/tauri.ts:459-481` omits the `context` slot that Rust renders.
8. Frontend markdown renderer handles only `##`, `- `, `` `code` `` (`StructuredPanel.tsx:489-534`).

---

## 2. Implementation phases

### Phase 1 — Reliability: kill the silent truncation

**Files:** `src-tauri/src/pipeline.rs`, `src-tauri/src/llm/engine.rs`, `src-tauri/src/llm/types.rs`, `src-tauri/src/commands/llm.rs`

- [ ] **Dedupe `LlmConfig`** first: make `commands/llm.rs:167-175` use `LlmConfig::default()` + overrides so n_ctx/max_tokens live in one place. Pure refactor, unblocks the next item.
- [ ] **Raise the input cap by resizing the budget, not chunking.** The models advertise 32,768 ctx; n_ctx=3072 was sized for prompt(≈1900) + 1600 chars + output. Bump n_ctx to 4096–6144 and the cap to ~4000 chars; measure KV-cache memory (n_ctx 3072 already costs hundreds of MB — scale linearly, decide a ceiling per the 1.7B model on an 8 GB machine). Keep the cap; just make it generous.
- [ ] **Surface truncation.** When input exceeds the cap, include a truncation notice in the `structured-output-ready` payload (new optional field) and render a small warning strip in `StructuredPanel` ("Long dictation — last N words weren't structured; use Raw for the full text"). The Raw button already carries the full transcript — point users at it.
- [ ] (Stretch, only if users still hit the cap) Two-pass chunking: extract per chunk, merge slot-wise with the existing dedupe in `schema.rs`. Complexity is real (goal conflicts across chunks); do not build speculatively.

**Verify:** dictate >1600 chars with structured mode on → no silent loss; warning visible; `cargo test llm::`.

### Phase 2 — Latency: model ready when the user is

**Files:** `src-tauri/src/pipeline.rs`, `src-tauri/src/llm/runner.rs`, `src-tauri/src/commands/llm.rs`

- [ ] **Pre-load at enable + app start.** When `structured_mode` turns on (settings save) and at startup when it's already on, kick `load_and_activate_llm` on a background thread (mirror the app-index pre-warm pattern, `commands/settings.rs:246-248`, `lib.rs:462-469`). The warm-up already exists post-load; the gap is *when* load starts.
- [ ] **Re-warm on recording start.** After the 5-min idle unload, the next dictation pays session rebuild + prefill. `start_recording_inner` (`pipeline.rs:192`) knows structured mode is on — kick session rebuild there so it overlaps with the user speaking (typically ≥2 s of cover). Idle-unload logic lives in `runner.rs:119-128`.
- [ ] **Emit a status event** (`llm-status: loading|warming|ready`) so the pill can show why the first structured dictation is slow instead of appearing hung; today `recording-state-change: structuring` is the only signal (`pipeline.rs:845`).
- [ ] (Optional) Make idle-unload duration a constant worth revisiting (5 min ships; original plan said 10) — with re-warm-on-record in place, shorter is fine and saves memory. Leave as-is unless memory pressure complaints exist.

**Verify:** cold-start structured dictation completes without the multi-second stall; log `model-load.log` timings before/after.

### Phase 3 — Quality & breadth: personas beyond the coding agent

The current prompt is excellent *for its one job*. The user goal is a general "structure my dictation" feature: emails, notes, plans — not only agent prompts.

**Files:** `src-tauri/src/llm/prompt.rs`, `llm/template.rs`, `llm/engine.rs`, `storage/context_modes.rs`, `src/features/modes/ContextModesPage.tsx`, `src/features/overlay/StructuredPanel.tsx`

- [ ] **Design a small profile registry** (compile-time, 2–4 profiles max): e.g. `agent-prompt` (current, default), `email`, `notes/outline`. Each profile = system prompt + GBNF grammar + markdown template triple. Grammar-per-profile keeps the anti-hallucination property.
- [ ] **KV-cache interaction is the hard constraint:** the warm prefix is keyed on the byte-identical system prompt (`engine.rs:301-365` keeps the longest common token prefix). Switching profiles invalidates it. Design: keep ONE active profile warmed; switching profiles re-warms in the background (accept one slow extraction after a switch, or block switch on re-warm with UI feedback). Do NOT try to keep multiple sessions warm (n_ctx-sized KV each — memory-prohibitive).
- [ ] **Selection UX:** per context mode — repurpose the vestigial `llm_prompt` column into a `structured_profile` reference (or add a column; migrations are ad-hoc column-existence checks, `storage/database.rs:180-259` — follow that pattern). Mode already syncs style on activation (`commands/context_modes.rs:160-163`); sync profile the same way. Overlay ModeSelector then gives quick switching for free.
- [ ] **Prompt iteration harness:** before shipping new prompts, build the equivalent of `examples/command_ab.rs` for extraction — a labeled set of dictation→expected-slots cases run offline against candidate prompts/models. The A/B harness pattern already exists in-repo; reuse it. Keep `llm/tests.rs`'s normalization tests green.
- [ ] **Few-shot gaps in the current prompt** (cheap wins even without profiles): add one example of a long mixed-intent dictation and one non-coding dictation, re-measure. Any prompt byte change invalidates warm caches on update — that's fine (one re-warm), just don't churn casually.

### Phase 4 — Observability & polish

**Files:** `src-tauri/src/llm/diaglog.rs`, `src/features/models/LlmModelsSection.tsx`, `src/lib/tauri.ts`, `src/features/overlay/StructuredPanel.tsx`

- [ ] **In-app diagnostics**: replace/augment the env-var gate with a "Log structured-mode diagnostics" toggle in the Models → LLM tab, and surface the last N extraction records (duration, input/output size, timeout/degraded reason) in a collapsible panel. The data already exists in `diaglog.rs`; it needs a switch and a reader command.
- [ ] **Fix TS `SlotExtraction`** (`src/lib/tauri.ts:459-481`) to include `context` — currently silently dropped by any frontend consumer that round-trips slots.
- [ ] Extend the mini markdown renderer (`StructuredPanel.tsx:489-534`) only as far as the templates need (numbered lists if a profile emits them; nothing speculative).
- [ ] **Refresh `docs/structured-mode-plan.md`**: add a banner at top marking it historical, pointing here.

### Phase 5 — Model catalog (optional, evidence-driven)

- [ ] Only expand the catalog if the Phase-3 harness shows a candidate beating Qwen3-1.7B Q8 on the labeled set at comparable latency. Candidates to test: Qwen3-4B (quality ceiling, slower), function-calling-tuned small models (the `command_ab.rs` roster: xLAM-2, Hammer2.1). Catalog mechanics are simple (`llm_models/manager.rs:155-184` + HF repo/filename); the cost is validation, not code.
- [ ] If adding models: implement SHA256 verification on download (planned in the original doc Phase 3, never built — `llm_models/downloader.rs`).

---

## 3. Invariants to preserve

1. **Never block or lose a dictation.** Every new failure path must degrade to plain output + `structured-mode-degraded`, like all 12 existing ones (`pipeline.rs:892-928`, `runner.rs:214-239`).
2. **Panel-first, no auto-paste** when structured output exists (`pipeline.rs:1000`).
3. **Anti-fabrication post-processing** (`schema.rs:152-180`) applies to any new profile — grounding beats eloquence.
4. **The frozen-prompt/KV-cache contract**: any code path that varies the system prompt per call silently destroys the caching win (2–7 s → sub-second prefill). Screen-context tokens are already carefully placed in the *user* turn for this reason (`prompt.rs:101-150`).
5. Command Mode classification shares the same loaded model on a throwaway session (`engine.rs:230-233`) — don't regress its latency by hogging the worker; the single-flight busy-rejection (`runner.rs:214-232`) is the arbiter.
