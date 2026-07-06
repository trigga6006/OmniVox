# Plan: Standard Dictation Formatting — Lists, Bullets, Numbered Items (No LLM)

> **For agentic workers:** This is an implementation guide produced by a codebase audit (2026-07-06, v0.4.0 / commit `335287d`). Verify cited line numbers before editing — they drift. Work phase by phase; each phase is independently shippable. Follow repo `CLAUDE.md` (surgical changes, simplicity first). Run `cargo test` in `src-tauri/` before and after each task.
>
> **STATUS (2026-07-06, branch `claude/audit-implementation`):** Phase 1 (spoken list commands) — DONE (`resolve_list_segments` in `postprocess/voice_commands.rs`, seeded via `seed_missing_builtins`). Phase 2 — DONE with a scope adjustment: standalone ordinal runs stay prose per the product contract pinned in `formatter_tests.rs`; instead, counted headers now honor short dictations and emit numbered lists when items lead with ordinals; dead patterns 4/5/implicit were deleted. Phase 3 — filler-removal toggle DONE; `auto_punctuate` removed as dead. Phase 4 — DONE: Ship-Mode blind 1500ms sleep → 600ms settle after the router's synchronous guard (sized for Electron targets per Codex review); paste-guard audit concluded it stays 250ms (doubles as a paste-ordering barrier — documented at the const); preview drain now measured via diag log before its timeout gets tightened.

**Goal:** Let users dictate structured text — bullet lists, numbered lists, basic layout — in **plain dictation mode**, deterministically, without invoking Structured Mode's LLM. Also tighten general formatting quality and output latency.

**Why:** Today the plain pipeline can only produce prose. There are no voice commands for bullets or numbered items, the automatic list formatter is ~80% dead code, and any markdown the user gets into a transcript is actively stripped.

---

## 1. Current state (audit findings)

### 1.1 Pipeline order (plain mode)

`stop_and_transcribe` in `src-tauri/src/pipeline.rs:545`:

```
Whisper ASR (batch)                          pipeline.rs:684
→ ProcessorChain.process                     pipeline.rs:724, processor.rs:61
    fillers → contextual fillers → phrase dedup
    → phonetic vocab correction → dictionary → snippets
    → capitalization → whitespace → punctuation cleanup → style
→ formatter::format_lists                    pipeline.rs:934-938, formatter.rs:761
→ voice-command parsing (segments)           pipeline.rs:945-967, postprocess/voice_commands.rs:362
→ output router (per-segment paste/keystroke) output/router.rs:108
```

Key ordering fact: **`format_lists` runs BEFORE voice-command parsing.** Anything the formatter emits passes through the voice parser; anything voice commands emit is never seen by the formatter.

### 1.2 What exists today

- **Voice commands** (`postprocess/voice_commands.rs:100` `default_command_table`): `new line` (Shift+Enter), `new paragraph` (Shift+Enter ×2), `delete last word`, `select all`, `copy/cut/undo/redo that`, `press tab/escape/enter`, trailing `send`. Plus opt-in mouse/scroll/window commands and user-defined `key:`/`launch:`/`mouse:` customs in the `custom_voice_commands` table. **No list-related commands at all.**
- **`format_lists`** (`formatter.rs:761`): first strips pre-existing markers (`strip_existing_markers`, `formatter.rs:15` — leading `- * • ·`, `## `, `**bold**`, `1. `/`2)`, inline `. - `), then attempts list detection. Of the 5 documented patterns, **only Pattern 1 (counted header: "these three things: ...") is live.** Dead code:
  - Pattern 3 ordinals: `starts_with_ordinal` hardcoded `false` (`formatter.rs:413-416`)
  - Pattern 4 repeated prefix: `has_nearby_list_cue` hardcoded `false` (`formatter.rs:578-581`)
  - Pattern 5 inline comma list: unconditional `return None` (`formatter.rs:585-588`)
  - Implicit header: `ListHeader::Implicit` + `find_implicit_list_end` are `#[allow(dead_code)]` (`formatter.rs:426, 673`)
  - Gates: ≥40 words (`MIN_WORDS_FOR_LIST`) and ≥4 sentences. Output is always `- ` bullets — **never `1.` numbered**.
- **Numbers:** no spoken-number→digit conversion anywhere. `parse_count` (`formatter.rs:189`) maps "two"–"ten" for header detection only.
- **Processor toggles not user-wired:** `ProcessorConfig.auto_punctuate` exists but is **never read** (`postprocess/types.rs:52`); `auto_capitalize`/`apply_dictionary`/`apply_filler_removal` are hardcoded on (only `writing_style` is user-driven, `state.rs:142-145`). Filler removal always strips `basically/actually/literally` even when meaningful (`processor.rs:294`).
- **Output latency:** every segment paste sleeps `POST_PASTE_GUARD_MS = 250` (`router.rs:22`, used at `:137,:193,:364`); clipboard verify polls up to 750 ms; Ship Mode sleeps a blind 1500 ms then presses Enter (`pipeline.rs:1047-1055`); stopping with live preview on can wait up to 1500 ms for the preview worker (`pipeline.rs:626`).

