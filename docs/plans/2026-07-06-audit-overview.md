# OmniVox Codebase Audit & Improvement Roadmap — Overview

**Date:** 2026-07-06 · **Audited version:** v0.4.0 (`335287d`, latest release tag) · **Author:** planning session (audit only — no code changes)

> **For agentic workers (Opus implementers): START HERE.** This directory contains the planning output of a full-codebase audit. This file is the map; the four sibling documents are the implementation guides. Do not treat cited line numbers as current — verify before editing. Honor repo `CLAUDE.md` (think before coding, simplicity first, surgical changes, goal-driven execution) and the `.agent-collab/` protocol (claim scopes in `CLAIMS.md` when working in tandem).

---

## 1. The documents

| Doc | Scope | Owner priority |
|---|---|---|
| [`2026-07-06-dictation-formatting.md`](./2026-07-06-dictation-formatting.md) | Bullet/numbered-list dictation and formatting quality in **plain** mode (no LLM); output latency | **High** — stated pain point |
| [`2026-07-06-structured-mode-tuneup.md`](./2026-07-06-structured-mode-tuneup.md) | Structured Mode reliability (truncation), first-use latency, personas beyond coding-agent prompts, observability | **High** |
| [`2026-07-06-command-mode-jarvis.md`](./2026-07-06-command-mode-jarvis.md) | Command Mode → background-capable "Jarvis": safety/control foundations, richer actions, screen awareness, task engine, agent delegation, wake word | **Highest** — the owner's flagship vision |
| [`2026-07-06-quick-wins-tech-debt.md`](./2026-07-06-quick-wins-tech-debt.md) | Bugs (orphaned vocabulary on mode delete, dead theme toggle), dead code, frontend CI/tests gap, pill refactor | Medium — de-risks the above |

## 2. What OmniVox is (30-second orientation)

Windows-first, local-only (zero-cloud) voice app. Tauri 2: Rust backend, React 19 + Tailwind 4 frontend, two isolated WebView windows (main app + always-on-top floating pill overlay). SQLite storage. Three product surfaces:

1. **Dictation** (LCtrl+LAlt push-to-talk): mic → windowed-sinc resample to 16 kHz → optional RNNoise → whisper.cpp (Vulkan GPU optional) → deterministic post-processing (`postprocess/`) → clipboard-verified paste into the focused app (`output/router.rs`). Extras: live preview, screen-context Whisper biasing (UIA text), phonetic vocabulary correction, context modes (per-app auto-switch, style/dictionary/snippets/vocab scoping), voice commands inside dictation ("new line", "send").
2. **Structured Mode** (optional LLM pass): local Qwen3 via llama.cpp turns a dictation into a slotted, grammar-constrained JSON → markdown "prompt for a coding agent", presented in a review panel (never auto-pasted). KV-cache-warmed, timeout-guarded, always degrades to plain text.
3. **Command Mode** (Right-Ctrl push-to-talk, v0.4.0): speak an action — deterministic matcher first, grammar-constrained LLM fallback (multi-step chains ≤5) over a **closed intent enum** (open app / key chords / media / window / web / type-text), with confidence-gated confirm pill for consequential actions.

Key architectural facts implementers keep tripping on:
- The two WebView windows run **isolated JS runtimes** — no shared Zustand stores; coordination is via Tauri events (`lib/tauri.ts` wrappers, emitted mostly from `pipeline.rs`).
- Dictation and Command Mode share one mic + one Whisper engine via a `CaptureMode` mutex claimed synchronously on the keyboard-hook thread (`state.rs`, `hotkey.rs`, `pipeline.rs:46-177`).
- Structured Mode and Command Mode share one loaded LLM; extraction uses a persistent KV-warmed session, command classify a throwaway session; the runner is single-flight with busy-rejection.
- The system prompt being byte-identical across calls is what makes Structured Mode fast (KV prefix reuse) — prompts are not free to vary per call.
- Everything degrades to plain dictation; no feature may block or lose an utterance.

## 3. Cross-cutting audit verdict

**Strengths:** unusually disciplined safety engineering (closed enums, GBNF grammars, all-or-nothing chain parsing, confirm pill, clipboard restore, graceful degradation everywhere); strong Rust unit-test culture for pure logic; thoughtful latency work (KV cache, anti-aliased resampling, preview abort callbacks).

**Systemic weaknesses:**
1. **Plain-mode formatting is a shell** — 4 of 5 list heuristics are dead code, no list voice commands exist, markers get stripped. (→ dictation doc)
2. **Structured Mode has one persona and one silent data-loss path** (1600-char truncation) and loads its model on the hot path. (→ structured doc)
3. **Command Mode shipped the skeleton, not the assistant** — no keyboard confirm/stop/undo, no background/async tasks, no memory, no screen actuation. The research doc's Phase-2/3 items are unbuilt. (→ Jarvis doc)
4. **Frontend has zero CI and zero tests** while carrying real logic (analytics compute, overlay sizing, 1,100-line FloatingPill). (→ tech-debt doc)

## 4. Suggested execution order (across docs)

Independent tracks can run in parallel (different agents, disjoint files) — claim scopes in `.agent-collab/CLAIMS.md`.

1. **Tech-debt §3 first item** — add frontend CI (tsc + build). One session, protects everything after.
2. **Dictation Phase 1** (spoken list commands) — highest user-visible value per effort; touches `postprocess/` + `output/` only.
3. **Structured Phases 1–2** (truncation fix, pre-load/re-warm) — touches `llm/` + `pipeline.rs` structured branch.
4. **Jarvis Phase A** (keyboard confirm, global stop, undo) — prerequisite for all further Jarvis work; touches overlay + `hotkey.rs` + command paths in `pipeline.rs`.
5. Then by appetite: Dictation Phases 2–4, Structured Phase 3 (personas), Jarvis B → D (task engine = the flagship), quick-wins interleaved.

Conflict warning: `pipeline.rs` is the shared artery — steps 2, 3, 4 each touch different regions of it (plain-output branch ~934-1017, structured branch ~735-928, command branch ~1128-1830), but coordinate merges if run concurrently.

## 5. Verification baseline (run before and after any change)

```
cd src-tauri && cargo test          # Rust unit tests (formatter, intent, schema, migrations...)
cd src-tauri && cargo clippy --all-targets
npm run build                        # tsc + vite (frontend type safety — NOT in CI yet)
```

Manual smoke on Windows build: dictate into Notepad (plain), long dictation with Structured Mode on, Right-Ctrl "open notepad and type hello". CI mirrors the Rust half (`.github/workflows/rust-health.yml`); release builds are NSIS via `release.yml` on `v*` tags.
