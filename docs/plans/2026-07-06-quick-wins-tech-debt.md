# Plan: Quick Wins & Tech Debt

> **For agentic workers:** Cleanup backlog from the 2026-07-06 audit (v0.4.0 / commit `335287d`). Items are independent unless noted — cherry-pick. Verify cited lines first. Per repo `CLAUDE.md`: surgical changes only; don't bundle unrelated fixes in one commit.

These are not features. They are correctness fixes, dead-code removals, and infrastructure gaps that make the three feature plans (dictation formatting, structured mode, Jarvis) safer to execute.

---

## 1. Correctness bugs

- [ ] **Mode deletion orphans vocabulary.** Deleting a context mode cascades `dictionary_entries` + `snippets` in a transaction but **not** `vocabulary_entries` (`storage/context_modes.rs:215-222`) — mode-scoped vocabulary rows leak and keep affecting phonetic correction. Add vocabulary to the cascade + a migration to purge existing orphans + a test beside the migration test (`storage/database.rs:262-315`).
- [ ] **Theme setting does nothing in the main window.** `handleThemeChange` persists the setting (`SettingsPage.tsx:263`) but nothing applies it to `document.documentElement` in the main window (overlay hard-forces dark, `FloatingPill.tsx:193`). Either wire light theme properly or remove the toggle — a visible setting that no-ops erodes trust.
- [ ] **TS `SlotExtraction` missing `context`** (`src/lib/tauri.ts:459-481`) vs Rust schema (`llm/schema.rs:14`) — frontend consumers silently drop the slot.
- [ ] **`noise_reduction` default mismatch:** Rust default `false` (`storage/types.rs:190`) vs pill local state `true` (`FloatingPill.tsx:74`) — wrong toggle shown until settings load.
- [ ] **Idle-pill geometry has three sources of truth:** `useOverlaySizing.ts:8-20` (62×26), `FloatingPill.tsx:909` (`w-[148px]` vs ACTIVE_W 156), `recover_overlay` (56×26, `commands/settings.rs:340-343`). Consolidate into shared constants (one TS module + matching Rust consts with a comment pointing at each other).
- [ ] **History auto-refresh race:** 300 ms `setTimeout` after `transcription-result` (`HistoryPage.tsx:66-72`) races pagination offset. Refresh on the event with current offset captured, no timer.

## 2. Dead / vestigial code (remove or finish)

- [ ] `settingsStore.ts` — duplicate settings shape, one writer, no reactive consumer. Remove (fold theme handling into the real `AppSettings` flow) — but do the theme fix above first or together.
- [ ] `recordingStore.error` / `setError` — never called (`recordingStore.ts:20,35-42`). Remove.
- [ ] Dead settings: `language`, `sample_rate` (UI hardcodes "16,000 Hz", `SettingsPage.tsx:336`), `auto_start` (no UI/consumer), `minimize_to_tray` (close-to-tray is unconditional, `lib.rs:486-491`). Decide each: implement or delete from `AppSettings`/UI/defaults. Deleting keys: the settings table tolerates missing keys via per-key defaults (`storage/settings.rs:10-186`), so removal is safe.
- [ ] Dead formatter heuristics — handled in the dictation-formatting plan (Phase 2); don't double-touch.
- [ ] Vestigial `context_modes.llm_prompt` column — reserved by the structured-mode plan (Phase 3) for profiles; don't delete.
- [ ] `docs/structured-mode-plan.md` — stale ("not yet implemented", FunctionGemma, `output_format` slots). Add a historical-doc banner pointing at the current plans directory.

## 3. Frontend infrastructure (biggest systemic gap)

- [ ] **CI has zero frontend coverage.** `rust-health.yml` runs cargo test/clippy/audit/deny/machete; nothing runs `tsc`, eslint, or `npm run build`. Add a frontend job (Node setup + `npm ci` + `tsc --noEmit` + `vite build`). This is the single highest-leverage infra item — TS type drift (like `SlotExtraction`) currently ships silently.
- [ ] **Zero frontend tests.** Add vitest with a first target of `src/features/analytics/compute.ts` (streaks/sessions/WPM — pure functions, non-trivial) and `useOverlaySizing` timing logic. Keep scope tight; don't chase coverage numbers.
- [ ] **`FloatingPill.tsx` is ~1,100 lines / ~20 useStates** holding every overlay surface. Before Jarvis work adds task strips and confirm variants to the pill (see Jarvis plan phases A/D), split it: pill body, quick-toggle column, event-wiring hook, per-state content components. Mechanical refactor, no behavior change, protected by manual overlay smoke-test (`design-lab/pill-lab.html` exists for visual reference).
- [ ] **Duplicated settings-sync blocks** in `FloatingPill.tsx:234-267` and `SettingsPage.tsx:132-170` — extract a shared hook (`useSettingsSync`) so new settings (several planned in the feature docs) get one wiring point.

## 4. Backend hygiene

- [ ] **`LlmConfig` duplication** (`llm/types.rs:37,41` vs `commands/llm.rs:167-175`) — covered in structured-mode plan Phase 1; do it there.
- [ ] **Migrations:** `PRAGMA user_version = 3` is written but never read (`storage/database.rs:135`); migrations are column-existence probes. Fine at current scale — adopt versioned migrations only when a destructive/data migration first appears (several plans add columns; probes still work for those).
- [ ] **Settings I/O:** `get_settings` full-table-scan per call; `update_settings` rewrites all 27 keys per toggle (`storage/settings.rs:203-278`). Not hot enough to matter yet (pipeline snapshots once per recording) — leave unless the settings count grows past ~40; then add an in-memory cache invalidated by `settings-changed`.
- [ ] **Hardcoded 48 px taskbar** in overlay positioning (`lib.rs:72`, `commands/settings.rs:8`) — breaks on auto-hide/scaled taskbars. Use `SHAppBarMessage`/work-area (`SystemParametersInfo SPI_GETWORKAREA`) instead.
- [ ] **History search uses `LIKE %q%`** — adopt SQLite FTS5 on `transcriptions.text` when history search feels slow; not before.
- [ ] **Crate naming:** cargo crate is `omnivoice`/`omnivoice_lib` vs product OmniVox (`src-tauri/Cargo.toml:2,10`). Rename is churny (CI cache keys, bundle identifiers) — do it only bundled with a release-infra touch, or explicitly ignore.
- [ ] **`examples/llm_probe.rs:6`** hardcodes a dev-machine path — parameterize via env/arg.

## 5. UX rough edges

- [ ] `command_mode` toggle lives on the Models page (`CommandModeSection`), not Settings — users look in Settings. Add it to Settings (keep Models placement too if it carries model-specific config).
- [ ] Settings voice-command reference modal hardcodes 4 commands (`SettingsPage.tsx:605-609`) while the real registry is the user-editable Commands page — link to the Commands page instead of a second source of truth.
- [ ] Dictation page fires 3 redundant `getSettings` IPC calls on mount (`DictationPanel.tsx:22,285,368`) — fetch once, pass down.
- [ ] Overlay window created at active size then shrunk by JS → startup flash (`lib.rs:92-104`, `FloatingPill.tsx:203`) — create at idle size.
- [ ] `currentPage` not persisted (`appStore.ts:3`) — restore last page on launch (tiny, nice).
