# OmniVox Structured Mode — Implementation Plan (v1)

**Status:** proposal, not yet implemented. To be reviewed by user + Codex before any code change.

**Branch:** work will land on `feat/structured-mode`, which will be branched from `perf/audit-fixes` (contains the audit fixes already completed and not yet merged to `main`).

**Goal:** add an opt-in mode that turns the output of `pipeline::stop_and_transcribe` from "cleaned prose" into "slot-filled Markdown agent prompt" using a local LLM (FunctionGemma-270M by default). No free-form generation — the LLM is constrained by a GBNF grammar to only emit the JSON slot object, and OmniVox code assembles the final Markdown from those slots.

---

## Context: why this plan exists

OmniVox today is a local dictation app: hotkey → Whisper → deterministic post-processor (filler removal, dictionary replacement, list formatting, voice commands) → clipboard/keystroke output. The user wants to move up a layer: speech → **polished agent prompt** for tools like Claude Code / Cursor / ChatGPT / Codex.

A previous attempt at "AI cleanup" failed in two ways:
1. **Latency** — took up to 5 s per dictation.
2. **Formatting mangling** — the LLM rewrote prose as word-by-word bullet lists, destroyed natural sentence structure.

This plan's core insight: **the LLM must never emit prose**. It is grammar-constrained (GBNF) to emit only a flat JSON object with structured slots (`goal`, `constraints`, `files`, etc.). OmniVox code then deterministically assembles Markdown from those slots. The LLM cannot "creatively rewrite" anything — it can only slot-fill.

---

## 1. Architecture

### 1.1 New module tree — mirror `asr/`

New directory `src-tauri/src/llm/` parallel to `asr/`:

```
src-tauri/src/llm/
├── mod.rs              // re-exports
├── types.rs            // LlmConfig, SlotExtraction, LlmInferenceResult
├── engine.rs           // LlmEngine trait + LlamaEngine (llama-cpp-2 impl)
├── grammar.rs          // GBNF loader / embedded grammar strings
├── prompt.rs           // system prompt builder, Gemma chat template
├── schema.rs           // serde types matching the GBNF output shape
└── template.rs         // Markdown renderer (slots → final paste text)
```

Rationale: Mirroring `asr/engine.rs`'s `AsrEngine` trait gives us the same testability pattern. A `MockLlmEngine` that returns a canned `SlotExtraction` is trivial to build for integration tests of `pipeline::stop_and_transcribe`.

### 1.2 Model catalog — **do not share `ModelManager`**

Create a parallel module `src-tauri/src/llm_models/`:

```
src-tauri/src/llm_models/
├── mod.rs
├── types.rs      // LlmModelInfo, LlmDownloadProgress
├── manager.rs    // LlmModelManager (mirrors ModelManager)
└── downloader.rs // LlmModelDownloader (mirrors ModelDownloader)
```

Why parallel instead of sharing:
- `ModelManager`'s `catalog()` and `recommend_for_cores()` are specific to Whisper tiers. Forcing a union type (`ModelKind::Whisper | ModelKind::Llm`) would infect every downstream call site (`load_and_activate_model`, `invalidate_cache`, every frontend type).
- `model_filename()` and `model_url()` are hardcoded Whisper filename maps. The LLM side needs different repos (huggingface `ggml-org/functiongemma-270m-it-GGUF` vs `ggerganov/whisper.cpp`).
- Events differ: `download-progress` for Whisper must stay unchanged for backward compat; LLM download should be its own channel (`llm-download-progress`).
- Duplication is ~80 lines of catalog code; shared abstraction would cost more than it saves. Keep them independent for now; revisit a shared `Downloader<T>` generic only if we add a third catalog.

### 1.3 Pipeline integration — new sequence

Current `stop_and_transcribe` flow:

```
stop → denoise → normalize → whisper.transcribe → processor.process →
  formatter::format_lists → voice_commands::parse → output.send → history
```

Structured Mode flow (when `settings.structured_mode == true` AND an LLM engine is loaded AND the transcript is non-empty AND length ≥ `structured_min_chars`):

```
stop → denoise → normalize → whisper.transcribe → processor.process →
  [emit "structuring"] → llm.extract_slots(text, system_prompt, grammar) →
  template::render(slots) →
  [emit "structured-output-ready"] → history.save(markdown, raw=text)
```