### 1.3 Structured Mode interplay

When Structured Mode fires, `format_lists` and voice-command parsing are **bypassed entirely** (`pipeline.rs:934, 950`). Plain-mode list work must therefore be self-contained and must not assume the LLM exists.

---

## 2. Design decisions (pre-made, follow unless evidence contradicts)

1. **Explicit voice commands are the primary mechanism** ("bullet point", "next item"), not smarter implicit heuristics. Deterministic, testable, zero false positives. Implicit detection is a later, conservative enhancement.
2. **Resolve list markers at parse time into text, not at execution time.** Numbered lists need a counter; putting state in the output router is wrong. Add a post-parse pass that rewrites list-command segments into text segments (with `\n- ` / `\n1. ` prefixes) and then **merges adjacent text segments**. The router stays stateless, and merging fixes the 250 ms-per-segment latency tax as a side effect.
3. **Markers travel as text, newlines as text.** In TypeSimulation mode the merged text (including `\n`) goes through the existing clipboard-verified paste — one paste instead of N. Note: `new line`/`new paragraph` remain keystroke commands for compatibility (some apps treat pasted `\n` differently from Shift+Enter); list items should use `\n` in the composed text since a pasted multi-line list is the natural unit. If real-world targets misbehave, fall back to per-item Shift+Enter + paste of `- ` marker.
4. **Don't fight `strip_existing_markers`.** It runs before voice parsing, so markers emitted by the new pass are never seen by it. But it WILL see the spoken words "bullet point" — verify it leaves them alone (it strips symbols, not words; add a test).
5. **Casing/punctuation of list items:** after splitting into items, each item should be capitalized and stripped of a stray trailing comma/and — reuse the item-cleaning helpers already in `formatter.rs` (see `clean_list_item` usage in the live Pattern-1 path).

---

## 3. Implementation phases

### Phase 1 — Spoken list commands (core deliverable)

**Files:** `src-tauri/src/postprocess/voice_commands.rs`, `src-tauri/src/output/router.rs`, `src-tauri/src/pipeline.rs`, `src/features/commands/VoiceCommandsPage.tsx` (display only)

- [ ] Add `VoiceCommand` variants (`voice_commands.rs:34`): `BulletItem`, `NumberedItem`, `EndList`. Scope: `Anywhere`.
- [ ] Add default table rows (`voice_commands.rs:100`) with generous phrase aliases (longest-first matching already exists):
  - `BulletItem`: "bullet point", "bullet", "next bullet", "dash point"
  - `NumberedItem`: "number item", "numbered item", "next number", "number point"
  - `EndList`: "end list", "end of list"
  - Check Whisper-transcription reality: test how Whisper actually renders these phrases (it may emit "bullet point," with punctuation — the parser already strips trailing punctuation for EndOfUtterance but Anywhere matching is word-boundary based; verify commas adjacent to matched phrases are cleaned, mirroring the orphaned-punctuation cleanup in `processor.rs:382`).
- [ ] Encode/decode in `action_to_command` / `command_to_action` (`voice_commands.rs:195, 224`) so the DB round-trips them (`custom_voice_commands` table stores actions as strings).
- [ ] **New post-parse pass** (new fn in `voice_commands.rs`, called from `pipeline.rs` right after `parse_commands_with_table`): walk segments, maintain list state (kind, counter), rewrite `BulletItem` → text `"\n- "`, `NumberedItem` → text `"\n{n}. "` (increment per item), `EndList` → text `"\n"` and reset. First item of a list gets a leading `\n` only if preceding text exists. Then merge adjacent `Text` segments into one.
  - Item text between two markers should be trimmed and capitalized; strip a trailing `.`/`,` the punctuation pass may have inserted mid-list if it reads awkwardly (judgment call — test with real dictations).
