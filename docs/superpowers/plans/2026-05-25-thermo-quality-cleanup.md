# Thermo Quality Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Apply the thermo-nuclear audit findings without changing OmniVox behavior.

**Architecture:** Keep the root `src/` and `src-tauri/` trees canonical. Split large frontend/backend files by ownership: state orchestration, presentation, static data, and tests live in separate focused modules.

**Tech Stack:** React 19, TypeScript, Vite, Tauri 2, Rust.

---

### Task 1: Baseline and Duplicate Tree

**Files:**
- Delete: `OmniVoice/**`
- Verify: `git ls-files OmniVoice`

- [ ] Run `npm run build` and `cargo test` from `src-tauri` to record baseline.
- [ ] Remove the tracked nested `OmniVoice/` app copy.
- [ ] Verify `git ls-files OmniVoice` returns no tracked files.

### Task 2: Formatter Decomposition

**Files:**
- Modify: `src-tauri/src/postprocess/formatter.rs`
- Create: `src-tauri/src/postprocess/formatter_tests.rs`
- Modify: `src-tauri/src/postprocess/mod.rs`

- [ ] Move formatter tests out of the production formatter file.
- [ ] Keep test module compiled only under `#[cfg(test)]`.
- [ ] Run formatter-related Rust tests.

### Task 3: Context Mode Seed Data

**Files:**
- Modify: `src-tauri/src/storage/context_modes.rs`
- Create: `src-tauri/src/storage/context_mode_seed_data.rs`
- Modify: `src-tauri/src/storage/mod.rs`

- [ ] Move builtin additions, dictionary entries, and snippet entries into typed seed definitions.
- [ ] Replace repeated seed functions with one generic seeding helper.
- [ ] Run storage/context-mode tests or full Rust tests.

### Task 4: Shared Settings Patch Flow

**Files:**
- Create: `src/hooks/useSettingsPatch.ts`
- Modify: `src/features/settings/SettingsPage.tsx`
- Modify: `src/features/overlay/FloatingPill.tsx`

- [ ] Introduce a shared optimistic settings patch hook with rollback.
- [ ] Use it from Settings and overlay toggles.
- [ ] Run TypeScript build.

### Task 5: Overlay Decomposition

**Files:**
- Modify: `src/features/overlay/FloatingPill.tsx`
- Create: focused overlay hooks/components/styles as needed.
- Modify: `src/features/overlay/StructuredPanel.tsx`
- Create: `src/features/overlay/StructuredPanel.css`

- [ ] Extract overlay sizing/event coordination into hooks.
- [ ] Move embedded CSS out of TSX files.
- [ ] Keep public UI behavior and Tauri event names unchanged.
- [ ] Run TypeScript build.

### Task 6: Pipeline Decomposition

**Files:**
- Modify: `src-tauri/src/pipeline.rs`
- Create: `src-tauri/src/pipeline/*.rs` modules or focused sibling modules as needed.

- [ ] Extract start-recording setup, preview worker, transcription, structured mode, and output finalization into focused helpers.
- [ ] Preserve command signatures and emitted event names.
- [ ] Run Rust tests and frontend build.

### Task 7: Final Verification

- [ ] Verify no owned source file crosses 1k lines unless justified.
- [ ] Run `npm run build`.
- [ ] Run `cargo test` in `src-tauri`.
- [ ] Review `git diff --stat` and summarize behavior-preserving changes.