Skips `format_lists` and voice command detection entirely — the LLM is doing the structuring; deterministic list formatting and voice commands would double-handle.

Important product decision for v1: **Structured Mode is panel-first, not auto-paste**. The success path stops at `structured-output-ready`; the user can review / edit / paste from the panel. That keeps Structured Mode aligned with the UX described in Section 7 and avoids a confusing "already pasted, but also editable" split-brain flow.

Fallback: if `llm.extract_slots` errors OR exceeds timeout, fall back to the existing path (`format_lists` + voice commands + output) using the already-processed text. The user is **never blocked** by LLM failure — worst case Structured Mode degrades to normal dictation for that utterance, and we emit a `structured-mode-degraded` event so the UI can say "LLM unavailable - used plain text".

### 1.4 AppState additions

In `src-tauri/src/state.rs`:

```rust
pub struct AppState {
    // ... existing fields ...

    /// Local LLM engine for Structured Mode. None until a model is loaded.
    pub llm_engine: Mutex<Option<Arc<crate::llm::engine::LlamaEngine>>>,
    /// ID of the currently active LLM model (key into LlmModelManager catalog).
    pub active_llm_model_id: Mutex<Option<String>>,
    /// LLM model catalog + download status.
    pub llm_model_manager: crate::llm_models::manager::LlmModelManager,
    /// Streaming LLM downloader.
    pub llm_downloader: crate::llm_models::downloader::LlmModelDownloader,
    /// Directory for LLM GGUFs (sibling to models_dir): <data_dir>/llm_models.
    pub llm_models_dir: PathBuf,
}
```

### 1.5 Concurrency

`LlamaEngine::extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction>` is CPU-bound (or GPU-bound via Vulkan) and blocking inside llama.cpp's native loop. One important correction to the first draft: **do not call it directly via `spawn_blocking` and wrap that in `tokio::time::timeout`**. Timing out the future does not cancel the native llama.cpp loop; the work would keep running in the background, and repeated timeouts could stack up hidden in the blocking pool.

Use a dedicated one-flight worker instead:

```rust
struct LlmRequest {
    text: String,
    reply_tx: tokio::sync::oneshot::Sender<AppResult<SlotExtraction>>,
}
```

- Spawn one dedicated `std::thread` that owns the loaded `LlamaEngine`.
- Feed it through a `sync_channel(1)` (or equivalent bounded queue).
- `pipeline::stop_and_transcribe` sends exactly one request and awaits the oneshot reply with a timeout.
- If the queue is full, immediately degrade to plain dictation instead of queueing more inference.
- If the timeout elapses, drop the reply receiver, emit `structured-mode-degraded`, and continue with plain output.
- The worker is still allowed to finish its in-flight extraction, but because the queue is bounded to one and the response receiver is dropped, timed-out work cannot pile up or surface stale results later.

The model is loaded (like Whisper) on a dedicated `std::thread::Builder::new().stack_size(256 MB)` thread — llama.cpp has the same huge-stack-frame problem in debug builds.

---

## 2. Dependencies (Cargo.toml)

Add to `src-tauri/Cargo.toml`:

```toml
llama-cpp-2 = { version = "0.1.126", default-features = false }
# llama-cpp-sys-2 is pulled transitively — we never reference it directly
```

**Feature flags** — extend existing `vulkan` / `cuda`:

```toml
[features]
default = []
cuda = ["whisper-rs/cuda", "llama-cpp-2/cuda"]
vulkan = ["whisper-rs/vulkan", "llama-cpp-2/vulkan"]
# Optional: CPU-only Structured Mode (no GPU) is the default with no feature flag
```

Rationale: one flag for both backends keeps distribution simple (one Vulkan build, not two). On the OmniVox Windows installer users already get the `vulkan` feature — llama.cpp's Vulkan shader support is mature enough that we get the same kind of 3-5× GPU speedup as Whisper.

Debug-build stack frame fix (mirror existing pattern):

```toml
[profile.dev.package.llama-cpp-sys-2]
opt-level = 2
```

**Toolchain check:** `llama-cpp-sys-2` needs CMake + a C++17 compiler. `whisper-rs-sys` already needs the same, so no new toolchain requirements. On Windows: MSVC Build Tools (already required). On macOS: Xcode CLT (already required). On Linux: cmake + g++ (already required).