- [ ] `segments_to_string` (`voice_commands.rs:549`, clipboard-mode collapse) handles the new variants identically (they'll already be text after the pass — make the pass run before both output paths so `segments_to_string` never sees them; keep a defensive arm anyway).
- [ ] Router: no changes needed if the pass fully rewrites to text (verify no `run_command` arm is required; add an unreachable-safe arm returning Ok).
- [ ] Tests in `voice_commands.rs` test module: bullet sequence, numbered sequence with correct 1./2./3., mixed prose→list→prose, "end list" resumes prose, phrase-with-comma robustness, single "bullet point" with no content (should not emit a dangling marker).
- [ ] Frontend: the Commands page (`VoiceCommandsPage.tsx`) lists built-ins from the DB — confirm new built-ins appear with toggles after the seed logic runs (see how existing built-ins seed in `storage/voice_commands.rs`; new built-ins must be inserted for existing installs, not just fresh DBs — check the seeding/migration path).

**Verify:** `cargo test` (new tests), then manual: dictate "shopping list colon bullet point milk bullet point eggs bullet point bread" into Notepad → three `- ` lines.

### Phase 2 — Numbered output + implicit-list revival (conservative)

**Files:** `src-tauri/src/postprocess/formatter.rs`, `formatter_tests.rs`

- [ ] Counted-header pattern (the one live pattern) should emit **numbered** items when the utterance uses ordinal cues ("first... second... third") and bullets otherwise. `ORDINAL_STARTERS` scaffolding exists — revive `starts_with_ordinal` (`formatter.rs:413`) with real logic + tests instead of `false`.
- [ ] Revive Pattern 3 (ordinal sentences → numbered list) behind strict gates: require ≥3 ordinal-led sentences in sequence, strip the ordinal word from the item ("First, buy milk" → "1. Buy milk").
- [ ] Decide per dead pattern: revive with tests, or **delete**. Do not leave `#[allow(dead_code)]` scaffolding. Recommendation from audit: revive Pattern 3 (ordinals — high precision), delete Pattern 4/5 and `Implicit` (low precision, high false-positive risk; explicit voice commands from Phase 1 cover the intent).
- [ ] Reconsider `MIN_WORDS_FOR_LIST = 40` for the counted-header pattern — "I need three things: milk, eggs, bread" is 8 words and currently never formats. A counted header with matching inline enumeration is high-precision at any length.
- [ ] Update the stale doc comment at `formatter.rs:751-758` to describe what actually runs.

**Verify:** `cargo test postprocess::` — existing `formatter_tests.rs` asserts many "stays prose" cases; keep them green (they encode the no-false-positive contract).

### Phase 3 — Formatting quality tune-up

**Files:** `src-tauri/src/postprocess/processor.rs`, `src-tauri/src/storage/types.rs`, `src-tauri/src/commands/settings.rs`, `src/features/settings/SettingsPage.tsx`, `src/lib/tauri.ts`

- [ ] **Wire `auto_punctuate` or remove it.** Field exists (`postprocess/types.rs:52`) but is never read. If keeping: gate `cleanup_punctuation`'s trailing-period insertion + capitalization on it, add an `AppSettings` field + Settings toggle. (Adds a row to the 27-key settings table — follow the pattern in `storage/settings.rs:203-278` and mirror in `lib/tauri.ts:81-133`.)
- [ ] **Expose filler removal** as a setting (`filler_removal: bool`, default true), and make the aggressive adverb strips (`basically`, `actually`, `literally`, `processor.rs:294`) context-aware or move them to a separate "aggressive" tier — they destroy meaning in sentences like "it literally exploded". The `, like,` guard (`processor.rs:434`) is the model to follow.
- [ ] Consider a per-context-mode override for these (modes already carry `writing_style` — same plumbing, `commands/context_modes.rs:160-163`), but only if the global toggle proves insufficient. Don't build both speculatively.

### Phase 4 — Output path latency

**Files:** `src-tauri/src/output/router.rs`, `src-tauri/src/pipeline.rs`

- [ ] Adjacent-text-segment merging (done structurally in Phase 1's pass) — confirm multi-segment dictations now do one paste instead of N; measure.
- [ ] **Ship Mode blind sleep** (`pipeline.rs:1047-1055`, 1500 ms + fresh Enigo): replace with the router's clipboard-verified completion signal + short fixed settle (e.g. 300 ms), reusing the router's Enigo. Keep a conservative floor — the sleep exists because slow apps drop the Enter.
- [ ] Audit the 250 ms `POST_PASTE_GUARD_MS`: needed per *final* paste (clipboard-restore protection), not between segments of the same dictation once merging lands. Reduce inter-segment guard only for keystroke-only segments if any remain.
- [ ] (Optional, measure first) Preview-worker drain timeout 1500 ms (`pipeline.rs:626`): the abort callback (v0.3.1) should make the actual wait short; log the real drain times before touching.

---

## 4. Risks / constraints

- **False positives are the cardinal sin.** A user saying "the bullet point of the memo" mid-prose must not trigger a list. `Anywhere` scope + common words is the tension — phrase choice ("bullet point" is rarely prose; bare "bullet" is riskier, consider EndOfSentence-adjacent gating or dropping bare "bullet") and the per-command enable toggles on the Commands page are the mitigations. When in doubt, ship fewer aliases.
- **Whisper renders spoken phrases unpredictably** ("bullet point." / "Bullet point,"). Test with real audio; the phonetic-alias approach used for "Voxify" (`llm/voxify.rs:36-49`) shows the shape of the problem.
- **App compatibility for pasted `\n`:** most editors accept it; chat apps (Slack, Discord) treat pasted newlines fine but Enter sends — pasting is safer than keystrokes here. Verify in: Notepad, VS Code, Word, Slack, browser textarea.
- **Don't regress the marker-stripping defense** — it protects against Whisper hallucinating markdown. Strip-then-reformat stays; only the new pass's own output must be exempt (guaranteed by ordering).