---

## 3. Model lifecycle

### 3.1 Bundling recommendation — **download on first enable, do not bundle**

Arguments to bundle FunctionGemma (292 MB Q8):
- Zero-friction first use.
- Matches Whisper medium-en bundling (user expects "it just works").

Arguments against bundling (winning argument):
- Whisper medium-en is load-bearing — nothing works without it. FunctionGemma is opt-in — 90%+ of first-launch users won't enable Structured Mode in session 1.
- Installer goes from ~1.7 GB to ~2.0 GB. Significant enough to hurt download/install UX.
- A Q4_K_M quant (~180 MB) downloaded on toggle-on is fast on broadband (~10-20 s) and shows a progress bar the user expects.

**Recommendation:** do NOT bundle. On first enable of Structured Mode, if no LLM is downloaded, show an inline "Download model (180 MB)" button in the Settings toggle. Auto-start the download, enable the mode when download completes. Small friction that clearly communicates what's happening.

### 3.2 Model catalog struct

In `src-tauri/src/llm_models/types.rs`:

```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LlmModelInfo {
    pub id: String,                 // "functiongemma-270m-it-q4"
    pub name: String,               // "FunctionGemma 270M (Q4)"
    pub size_bytes: u64,
    pub quantization: String,       // "Q4_K_M", "Q8_0", ...
    pub context_length: u32,        // 32768 for FunctionGemma
    pub description: String,
    pub huggingface_repo: String,   // "ggml-org/functiongemma-270m-it-GGUF"
    pub huggingface_file: String,   // "functiongemma-270m-it-Q4_K_M.gguf"
    pub is_downloaded: bool,
    pub path: Option<String>,
    pub is_default: bool,           // true for the recommended starter
}
```

Starter catalog (Phase 1):

| id | quant | size | notes |
|---|---|---|---|
| `functiongemma-270m-it-q4` | Q4_K_M | ~180 MB | **default** — fast, fits anywhere |
| `functiongemma-270m-it-q8` | Q8_0 | ~292 MB | slightly better structure adherence |
| `qwen3-0.6b-instruct-q4` | Q4_K_M | ~400 MB | fallback if FunctionGemma misbehaves |
| `qwen3-1.7b-instruct-q4` | Q4_K_M | ~1 GB | power user, 16 GB RAM+ |

### 3.3 Hot-swap

`set_active_llm_model(model_id)` command mirrors `set_active_model`:
1. Load new model on a 256 MB stack thread with `panic::catch_unwind`.
2. On success, atomically swap `state.llm_engine` (drop old Arc → `llama_model` frees on last reference).
3. On failure, leave existing engine in place and surface a user-visible error.

### 3.4 Unload policy

Policy: unload LLM engine when Structured Mode is turned OFF, or after 10 min idle (no structured inferences), whichever comes first. Rationale: FunctionGemma 270M Q4 is only ~180 MB in RAM — not aggressive to keep. But users who disable the feature should reclaim the RAM cleanly.

Implementation: a single `tokio::spawn` idle-timer task armed whenever an inference completes; fires `*state.llm_engine.lock().unwrap() = None` if no new inference in 10 min. Re-loaded lazily on next Structured Mode invocation.

---

## 4. Grammar & prompt design

### 4.1 GBNF grammar (v1)

File `src-tauri/resources/grammars/slot_extraction_v1.gbnf`, embedded via `include_str!`:

```
root        ::= "{" ws "\"goal\"" ws ":" ws string
                ( ws "," ws "\"constraints\"" ws ":" ws string-array )?
                ( ws "," ws "\"files\"" ws ":" ws string-array )?
                ( ws "," ws "\"output_format\"" ws ":" ws string )?
                ( ws "," ws "\"urgency\"" ws ":" ws urgency-val )?
                ( ws "," ws "\"follow_up_tasks\"" ws ":" ws string-array )?
                ws "}"

string-array ::= "[" ws ( string ( ws "," ws string )* )? ws "]"
string      ::= "\"" char* "\""
char        ::= [^"\\] | "\\" ["\\/bfnrt] | "\\u" [0-9a-fA-F]{4}
urgency-val ::= "\"low\"" | "\"normal\"" | "\"high\""
ws          ::= [ \t\n]*
```

Properties:
- `goal` is the only required field. Everything else is optional and only emitted if the user actually mentioned it.
- `urgency` is an enum — cannot hallucinate novel values.
- `output_format` is a free string but will be post-validated against a known set (`commit_message`, `pr_description`, `plain_explanation`, `bug_report`, `feature_request`) and falls through to free text if no match.
- No nested objects. Flat structure prevents creative JSON structures.

### 4.2 System prompt (exact content)

In `src-tauri/src/llm/prompt.rs`:

```
You extract structured fields from a user's spoken dictation.

RULES:
1. Do NOT rephrase, summarize, or expand the user's words.
2. Copy slot values verbatim or with only the smallest edits for grammar.
3. Only include a slot if the user actually mentioned that information.
   Omit the key entirely when absent.
4. "goal" is required — it is the primary thing the user wants done.
5. Return only JSON. No prose before or after.

EXAMPLE INPUT:
"Hey can you refactor the checkout flow in billing.tsx and
cart.tsx. Keep the existing Stripe integration intact. This is
pretty urgent, ideally today."

EXAMPLE OUTPUT:
{"goal":"Refactor the checkout flow","files":["billing.tsx","cart.tsx"],"constraints":["Keep the existing Stripe integration intact"],"urgency":"high"}
```

Formatted with Gemma 3 chat template:

```
<start_of_turn>user
<SYSTEM PROMPT ABOVE>

INPUT:
{processed_transcript}<end_of_turn>
<start_of_turn>model
```

### 4.3 Transcript handling

The LLM sees the transcript **after** `processor.process` (filler removal, dictionary replacement, casing) but **before** `format_lists` — the dictionary replacements are the user's preferred spellings, so we want the LLM to see those. We do not want bullet-list formatting in the input because it would bias the LLM to emit structured lists even when the user was just speaking a simple goal.

### 4.4 Output parsing

`schema.rs`:

```rust
#[derive(Deserialize, Debug, Clone)]
pub struct SlotExtraction {
    pub goal: String,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub files: Vec<String>,
    pub output_format: Option<String>,
    pub urgency: Option<Urgency>,
    #[serde(default)]
    pub follow_up_tasks: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(rename_all = "lowercase")]
pub enum Urgency { Low, Normal, High }
```

Parse with `serde_json::from_str`. On error (should be impossible given GBNF, but defense-in-depth): log, emit `structured-mode-degraded`, fall through to deterministic pipeline.

---

## 5. Markdown template (v1, target-agnostic)

`src-tauri/src/llm/template.rs`:

```rust
pub fn render_markdown(s: &SlotExtraction) -> String {
    let mut out = String::new();
    out.push_str("## Goal\n");
    out.push_str(s.goal.trim());
    out.push('\n');

    if !s.constraints.is_empty() {
        out.push_str("\n## Constraints\n");
        for c in &s.constraints { out.push_str("- "); out.push_str(c.trim()); out.push('\n'); }
    }
    if !s.files.is_empty() {
        out.push_str("\n## Files / Components\n");
        for f in &s.files { out.push_str("- `"); out.push_str(f.trim()); out.push_str("`\n"); }
    }
    if let Some(fmt) = &s.output_format {
        out.push_str("\n## Output Format\n");
        out.push_str(fmt.trim()); out.push('\n');
    }
    if let Some(u) = s.urgency {
        out.push_str("\n## Urgency\n");
        out.push_str(match u { Urgency::Low=>"low", Urgency::Normal=>"normal", Urgency::High=>"high" });
        out.push('\n');
    }
    if !s.follow_up_tasks.is_empty() {
        out.push_str("\n## Follow-up\n");
        for t in &s.follow_up_tasks { out.push_str("- "); out.push_str(t.trim()); out.push('\n'); }
    }
    out
}
```

Empty slots produce NO section header (not "N/A"). `format_lists` is bypassed — this Markdown is the canonical output.

Example input → output:

Input (post-processor): `Refactor the checkout flow in billing.tsx and cart.tsx. Keep the Stripe integration. Urgent.`

Output:
```
## Goal
Refactor the checkout flow

## Constraints
- Keep the Stripe integration

## Files / Components
- `billing.tsx`
- `cart.tsx`

## Urgency
high
```

---

## 6. Pipeline integration (concrete diff)

### 6.1 New AppSettings fields (add to `storage/types.rs`)

```rust
/// When true, dictation is run through the local LLM for slot extraction
/// and output as a structured Markdown prompt instead of plain prose.
pub structured_mode: bool,
/// ID of the LLM model to use for structured extraction (see LlmModelManager).
pub active_llm_model_id: Option<String>,
/// Max seconds to wait for LLM inference before falling back to plain output.
pub llm_timeout_secs: u32,
/// Minimum transcript chars before Structured Mode engages (shorter utterances
/// fall through to plain dictation — not enough content to structure).
pub structured_min_chars: u32,
```

Defaults: `structured_mode=false`, `active_llm_model_id=None`, `llm_timeout_secs=8`, `structured_min_chars=40`.

Update `storage/settings.rs::get_settings`/`update_settings` to read/write all four (pairs array grows from 19 to 23). Update `onSettingsChanged` frontend type.

### 6.2 Pipeline sequence (inside `stop_and_transcribe`)

After the existing `processed_text` assignment, insert:

```rust
// --- Structured Mode branch ------------------------------------------------
let structured_enabled = settings.as_ref().map(|s| s.structured_mode).unwrap_or(false);
let min_chars = settings.as_ref().map(|s| s.structured_min_chars).unwrap_or(40) as usize;
let llm_timeout = settings.as_ref().map(|s| s.llm_timeout_secs).unwrap_or(8);

let structured_markdown: Option<String> =
    if structured_enabled && processed_text.chars().count() >= min_chars {
        let _ = app_handle.emit("recording-state-change", "structuring");
        match state
            .llm_runner
            .extract_with_timeout(processed_text.clone(), Duration::from_secs(llm_timeout as u64))
            .await
        {
            Ok(slots) => {
                let md = crate::llm::template::render_markdown(&slots);
                let _ = app_handle.emit("structured-output-ready", &StructuredOutputPayload {
                    markdown: md.clone(),
                    slots,
                    raw_transcript: processed_text.clone(),
                });
                Some(md)
            }
            Err(e) => {
                eprintln!("LLM structured extraction failed: {e}");
                let _ = app_handle.emit("structured-mode-degraded", &e.to_string());
                None
            }
        }
    } else {
        None
    };
// --- end Structured Mode branch -------------------------------------------
```

Then branch:

```rust
let final_text = if let Some(md) = structured_markdown {
    md
} else {
    let formatted = crate::postprocess::formatter::format_lists(&processed_text);
    formatted
};
```

Voice command segmentation becomes: `let voice_segments = if voice_commands_enabled && structured_markdown.is_none() { ... } else { None };`

Output routing rule:
- If `structured_markdown.is_none()`, keep the existing output path unchanged.
- If `structured_markdown.is_some()`, **do not auto-call `output.send` in v1**. The panel is now the commit point for Paste / Copy / Edit / Dismiss.
- History still saves the structured Markdown immediately so the result is not lost if the panel is dismissed.

### 6.3 New events

| Event | Payload | Fired from |
|---|---|---|
| `recording-state-change` → `"structuring"` | `"structuring"` string | pipeline, between processing and idle |
| `structured-output-ready` | `{ markdown, slots, raw_transcript }` | pipeline, on successful extraction |
| `structured-mode-degraded` | string reason | pipeline, on LLM failure/timeout |
| `llm-download-progress` | `LlmDownloadProgress` | `LlmModelDownloader` |
| `llm-model-loaded` | `modelId` | `set_active_llm_model` command |

History saves `final_text` (the Markdown if structured, plain otherwise) AND a new `raw_transcript` column so the user can always recover the original dictation.

Migration detail to make this safe on existing installs:
- Add `raw_transcript TEXT NULL` first; do **not** make it `NOT NULL` in the initial migration.
- For pre-migration rows, treat `raw_transcript IS NULL` as "same as `text`" in the read path.
- Only tighten the constraint later if we ever do an explicit backfill migration.

---

## 7. UI — expanded pill panel

### 7.1 New pill state

Add `"structuring"` to `RecordingStatus` type in `src/stores/recordingStore.ts`. Treat as visually similar to `processing` but with a distinct icon (e.g., `Sparkles` from lucide) and tint (violet/indigo, not amber — signals "different pipeline").

In `FloatingPill.tsx`, add branch in the status-to-visual map: same pill dimensions as `processing`, different label "Structuring…", different dot color.

### 7.2 Structured output panel

Triggered by `structured-output-ready` event. New component `src/features/overlay/StructuredPanel.tsx`.

This panel is the only success-path output surface for Structured Mode v1. The pipeline does not auto-paste structured Markdown before the panel appears.

Dimensions:
- Width: 420 px (vs 210 px active pill)
- Height: variable, min 160 / max 360 based on content

Uses existing `resize_overlay` command — same mechanism ModeSelector uses.

Layout (top to bottom):
1. Header row: icon + "Structured" label, close X. 24 px.
2. Rendered Markdown preview (scrollable). Uses a lightweight react-markdown or custom renderer (no heavy deps). Shows sections as formatted headings + lists.
3. Collapsed "View raw" disclosure — shows original transcript on click.
4. Action row (bottom, pinned): [Paste] [Copy] [Edit] [Dismiss].
   - **Paste:** invoke a new `paste_structured_output` command that runs `output.send` on the current Markdown (respecting current `OutputConfig`). Auto-close on success.
   - **Copy:** write Markdown to clipboard via `arboard`, show "Copied" flash for 800 ms, keep panel open.
   - **Edit:** opens a textarea over the Markdown view so the user can tweak before pasting. On save, panel's local Markdown updates and Paste uses the edited version.
   - **Dismiss:** close panel, do not paste. History still has the record.

Persistence: panel stays open until user acts. Auto-close on Paste success. Auto-close on ESC. If user starts a new recording while panel open, the panel closes immediately (pill goes into recording state).

Hotkey to paste directly: Ctrl/Cmd+Enter inside the panel triggers Paste.

### 7.3 Coexistence with ModeSelector / ShipPopup

- ModeSelector opens above the pill via `w-screen h-screen flex flex-col justify-end`. StructuredPanel uses the same container but takes priority: if a structured result arrives while ModeSelector is open, close ModeSelector first, then open panel.
- Ghost mode: if enabled and a structured result arrives, keep ghost (do not force reveal). User can Ctrl+click hotkey to reveal. Reasoning: respecting user's "hide me" intent is more important than showing the panel. The history still records it, and the user can open the panel later from the main window.

---

## 8. Settings page additions

New section in `src/features/settings/SettingsPage.tsx`, titled "Structured Mode" with a subhead explaining "Turn dictation into polished agent prompts with a local LLM".

Controls:

| Control | Type | Notes |
|---|---|---|
| Structured Mode | Toggle | Big, prominent. Disabled with explanation if no LLM model downloaded. Toggling on when no model → inline download prompt (see §3.1). |
| Active LLM model | Dropdown | Lists downloaded LLM models + "Download more..." opens a subpanel mirroring Models page. |
| Min characters | Slider 20–200, default 40 | Below threshold → plain dictation. |
| Timeout | Slider 3–15 s, default 8 | After timeout → fall back to plain dictation. |
| ~~Temperature~~ | **NOT exposed** | Hard-coded 0.0 in `LlmConfig`. Giving users a knob here is a footgun: non-zero temp + GBNF = occasional malformed JSON. |

A "Test" button below the toggle: plays a canned prompt ("refactor checkout in billing.tsx urgent") through the LLM and shows the resulting Markdown inline, so users can verify the model is working without recording.

A separate subpage `LlmModelsPage` (mirrors ModelsPage for Whisper) is reachable from the dropdown's "Manage models..." link — lists downloaded/available LLM models with download/delete controls, wired to new commands.

---

## 9. Phasing

### Phase 1 — Foundation (user-invisible)

New/modified files:
- **New:** `src-tauri/src/llm/{mod,types,engine,grammar,prompt,schema,template}.rs`, `src-tauri/src/llm_models/{mod,types,manager,downloader}.rs`, `src-tauri/resources/grammars/slot_extraction_v1.gbnf`
- **New:** `src-tauri/src/commands/llm.rs` — `list_llm_models`, `download_llm_model`, `delete_llm_model`, `set_active_llm_model`, `get_active_llm_model`, `llm_test_extract(text)`
- **Modified:** `Cargo.toml` (add llama-cpp-2 + feature map), `state.rs`, `lib.rs` (handler registration), `error.rs` (new `AppError::Llm(String)`)
- **Tests:** `src-tauri/src/llm/tests.rs` — grammar fixtures, schema round-trip, template snapshot tests

Estimated LOC: ~900 Rust, 0 TS (unless we expose the test command to a dev menu).

Demo to confirm: dev-only command `llm_test_extract` invoked from DevTools console returns valid Markdown for "refactor checkout in billing.tsx urgent".

Risks: CMake build failures on fresh Windows envs, feature flag conflicts with whisper-rs at link time (both embed ggml — llama-cpp-2 and whisper-rs may collide on ggml symbols). **Mitigation:** test a build with both `vulkan` features enabled early in Phase 1. If ggml symbol conflict appears, use the `whisper-rs` fork's `external-ggml` feature or align versions.

### Phase 2 — Structured Mode MVP

New/modified files:
- **Modified:** `pipeline.rs` (branch added — ~60 LOC), `storage/types.rs` + `storage/settings.rs` (4 new fields), `storage/history.rs` (migration adding `raw_transcript` column)
- **New:** `src/features/overlay/StructuredPanel.tsx` (~250 LOC), `src/features/settings/StructuredModeSection.tsx` (~180 LOC)
- **Modified:** `src/features/overlay/FloatingPill.tsx` (new structuring state, panel trigger — ~40 LOC), `src/lib/tauri.ts` (new types + listeners — ~60 LOC), `src/stores/recordingStore.ts` (new status value)
- **New:** `src/features/settings/LlmModelsPage.tsx` (~200 LOC)

Estimated LOC: ~550 Rust, ~750 TS. Default OFF.

Demo: toggle Structured Mode on in Settings, download model if needed, dictate "fix the login bug in auth.ts and add a test, this is urgent", observe panel appear with Markdown, click Paste into a target editor.

Risks: panel obscuring the pill when user is mid-workflow; fallback path not always exercised in manual testing. **Mitigations:** explicit ESC-dismiss; hidden `structured_force_fallback: true` dev flag to exercise the fallback path during QA.

### Phase 3 — Target detection

- Keyword prefix parser ("tell Claude to ...", "for Cursor, ...") strips the prefix and sets a `target` field.
- Per-target template modules: `template::claude_code`, `template::cursor`, `template::chatgpt`, `template::codex`, `template::generic` (current v1 template).
- Settings: default target picker; per-target template preview.

Risks: prefix parsing false positives. **Mitigation:** only match at start-of-utterance, only after trimming filler words; unmatched falls through to generic.

---

## 10. Risks & mitigations

| Risk | Mitigation |
|---|---|
| **Latency regression** (the #1 killer of the previous cleanup feature) | FunctionGemma 270M Q4 at ~100 tok/s on GPU / ~50 tok/s on CPU. JSON output is ~80 tokens typical. Target: < 2 s GPU, < 3 s CPU. Hard timeout at 8 s fallback. Explicit `structured_min_chars` threshold skips short utterances entirely. |
| **Formatting mangling** (lists word-by-word, broken prose) | LLM output is constrained to the GBNF JSON grammar. It CANNOT emit prose. The Markdown is deterministically assembled by Rust from the JSON. Worst-case failure = invalid JSON → fallback to plain pipeline. |
| **Hallucination** (LLM invents files or constraints) | System prompt hard-constrains "only include slots the user mentioned." GBNF forces every field except `goal` to be optional. If we see false positives in testing, tighten prompt with a negative example. |
| **Prose rewriting** | Impossible given grammar: no field is "rewritten goal." Slot values are copied near-verbatim. Panel's "View raw" disclosure lets user always see original text. |
| **Users can't turn it off** | Single prominent toggle in Settings. Default OFF on upgrade. Keyboard shortcut in the panel to "dismiss and disable for this session." |
| **Model download failures** | Reuse the battle-tested `.part` + atomic rename pattern from `ModelDownloader`. Resume-on-retry is a Phase 3 add. On download failure, Structured Mode auto-disables and shows an error panel with retry. |
| **Model file corruption** | Validate by attempting to load the model once post-download; if load fails, delete the file and re-queue the download. Show SHA256 verification on re-download if two consecutive loads fail (Phase 3). |
| **GPU driver issues** | `LlamaEngine::load` wraps the `llama_cpp-2` init in `panic::catch_unwind` (mirrors whisper-rs loader). On GPU init failure, automatically retry once with `use_gpu = false` and emit `llm-gpu-fallback` event. |
| **Memory pressure** on low-RAM machines | Q4 default (~180 MB). Idle-unload after 10 min. Don't preload at startup — lazy-load on first structured inference. |
| **LLM + Whisper ggml symbol collision** | Tested in Phase 1 with both features on. If conflict → pin versions or use `external-ggml` feature. **Hard stop before Phase 2 if unresolved.** |
| **Voice-command users surprised by auto-bypass** | Document in toggle tooltip: "Voice commands and bullet formatting are disabled while Structured Mode is active." |
| **Panel state escapes user focus** | Panel auto-closes on new recording; ESC dismisses; pasting auto-closes. Never modal — can always be dismissed. |

---

## 11. Testing

### Unit tests (`src-tauri/src/llm/tests.rs`)

- `schema::SlotExtraction` deserializes every permutation of optional fields (11 combinations).
- `template::render_markdown` snapshot tests: input fixtures → expected Markdown, committed as `.snap` files.
- Grammar load: `include_str!` compiles to a `LlamaGrammar` without error.

### Integration test (`src-tauri/src/pipeline/tests.rs`)

New `AsrEngine` + `LlmEngine` mocks. The `LlmEngine` trait (introduced for this) lets `pipeline::stop_and_transcribe` accept an `Arc<dyn LlmEngine>` in test builds. Test cases:

1. Structured mode off → plain text output (existing behavior preserved).
2. Structured mode on + mock returns valid slots → Markdown output.
3. Structured mode on + mock returns error → fallback to plain text; `structured-mode-degraded` emitted.
4. Structured mode on + mock sleeps 20 s → timeout at 8 s → fallback; `structured-mode-degraded` emitted.
5. Structured mode on + transcript shorter than `structured_min_chars` → plain text (no LLM call).

### Manual test script (10 dictations)

The user should try these with FunctionGemma Q4 loaded, target is VS Code or a markdown-preview area:

1. *"Refactor the checkout flow in billing.tsx and cart.tsx, keep the Stripe integration, urgent"* → all 4 slots present.
2. *"Write a commit message for this change, conventional commits style"* → goal + `output_format="commit_message"`.
3. *"Explain what this function does in plain English"* → goal only (no files, no constraints).
4. *"Fix the bug where the login form resets on refresh, don't touch the auth middleware"* → goal + constraint.
5. *"Short one"* (< 40 chars) → falls through to plain dictation, panel does NOT appear.
6. *"For Claude, add error handling around the fetch call in api-client.ts"* (Phase 3 target detection) → target=Claude template.
7. *"New paragraph. Actually, scratch that, just bullet out the three things I need: first, fix validation, second, update the docs, third, add a test."* → goal + follow_up_tasks=[3 items].
8. *"Run the deploy, it's not urgent"* → goal + urgency=low.
9. (Disconnect GPU / force CPU) same as #1 → Markdown still correct, latency under 3 s.
10. (Kill LLM engine mid-inference by deleting the model file) → `structured-mode-degraded` event fires, plain text still pasted.

Ship criteria: tests 1–5 and 7–10 all pass. Test 6 deferred to Phase 3.

---

## 12. Open decisions (user input needed before Phase 1 begins)

1. **Bundling** — confirm: do NOT bundle FunctionGemma in the installer; download on first enable (§3.1).
2. **Default model** — confirm: FunctionGemma 270M Q4_K_M (180 MB).
3. **History schema migration** — confirm: add `raw_transcript` column to `transcriptions` so Structured Mode outputs can always be traced back to the original dictation (§6.3).
4. **Phase 1 → Phase 2 gate** — agreement: if ggml symbol collision between llama-cpp-2 and whisper-rs can't be resolved, Phase 2 is blocked. Acceptable stop condition?
5. **Panel behavior under ghost mode** — confirm: respect ghost mode even when structured output is ready (don't force panel open). Panel accessible later via history.
6. **Voice command + Structured Mode interaction** — confirm: Structured Mode disables voice command detection and list formatting. The LLM is the sole formatter.
7. **Temperature knob in Settings** — confirm: NOT exposed. Hard-coded 0.0. Any reason to override this?
