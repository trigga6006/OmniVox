# Import Audio — Spec & Implementation Plan

Status: **DRAFT — Round 2 review applied**
Owner: Ben
Target branch: `feat/import-audio` (off `main`)

### Round 2 review — changes applied

- §6.2 / §6.5: Spawned task no longer takes `AppState` by value
  (`AppState` is not `Clone` — it owns `Mutex`, `Database`, managers).
  The task receives only `AppHandle` + ids + cancel flag, and retrieves
  managed state per access via `app_handle.state::<AppState>()`. This
  matches the existing pattern at `pipeline.rs:516-525`. (Codex P1 —
  would not have compiled as written)
- §7.2: Store shape changed from `interface ImportStore extends
  ImportState` (invalid — interfaces cannot extend a discriminated
  union) to `type ImportStore = { state: ImportState } & ImportActions`.
  Wrapping the union under a single field also forces listeners to
  write a complete variant rather than spreading partial updates.
  (Codex P2 × 2)
- §7.4: `ImportProgress` payload gains `duration_ms: number | null`,
  populated by Rust once decoding completes. Lets the listener
  construct a complete `transcribing` variant without reading current
  store state for missing fields.
- §7.1 / §9 / Appendix: The disabled "record" affordance moved from
  `Sidebar.tsx` (which has no record button — only nav + a status dot)
  to `RecordButton.tsx` / `DictationPanel.tsx` where the actual control
  lives. Sidebar gets an optional import-in-progress dot mirroring the
  existing recording dot at `Sidebar.tsx:60-64`. (Codex P3)

### Round 1 review — changes applied

- §6.5: `import_transcribe` now returns immediately after spawning the
  task. Frontend supplies the `import_id` so it can subscribe before
  calling. (Codex P1 — fire-and-forget race)
- §6.4 / §6.5: Cancellation uses `FullParams::set_abort_callback_safe`
  (returns `bool`), not the progress callback (which is `FnMut(i32)`
  with no return). Progress callback is for events only. (Codex P1)
- §6.5 / §9: Live dictation and imports share a backend
  `transcription_gate` semaphore that covers live final processing, not
  just active microphone capture. Both directions are enforced
  server-side.
  (Resolves §11.7)
- §6.2: Dropped the `wait_for_preview_worker` call from the import
  pipeline — preview is owned by the live recording session and the
  shared transcription guard prevents overlap with imports. `pipeline.rs`
  stays otherwise focused on the live path. (Codex P2)
- §7.1 / §7.2: Page state moved to a small Zustand `importStore` and
  the event listeners are mounted once in `App.tsx`. Navigating away
  mid-import no longer loses progress. (Resolves §11.2)
- §7.1: Drag-and-drop uses `getCurrentWebview().onDragDropEvent`, not
  HTML5 file drag — browser `File` objects don't reliably expose local
  paths in Tauri. (Codex P2)
- §4.3 / §7.5: "Save to existing note" appends with a blank-line
  separator and an `## interview.m4a` filename heading, never
  overwrites. (Codex P2)

---

## 1. Motivation

OmniVox today only transcribes **live microphone input** through
`pipeline::stop_and_transcribe`. Users have asked for a way to drop in an
existing audio file (a recorded meeting, a voice memo, an interview clip)
and get a clean transcript they can paste into a note.

The whisper engine already handles every type of audio Whisper is good at
— the gap is purely the **input adapter** (decoding an arbitrary container
to `16 kHz mono f32`) and a **contained UI surface** that doesn't touch
the live-dictation side-effect chain.

## 2. Goals (v1)

1. New **Imports** page in the sidebar. Drop zone + file picker for
   `.wav`, `.mp3`, `.m4a`, `.flac`, `.ogg`. Single file at a time.
2. Server-side decode + resample to `16 kHz mono f32` (matches what
   `cpal` produces for live capture, so the existing
   `AsrEngine::transcribe` signature is reused unchanged).
3. **Real progress** during transcription (not just a spinner), using
   whisper.cpp's progress callback.
4. Transcript preview in an editable textarea on the Import page with
   three actions: **Copy**, **Save to Note** (pick existing or create
   new), **Discard**.
5. Every successful import is saved to the existing `transcriptions`
   table with `source = "import"` plus filename metadata, so the
   History page shows it like any other transcription.
6. Audio file is **not retained** after transcription completes — only
   the filename is recorded.

## 3. Non-goals (explicitly out of scope for v1)

- **Batch import / queue.** One file at a time. A queue is easy to add
  later once the single-file path is solid.
- **Speaker diarisation.** Whisper has no speakers built in. This is a
  separate large project (pyannote etc.).
- **Auto-paste to focused app.** Imports stay on the Import page. The
  user explicitly chooses Copy or Save to Note.
- **Ship Mode, voice commands, "Voxify" trigger, screen-context capture,
  audio ducking, focus restoration.** All disabled for imports — see
  §6.3 for the reasoning. Settings struct is **not modified**; the
  import pipeline simply doesn't read those fields.
- **Structured Mode on imports.** v1 produces plain transcript only.
  Re-running an import through Structured Mode is a follow-up — likely
  belongs on the History page (a "structure this" action that works for
  both live and imported rows) rather than on the Import page.
- **Re-import / re-transcribe an existing history row** with a different
  model. Adjacent feature; deferred.
- **Drag-and-drop of multiple files.** UI accepts one; if the user drops
  multiple, we transcribe the first and ignore the rest with a toast.
- **Per-import dictionary / vocabulary scoping.** The import pipeline
  uses the **currently active** dictionary + vocabulary, same as live
  dictation does today. No "pick a profile for this import" UI in v1.

## 4. UX flow

Sidebar gets a new entry between **Dictation** and **History**:

```
🎤 Dictation
📥 Imports   ← new
🕐 History
📒 Notes
📖 Dictionary
⬇ Models
⚙ Settings
```

Icon: `Upload` from lucide-react (consistent with `Download` used for Models).

The page is a **single screen** with three visual states driven by a small state machine:

### 4.1 State: `idle`

- Centered drop zone (dashed border, amber accent on hover, matches
  empty-state styling on Notes / History).
- Below: "or **browse files**" button → native file picker
  (`@tauri-apps/plugin-dialog`).
- Below that: tiny grey caption listing supported formats and a soft
  size limit ("Up to 500 MB. Long files take longer to transcribe.").

### 4.2 State: `decoding` → `transcribing`

- Filename + duration ("interview.m4a · 12:43") at the top.
- Single progress bar. Phases:
  - `0–5 %`: decoding + resampling
  - `5–100 %`: Whisper inference (driven by `set_progress_callback_safe`)
- Cancel button. Cancelling drops the audio buffer and returns to `idle`.
- The AudioVisualizer wave from `features/dictation/AudioVisualizer.tsx`
  is **not reused** — that one is driven by live RMS samples. We use a
  simple shimmer / indeterminate fallback while waiting for the first
  progress tick, then a real percentage bar.

### 4.3 State: `complete`

- Filename + duration + "Transcribed in 23s" line.
- Editable textarea with the transcript. (Edits are local-only; if the
  user saves to a note, the edited text is what gets saved.)
- Action row: **Copy** · **Save to Note ▾** · **New import** · **Discard**.
- "Save to Note" opens a small popover: search existing notes or
  "+ New note from this transcript". New-note uses the filename
  (without extension) as the title and the transcript as the content.
  Save-to-existing **appends** to the bottom of the chosen note with
  a blank-line separator and an `## <filename>` heading — never
  overwrites. (See §7.6 for the exact merge rule.)

### 4.4 State: `error`

- Filename + red banner with the failure reason ("Couldn't decode this
  file format" / "Transcription failed: …").
- Buttons: **Try again** · **Pick a different file**.

## 5. Data model

### 5.1 `transcriptions` table — extend, don't fork

Adding three nullable columns rather than a new `imports` table. This
keeps the History UI as a single unified list, and we filter / badge by
`source` in the existing row component.

```sql
ALTER TABLE transcriptions ADD COLUMN source TEXT;
ALTER TABLE transcriptions ADD COLUMN source_filename TEXT;
ALTER TABLE transcriptions ADD COLUMN source_duration_ms INTEGER;
```

- `source`: `"live"` | `"import"`. `NULL` for pre-migration rows is
  treated as `"live"` on read (default-on-read pattern, matches the
  existing `raw_transcript` migration).
- `source_filename`: original filename only (no path). Stored so the
  History row can show "interview.m4a" alongside the snippet.
- `source_duration_ms`: media duration of the source file. May differ
  from `duration_ms` if we ever trim silence, but for v1 they're equal.
  Keeping the column separate makes that future-proof.

Migration follows the existing `migrate_add_*` pattern in
`src-tauri/src/storage/database.rs:140-218`:

```rust
fn migrate_add_import_metadata(&self) -> AppResult<()> {
    // Three independent column-existence checks, one ALTER each.
}
```

Call it from `create_tables()` alongside the other migrations.
**Do not bump `PRAGMA user_version`** — the existing migrations don't
key off it, and bumping would orphan in-flight upgrades from earlier
versions.

### 5.2 `TranscriptionRecord` (`src-tauri/src/storage/types.rs:27-42`)

Add three optional fields with the same `serde` pattern as
`raw_transcript`:

```rust
#[serde(default, skip_serializing_if = "Option::is_none")]
pub source: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub source_filename: Option<String>,
#[serde(default, skip_serializing_if = "Option::is_none")]
pub source_duration_ms: Option<u64>,
```

`Option<String>` for `source` rather than an enum so the existing
`row_to_record` helper stays straightforward and pre-migration rows
deserialize as `None`. We can tighten to an enum later if needed.

### 5.3 Frontend type mirror

Extend `TranscriptionRecord` in `src/lib/tauri.ts:42-50` with the three
optional fields. No other type changes required — Notes, History,
Settings all keep their current shape.

## 6. Rust architecture

### 6.1 New module: `src-tauri/src/audio/decode.rs`

Pure-Rust decode + resample. **No ffmpeg dependency** — shipping ffmpeg
on Windows is a deploy headache.

```rust
pub struct DecodedAudio {
    pub samples: Vec<f32>,      // 16 kHz mono, peak in [-1.0, 1.0]
    pub duration_ms: u64,
}

pub fn decode_file(
    path: &Path,
    progress: impl Fn(f32),     // 0.0–1.0, fires every ~250 ms
) -> AppResult<DecodedAudio>;
```

Implementation: **symphonia** for demux + decode (handles WAV, MP3,
FLAC, AAC/M4A, OGG/Vorbis out of the box), **rubato** for resampling
to 16 kHz. Channel downmix is a simple per-frame average — Whisper is
mono-only, no point preserving stereo.

New Cargo deps:

```toml
symphonia = { version = "0.5", features = [
    "mp3", "aac", "isomp4", "vorbis", "flac", "wav",
] }
rubato = "0.16"
```

Approximate weight: ~1.5 MB to the release binary. Acceptable for a
desktop app; we can revisit if it bloats noticeably.

### 6.2 New module: `src-tauri/src/pipeline/import.rs`

Don't add to `pipeline.rs` — it's already 1064 lines. Splitting
`pipeline.rs` into a module folder is a larger refactor and not in
scope; the import path lives in a sibling file referenced as
`crate::pipeline_import::...` (or we promote `pipeline` to a module
folder later). **Decision for review:** keep `pipeline.rs` as the live
pipeline, add `pipeline_import.rs` as a sibling. Codex: push back if
you'd prefer the module-folder refactor up front.

The import pipeline is **strictly transcribe + persist** — none of the
side effects from `pipeline.rs:567-1044`:

```rust
pub async fn transcribe_file(
    app_handle: AppHandle,
    import_id: Uuid,
    path: PathBuf,
    cancel: Arc<AtomicBool>,    // shared with import_cancel command
) {
    // AppState is NOT passed in — it is not Clone and owns non-Clone
    // fields (Mutex, Database, managers).  Inside this task we retrieve
    // managed state per access, exactly as the live pipeline does at
    // pipeline.rs:516-525:
    //
    //     let state: tauri::State<'_, AppState> = app_handle.state();
    //     let engine_opt = state.engine.lock().ok().and_then(|g| g.as_ref().map(Arc::clone));
    //
    // The Arc<WhisperEngine> can be moved into the spawn_blocking
    // closure for the inference step; the lock is released first so
    // it doesn't hold across .await.
    //
    // 1. Emit import-progress { import_id, phase: "decoding",
    //    percent: 0, duration_ms: null }.
    // 2. decode::decode_file(path, |p| { if cancel.load(...) return; emit })
    //    The decode loop checks `cancel` between frames and short-circuits
    //    by returning a `Cancelled` error variant.  No whisper involvement
    //    yet, so cancellation here is pure-Rust and instant.
    //    On success, compute duration_ms from the sample count.
    // 3. Emit import-progress { phase: "transcribing", percent: 0,
    //    duration_ms: Some(d) } so the frontend can transition state
    //    and show the source duration in the UI ("12:43") before the
    //    first inference tick lands.
    // 4. Optional denoise + normalize (driven by the existing
    //    noise_reduction setting via `app_handle.state::<AppState>()` —
    //    same audio quality benefits apply to imported files).
    // 5. spawn_blocking → engine.transcribe_with_progress(&samples,
    //                       on_progress = |p| emit import-progress,
    //                       abort_when  = || cancel.load(...))
    //    See §6.4 for why progress and abort are separate callbacks.
    // 6. processor.process(&text)  ← dictionary / capitalization, same
    //    as live path. Voice-command parsing + Ship Mode + Voxify gating
    //    are NOT applied.
    // 7. Save TranscriptionRecord with source="import",
    //    source_filename, source_duration_ms.
    // 8. Emit `import-transcription-ready` { import_id, text,
    //    source_filename, duration_ms } — NOT `transcription-result`.
    // 9. Clear active_import (via the Drop guard from §6.6) so
    //    subsequent imports + live recordings can proceed. Always runs
    //    on success / error / cancel.
}
```

Note: `wait_for_preview_worker` from `pipeline.rs:549` is intentionally
**not** called here. The live preview worker is spawned by
`start_recording` and exits when its recording session ends. Since
§6.5 serializes all Whisper final-transcription work behind
`state.transcription_gate`, there is no scenario where a preview worker
and an import both allocate Whisper decode state. The function stays
private inside `pipeline.rs`.

Explicitly **omitted** from the live path:

| Live pipeline step (`pipeline.rs`) | Reason omitted for imports |
|---|---|
| `capture_foreground_window` (270-275) | No "previous app" — user is in OmniVox |
| `auto_switch_modes` (284-326) | Context is the import, not a focused app |
| `audio::ducking::duck` (333-335) | No mic recording |
| `screen_context` capture (341-359) | Would capture OmniVox's own UI |
| Live preview worker (420-547) | Only meaningful for in-progress mic |
| `voxify` trigger detection (757-770) | False positives on long recordings |
| `voice_commands` parsing (927-933) | False positives in transcript content |
| Output routing (954-968) | Imports must not paste anywhere |
| Ship Mode auto-Enter (975-997) | Same — would be catastrophic |
| `transcription-result` emit (1006) | Notes auto-append would silently fire |
| `structured-output-ready` (1007-1020) | Skip in v1; revisit on History page |

Steps that **are** shared / reused (worth importing as helpers rather
than copy-paste):

- `crate::audio::normalize::normalize_peak`
- `crate::audio::denoise::denoise` (gated by `settings.noise_reduction`)
- `state.processor.process(...)` (dictionary + capitalization)
- `crate::storage::history::save_transcription`

### 6.3 Why a separate `import-transcription-ready` event

This is the single biggest design decision; documenting it explicitly
for review.

`transcription-result` is treated by current code as a "live dictation
just completed, run the full side-effect chain" signal:

1. **NotesPage** (`src/features/notes/NotesPage.tsx:91-102`) appends
   any `transcription-result` into the currently open note. Firing this
   from the import path would dump the entire transcript into whatever
   note the user happened to leave open last.
2. **Last-transcription store** (mentioned in `pipeline.rs:1000-1005`
   comment) — same coupling.
3. Plausibly future listeners (auto-clear preview, history auto-refresh
   already exists) all assume "this just came out of the mic".

A new event makes the boundary explicit and lets the Import page own
its own listener.

### 6.4 Whisper progress + abort callbacks

`whisper-rs 0.16` exposes **two separate callbacks**, both of which we
use — they're not interchangeable:

- **Progress:** `FullParams::set_progress_callback_safe(|p: i32| {})`
  is `FnMut(i32)` returning `()`. Purely informational: we forward the
  0–100 percent to the frontend via `import-progress` events.
  **Cannot abort** — the spec previously claimed cancellation could
  short-circuit through this callback; that was wrong.
- **Abort:** `FullParams::set_abort_callback_safe(|| -> bool { … })`
  is `FnMut() -> bool` returning `true` to stop inference. Whisper
  polls this between decode steps, so cancellation latency is bounded
  by one segment (≈1–2 s on base, ≈5–10 s on large). This is what
  honors `cancel.load(Ordering::Relaxed)`.

We wire both through a new method on `WhisperEngine`:

```rust
pub fn transcribe_with_progress(
    &self,
    audio: &[f32],
    on_progress: impl FnMut(u8) + Send + 'static,
    abort_when: impl FnMut() -> bool + Send + 'static,
) -> AppResult<TranscriptionResult>;
```

Implementation duplicates ~30 lines from `transcribe()` (the params
setup) but is the right call — adding callbacks to the existing
`AsrEngine` trait would force every mock implementation to handle
them, and there's no use case for live-dictation progress or aborts
(the user is holding the button and can see the audio level).

The progress callback is throttled: only emit a Tauri event when
`percent` has changed by ≥1. The C side calls back constantly;
emitting every tick floods the IPC channel.

When the abort callback returns `true`, `state.full(...)` returns an
`Err` (whisper.cpp surfaces it as an inference failure). We map that
to a dedicated `AppError::Cancelled` variant so the import pipeline
can distinguish "user cancelled" from "real failure" and avoid
emitting an error toast.

### 6.5 New Tauri commands

In a new `src-tauri/src/commands/imports.rs`:

```rust
#[tauri::command]
pub async fn import_transcribe(
    app_handle: AppHandle,
    state: State<'_, AppState>,
    import_id: String,   // frontend-generated UUID, passed in
    path: String,
) -> Result<(), String>;

#[tauri::command]
pub async fn import_cancel(
    state: State<'_, AppState>,
    import_id: String,
) -> Result<(), String>;
```

**The command returns immediately** after validating inputs, claiming
`active_import`, and spawning the work via `tauri::async_runtime::spawn`.
It does **not** await decode or transcription. This is the critical
fix from round 1: if the command awaited the full pipeline, the
frontend would only register progress listeners after `await`
resolves — by which point every `import-progress` event and likely the
`import-transcription-ready` event would already have fired.

Sketch of the command body (round 2 — `AppState` is borrowed via
`State<'_, _>` here in the command, **not** moved into the spawned
task):

```rust
let import_uuid = Uuid::parse_str(&import_id)
    .map_err(|e| format!("bad import_id: {e}"))?;
let path_buf = PathBuf::from(&path);

// Pre-flight: file exists, size cap, supported extension.
validate_import_path(&path_buf)?;

// Concurrency guard — see §9 edge-case table.
if state.audio.lock().map(|a| a.is_recording()).unwrap_or(false) {
    return Err("Stop recording before importing".into());
}

let transcription_permit = state
    .transcription_gate
    .clone()
    .try_acquire_owned()
    .map_err(|_| "Dictation is still processing".to_string())?;

let cancel = Arc::new(AtomicBool::new(false));
{
    let mut guard = state.active_import.lock()
        .map_err(|_| "active_import lock poisoned".to_string())?;
    if guard.is_some() {
        return Err("Another import is already in progress".into());
    }
    *guard = Some(ImportState { id: import_uuid, cancel: cancel.clone() });
}

let handle = app_handle.clone();
tauri::async_runtime::spawn(async move {
    // Inside this task: retrieve managed state per access.
    // Do NOT capture `state` from the outer scope.
    // Keep `transcription_permit` alive until the task exits.
    let _permit = transcription_permit;
    crate::pipeline_import::transcribe_file(
        handle, import_uuid, path_buf, cancel,
    ).await;
});
Ok(())
```

**Frontend pattern** (in the `importStore` action, see §7.2):

```ts
async function startImport(path: string) {
  const importId = crypto.randomUUID();
  useImportStore.setState({
    state: {
      kind: "decoding",
      importId,
      filename: basename(path),
      percent: 0,
    },
  });
  // Listeners are already mounted in App.tsx and write to the store —
  // see §7.2.  By the time the command returns, the store is already
  // receiving events for this importId.
  await importTranscribe(importId, path);
}
```

The single global listener (mounted once in `App.tsx`) filters events
by reading `useImportStore.getState().state`, narrowing to a busy
variant, and comparing `payload.import_id` with `state.importId` before
writing. Stale events from a cancelled prior import are ignored.

**Concurrency guards (both directions, resolves §11.7):**

- Add `state.transcription_gate: Arc<tokio::sync::Semaphore>` with one
  permit and acquire an `OwnedSemaphorePermit` for the entire span where
  Whisper final inference may run. Use `try_acquire_owned()` for
  user-facing commands so busy states fail immediately instead of
  silently queueing:
  - live path: in `stop_and_transcribe`, acquire near the top of the
    function before stopping mic capture. This closes the tiny race where
    `audio.is_recording()` could become false before live Whisper has
    claimed the gate. Release after history save / error / cancellation
    cleanup.
  - import path: in `import_transcribe`'s spawned task, before decoding
    begins or at latest before `engine.transcribe_with_progress(...)`;
    release after ready / error / cancellation cleanup.
- `import_transcribe` also returns `Err("Stop recording before importing")`
  if `state.audio.is_recording()` is true, so users cannot start a file
  import while actively recording.
- If `import_transcribe` cannot acquire `transcription_gate`, it returns
  `Err("Dictation is still processing")` when no import is active. This
  closes the gap where mic recording has stopped but live Whisper
  processing is still running.
- `start_recording` / `toggle_recording` in `pipeline.rs` guard against
  either `state.active_import.lock().is_some()` or
  `state.transcription_gate.clone().try_acquire_owned().is_err()`. Return
  early with `emit_error(..., ErrorCode::Busy, "Import in progress")`
  for active imports and `"Transcription still processing"` for live
  final processing. If the permit check succeeds in `start_recording`,
  drop it immediately; the real permit is acquired later by
  `stop_and_transcribe`.
- Only **one import at a time** in v1. If `active_import` is occupied,
  `import_transcribe` returns an error.

The combined effect is that whisper-rs `WhisperState` is never
allocated twice concurrently — addresses the same memory-pressure
class already documented at `pipeline.rs:411-419`.

**Cancellation:** cooperative via an `Arc<AtomicBool>` stored in
`ImportState` (see §6.6). The decode loop checks it between frames;
the transcribe step plumbs it through to whisper's
`set_abort_callback_safe` (see §6.4). `import_cancel` validates that
the supplied `import_id` matches the active one and flips the flag.

### 6.6 `AppState` additions

Add two fields to `src-tauri/src/state.rs`:

```rust
pub active_import: Arc<Mutex<Option<ImportState>>>,
pub transcription_gate: Arc<tokio::sync::Semaphore>,

pub struct ImportState {
    pub id: Uuid,
    pub cancel: Arc<AtomicBool>,
}
```

The `Arc<AtomicBool>` is cloned into the spawned task at the moment
the import is registered. The task owns its clone for the duration of
the work and clears `active_import` (back to `None`) at every exit
point: success, error, or cancellation. A `Drop` guard on a small
RAII handle is the cleanest way to enforce this without scattered
`*guard = None;` calls.

`transcription_gate` is separate from `active_import`: it represents
"Whisper final inference or import transcription owns the large decode
state right now." This matters because live dictation has a processing
phase after mic capture stops (`recording-state-change: "processing"`),
when `audio.is_recording()` is false but Whisper is still running.

### 6.7 File-picker plugin

Add `tauri-plugin-dialog` to `Cargo.toml` and `package.json`. Register
in `lib.rs` via `.plugin(tauri_plugin_dialog::init())`. Add the
`dialog:allow-open` capability for the main window.

Path validation in `import_transcribe`: reject non-existent files,
reject files >500 MB (configurable constant — picked as "longer than
any realistic dictation, short enough that a 16 kHz mono f32 buffer
fits comfortably in RAM").

## 7. Frontend architecture

### 7.1 New page: `src/features/imports/ImportsPage.tsx`

The page is a **thin view layer** that reads from `useImportStore()`
(see §7.2) and renders one of four states. All state transitions are
driven by store actions, not component-local `useState`. This means:

- A 12-minute import keeps its progress when the user navigates to
  History or Settings and comes back — the page re-renders from the
  store, no listeners are re-registered.
- The dictation **record button** (`src/features/dictation/RecordButton.tsx`
  / consumed by `DictationPanel.tsx`) reads
  `useImportStore(s => s.state.kind !== "idle" && s.state.kind !== "complete")`
  to show a disabled / tooltip state ("Import in progress") instead of
  silently failing when the user clicks it. Sidebar itself has no
  record control — it just has nav buttons + a recording status dot
  at `Sidebar.tsx:60-64`, which we mirror with an analogous
  import-in-progress dot when one is active.

**Drag-and-drop** uses Tauri's webview API, not HTML5 file drag —
browser `File` objects don't reliably expose local filesystem paths
in Tauri's webview, and the path is exactly what
`import_transcribe(path)` needs:

```ts
import { getCurrentWebview } from "@tauri-apps/api/webview";

useEffect(() => {
  const unlisten = getCurrentWebview().onDragDropEvent((event) => {
    if (event.payload.type === "drop" && event.payload.paths.length > 0) {
      const [first, ...rest] = event.payload.paths;
      if (rest.length > 0) {
        toast("Importing the first file; ignoring the rest.");
      }
      useImportStore.getState().startImport(first);
    }
  });
  return () => { unlisten.then(fn => fn()); };
}, []);
```

The browse button uses `@tauri-apps/plugin-dialog`:

```ts
const path = await open({
  multiple: false,
  filters: [{ name: "Audio", extensions: ["wav","mp3","m4a","flac","ogg","aac"] }],
});
if (path) useImportStore.getState().startImport(path);
```

The state shape lives in the store (§7.2), not the page. The page is
a `switch (store.kind)` rendering the appropriate sub-view.

### 7.2 New store: `src/stores/importStore.ts`

Small Zustand store, mirrors the shape of `recordingStore`. The state
is a **discriminated union** wrapped under a single `state` field —
this is deliberate. An `interface ImportStore extends ImportState`
would be invalid TypeScript (interfaces cannot extend unions), and a
flat `ImportState & ImportActions` intersection breaks discriminated-
union narrowing on selectors (TS can't narrow when the discriminant
sits next to non-union action fields).

Wrapping forces every state transition to write a **complete variant**,
which addresses the round-1 mistake where the listener spread partial
updates like `setState({ kind, percent })` — runtime Zustand merges
would have retained stale `importId` / `filename` from a prior import,
and TypeScript would have provided zero help catching it.

```ts
type ImportState =
  | { kind: "idle" }
  | { kind: "decoding";     importId: string; filename: string; percent: number }
  | { kind: "transcribing"; importId: string; filename: string; percent: number; durationMs: number }
  | { kind: "complete";     importId: string; filename: string; durationMs: number; transcript: string }
  | { kind: "error";        importId: string | null; filename: string | null; message: string };

type ImportActions = {
  startImport: (path: string) => Promise<void>;
  cancelImport: () => Promise<void>;
  discardComplete: () => void;
  updateTranscript: (text: string) => void;  // editable textarea
};

type ImportStore = { state: ImportState } & ImportActions;
```

Selector pattern stays clean:

```ts
const phase = useImportStore(s => s.state.kind);
const busy  = useImportStore(s => s.state.kind !== "idle" && s.state.kind !== "complete");

// Narrowing works because the discriminant is on the same object:
const s = useImportStore(s => s.state);
if (s.kind === "transcribing") {
  // TS knows s.percent and s.durationMs exist here
}
```

**Event listeners are mounted once** in `App.tsx` (alongside the
existing `onRecordingStateChange` etc.) and write directly to the
store. The page subscribes via `useImportStore` and never registers
listeners itself — that's what keeps progress alive across navigation.

Every listener handler reads the current state, validates the
`import_id` matches, then writes a **complete** new variant. The
`duration_ms` value flows through the progress event itself (§7.4)
so the listener never needs to invent values:

```ts
// In App.tsx mount:
useEffect(() => {
  const unlistenProgress = onImportProgress((p) => {
    const cur = useImportStore.getState().state;

    // Stale event (e.g. cancelled prior import) — ignore.
    if (cur.kind !== "decoding" && cur.kind !== "transcribing") return;
    if (p.import_id !== cur.importId) return;

    if (p.phase === "decoding") {
      useImportStore.setState({
        state: {
          kind: "decoding",
          importId: cur.importId,
          filename: cur.filename,
          percent: p.percent,
        },
      });
    } else {
      // Rust sends duration_ms on every transcribing tick — see §7.4.
      // We assert here rather than guess.
      if (p.duration_ms == null) {
        console.warn("transcribing event without duration_ms; dropping");
        return;
      }
      useImportStore.setState({
        state: {
          kind: "transcribing",
          importId: cur.importId,
          filename: cur.filename,
          percent: p.percent,
          durationMs: p.duration_ms,
        },
      });
    }
  });

  const unlistenReady = onImportTranscriptionReady((p) => {
    const cur = useImportStore.getState().state;
    if (cur.kind === "idle" || cur.kind === "complete" || cur.kind === "error") return;
    if (p.import_id !== cur.importId) return;
    useImportStore.setState({
      state: {
        kind: "complete",
        importId: cur.importId,
        filename: p.source_filename,
        durationMs: p.duration_ms,
        transcript: p.text,
      },
    });
  });

  return () => {
    unlistenProgress.then(fn => fn());
    unlistenReady.then(fn => fn());
  };
}, []);
```

The store is reset to `{ state: { kind: "idle" } }` by
`discardComplete()` and after the user chooses **New import** from
the complete state.

### 7.3 New sidebar entry

`src/app/Sidebar.tsx:6-12` — add:

```ts
{ page: "imports", icon: Upload, label: "Imports" },
```

…between Dictation and History. `Page` type in `src/stores/appStore.ts`
gains the `"imports"` variant. Routing switch in `App.tsx` gets the
new branch.

### 7.4 New types + bindings in `src/lib/tauri.ts`

```ts
export interface ImportProgress {
  import_id: string;
  phase: "decoding" | "transcribing";
  percent: number;             // 0–100
  /**
   * Source media duration in ms.
   * NULL during the decoding phase (we don't know it yet).
   * Always populated by Rust on every transcribing-phase event so the
   * listener can construct a complete `transcribing` variant without
   * reading current store state — see §7.2.
   */
  duration_ms: number | null;
}

export interface ImportTranscriptionReady {
  import_id: string;
  text: string;
  source_filename: string;
  duration_ms: number;
}

export const importTranscribe = (importId: string, path: string) =>
  invoke<void>("import_transcribe", { importId, path });
export const importCancel = (importId: string) =>
  invoke<void>("import_cancel", { importId });

export const onImportProgress = (cb: (p: ImportProgress) => void) =>
  listen<ImportProgress>("import-progress", e => cb(e.payload));
export const onImportTranscriptionReady = (cb: (p: ImportTranscriptionReady) => void) =>
  listen<ImportTranscriptionReady>("import-transcription-ready", e => cb(e.payload));
```

Extend `TranscriptionRecord` with the three optional fields from §5.3.

### 7.5 HistoryPage delta

`src/features/history/HistoryPage.tsx` already auto-refreshes on
`transcription-result`. Add a parallel listener for
`import-transcription-ready` so imported transcripts appear in History
without a refresh. Render a small "📥 imported" badge next to rows
where `source === "import"`, with the filename in the tooltip.

### 7.6 NotesPage interaction

**No changes required to NotesPage itself.** The Notes editor only
listens for `transcription-result`, so imports won't auto-append.

The "Save to Note" action on the Import page is owned by the page (or
the store action). Two cases:

- **New note.** `addNote(filename, transcript)` — filename without
  extension becomes the title, transcript becomes the content. This is
  the default action when the user clicks **Save to Note** without
  picking an existing one.
- **Existing note.** `updateNote(id, existing.title, mergedContent)`
  where `mergedContent` is:
  ```ts
  const heading = `## ${filename}`;
  const sep = existing.content.length === 0 ? "" : "\n\n";
  const mergedContent = `${existing.content}${sep}${heading}\n\n${transcript}`;
  ```
  i.e. always **append** to the bottom, separated by a blank line, with
  the filename as an `## H2` heading. Never overwrites the existing
  body. The title stays untouched.

This is a deliberate one-way merge — there's no "where would you like
to insert this?" picker because it would force a complex caret-aware
editor experience for a v1 feature most users will use once or twice
per import.

## 8. Settings touchpoints

**None.** This is intentional — `AppSettings` is not modified. The
import path:

- Reads `noise_reduction` and respects it (same audio benefit as live).
- Reads `active_model_id` indirectly via the loaded engine (whatever
  model is loaded for live use is what imports use).
- **Ignores** `voice_commands`, `command_send`, `ship_mode`,
  `auto_switch_modes`, `structured_mode`, `use_screen_context`,
  `audio_ducking`, `live_preview`. These are all live-dictation
  concerns.

Rationale: a setting-per-toggle for imports is premature. If a user
asks for "run imports through Structured Mode" we add a per-import
toggle on the page (or move it to a "structure this" action on the
History row), not a global setting.

## 9. Edge cases & failure modes

| Scenario | Behavior |
|---|---|
| User drops a non-audio file (e.g. `.txt`) | symphonia returns `Unsupported`, page shows error state with "Couldn't decode this file format". |
| User drops a corrupted MP3 | symphonia decode error mid-stream → error state with the specific symphonia error message. |
| User drops a 4-hour podcast | Decode succeeds (it's just bytes). Transcription takes ~30 min on a base model. Progress bar reflects whisper percent. No timeout — the user can cancel. **Memory**: a 4-hour mono f32 at 16 kHz is ~920 MB. Add a guard: reject files whose decoded sample count would exceed a configurable cap (default ~1 GB, ≈4.7 hours). |
| User cancels mid-transcription | Cancellation flag is observed by Whisper's abort callback. Whisper may finish the current decode step (~1–2 s on base, longer on large models) before the call returns. State machine returns to `idle`. Nothing is persisted. |
| User starts a live recording mid-import | **Blocked.** `start_recording` returns `ErrorCode::Busy` with "Import in progress". The dictation **record button** (`RecordButton.tsx` / `DictationPanel.tsx`) is also disabled via `useImportStore(s => s.state.kind !== "idle" && s.state.kind !== "complete")` so the user sees the constraint before clicking. Sidebar shows an import-in-progress dot mirroring the recording dot at `Sidebar.tsx:60-64`. Resolves §11.7 in favor of strict mutual exclusion. |
| User triggers an import mid-recording | **Blocked.** `import_transcribe` returns "Stop recording before importing". The drop zone shows a disabled state if a live recording is in progress (read from `useRecordingStore`). |
| User triggers an import while live dictation is processing | **Blocked.** `audio.is_recording()` is false in this phase, so the guard is `state.transcription_gate`. `import_transcribe` returns "Dictation is still processing" and the Import page keeps the selected file in idle/error-ready state so the user can retry. |
| User leaves the Import page mid-transcription | Background continues. State lives in `useImportStore` (§7.2) and the global event listeners are mounted in `App.tsx`, so navigating back to Imports re-renders the live progress bar with no loss. History also auto-refreshes via the `import-transcription-ready` listener. |
| App quits mid-import | Import is lost. Audio file is untouched on disk; the user can re-import. No "resume" support. |
| User imports the same file twice | Allowed. Two history rows with the same `source_filename`. No dedupe. |
| Model not loaded | Same error path as live: emit `recording-error` with `ErrorCode::NoModelLoaded`. Page shows a "Go to Models" CTA. |
| Audio sample rate already 16 kHz | Skip the rubato pass — straight channel downmix only. |
| Audio is 8 kHz (telephony) | Upsample to 16 kHz via rubato. Quality will be limited by source, not by us. Document, don't block. |

## 10. Test plan

**Unit (Rust):**
- `audio::decode::decode_file` against a small WAV / MP3 / M4A fixture
  in `src-tauri/tests/fixtures/`. Assert: sample count matches
  `duration_ms × 16`, channel count is 1, peak is in [-1, 1].
- Import persistence helper test: factor the record construction into a
  small pure helper, e.g. `build_import_record(transcription, filename,
  source_duration_ms)`, and assert it sets `source = "import"`,
  `source_filename`, `source_duration_ms`, final text, model name, and
  raw transcript correctly. Do **not** force `transcribe_with_progress`
  onto the `AsrEngine` trait just to mock this path; §6.4 intentionally
  keeps progress/abort off the trait.
- Migration: open a DB with the pre-import schema (synthesise via
  `CREATE TABLE` without the new columns), run init, assert
  `PRAGMA table_info` reports the new columns and existing data is
  intact.

**Integration (manual, captured as a checklist in the PR):**
- Import a 30s WAV → transcript appears, history row created with
  badge.
- Import a 5-minute M4A → progress bar moves smoothly, cancellation
  works.
- Import a `.txt` → error state shown.
- Import then "Save to Note" → new note created with transcript;
  existing notes unaffected.
- Try to start live dictation while a 10-minute import is running →
  record button shows disabled state with tooltip; clicking via hotkey
  emits a "Busy" error. Same in reverse (import button disabled while
  recording).
- Navigate away from Imports mid-transcription, come back → progress
  bar resumes from current percent (store-backed state survived
  unmount).
- Confirm Notes editor open during import → transcript **does not**
  auto-append (this is the load-bearing behavioral test).
- Save import to an existing non-empty note → existing content
  preserved, transcript appended below a blank line with an
  `## <filename>` heading. Save to an empty note → no leading blank
  line.
- Confirm Ship Mode setting on → import does not press Enter.
- Confirm voice_commands setting on, import containing the phrase "new
  line" → output contains the literal text "new line", not a `\n`.

**No automated UI test infra exists yet** — manual checklist is the
v1 bar.

## 11. Open questions

### Resolved in round 1

- ~~**§11.2 In-progress import survives page navigation.**~~ **Resolved
  in Codex's direction:** state lives in `useImportStore` (§7.2),
  listeners mounted once in `App.tsx`. Navigation no longer drops
  progress.
- ~~**§11.7 Live-dictation concurrency.**~~ **Resolved in Codex's
  direction:** strict mutual exclusion. Live final transcription and
  imports cannot run concurrently; active mic recording is also blocked
  from starting imports. Both directions are enforced server-side with
  UX hints in both pages. See §6.5 + §9.

### Still open

1. **`pipeline.rs` vs `pipeline_import.rs` vs full module split.** I
   proposed the sibling-file option. The only live-pipeline changes are
   the import-visible / transcription-gate guard in `start_recording`
   and acquiring the shared `transcription_gate` permit around final
   Whisper processing.
   That still feels small enough that the sibling-file approach is
   correct for v1; a `pipeline/` folder split can be a follow-up.
2. **Cancel semantics.** Whisper's abort callback is polled between
   decode steps. On large models this is a 5–10 s delay. Do we show
   a "Cancelling…" sub-state, or just leave the spinner running? My
   lean: a "Cancelling…" label appears immediately on click, and the
   store transitions to `idle` when the task actually exits. Cheap
   and honest.
3. **Dictionary / vocabulary scoping.** I locked it to "use whatever is
   active". Should the user pick a context mode per import? My take:
   defer until someone asks.
4. **File-size cap.** 500 MB file / ~1 GB decoded sample buffer. Right
   ballpark? Too conservative? Too permissive?
5. **Should `transcribe_with_progress` replace `transcribe` rather
   than be a parallel method?** Cleaner long-term but adds two
   callback params to the `AsrEngine` trait and forces every mock
   impl to handle them. (Worth noting the trait exists specifically
   for testability — `engine.rs:15-17`.)

## 12. Risks

- **Symphonia decode edge cases.** MP3 variants in the wild are
  weird. Plan: catch decode panics in a `spawn_blocking` boundary, do
  not crash the app. Show the symphonia error verbatim in the error
  state so we get useful bug reports.
- **Whisper progress callback FFI.** `set_progress_callback_safe` is
  marked safe in whisper-rs but historically had soundness issues
  across versions. Verify on the pinned `whisper-rs = "0.16"` version
  before relying on it; fall back to a phase-only indeterminate bar if
  it misbehaves.
- **Binary size bloat from symphonia.** Adds ~1.5 MB. Acceptable but
  worth confirming with a release-build size diff in the PR.
- **No drag-and-drop on Linux Wayland.** Tauri's file drop event
  is fiddly on some Linux compositors. v1 ships drag-and-drop on
  Windows / macOS and the picker as the universal fallback.
- **Long files spike memory.** 4.7 hours × 16 kHz × 4 bytes = ~1 GB
  audio buffer in addition to whisper's ~500 MB decode state. The
  decoded-sample cap (§9) is the mitigation.

## 13. Milestones (suggested implementation order)

1. **Schema + types.** Migration, `TranscriptionRecord` extension,
   `tauri.ts` type mirror, badge rendering on History page (no new
   data flowing yet — verify the migration is invisible to existing
   users).
2. **Decode module.** `audio/decode.rs` + symphonia/rubato deps + a
   `cargo test` against a small fixture WAV/MP3.
3. **Progress-aware engine method.** `transcribe_with_progress` +
   throttling. Test by calling it directly from a temporary Tauri
   command and watching the event stream.
4. **Import pipeline + commands.** `pipeline_import.rs` +
   `commands/imports.rs` + `AppState.active_import` + event wiring.
   No UI yet — drive end-to-end from the devtools console.
5. **Imports page UI.** State machine, drop zone, picker, progress
   bar, complete-state textarea, Save to Note popover.
6. **HistoryPage badge + listener.** Render the "imported" badge,
   listen for `import-transcription-ready` to auto-refresh.
7. **Manual QA checklist** from §10 + capture screenshots in the PR.

Each milestone is a self-contained commit; the feature is invisible
to users until milestone 5 lands.

---

## Appendix: file inventory

**New files:**
- `src-tauri/src/audio/decode.rs`
- `src-tauri/src/pipeline_import.rs`
- `src-tauri/src/commands/imports.rs`
- `src-tauri/tests/fixtures/sample_30s.wav` (and `.mp3`)
- `src/features/imports/ImportsPage.tsx`
- `src/stores/importStore.ts`

**Modified files:**
- `src-tauri/Cargo.toml` — add symphonia, rubato, tauri-plugin-dialog
- `src-tauri/src/storage/database.rs` — `migrate_add_import_metadata`
- `src-tauri/src/storage/types.rs` — extend `TranscriptionRecord`
- `src-tauri/src/storage/history.rs` — update SELECTs + `row_to_record`
- `src-tauri/src/state.rs` — add `active_import:
  Arc<Mutex<Option<ImportState>>>` and `transcription_gate:
  Arc<tokio::sync::Semaphore>`
- `src-tauri/src/lib.rs` — register imports commands + dialog plugin
- `src-tauri/src/asr/engine.rs` — add `transcribe_with_progress`
- `src-tauri/src/pipeline.rs` — guard `start_recording` when
  `state.active_import` is occupied or the transcription gate is held,
  and acquire `state.transcription_gate` around final Whisper processing
  in `stop_and_transcribe` (see §6.5).
- `src-tauri/src/error.rs` — add `ErrorCode::Busy` and
  `AppError::Cancelled` variants
- `src/app/Sidebar.tsx` — new nav item, plus an optional
  import-in-progress dot near the existing recording dot at
  `Sidebar.tsx:60-64`. **No** record button lives in the sidebar
  (it never did).
- `src/features/dictation/RecordButton.tsx` (and/or
  `DictationPanel.tsx`) — reads `useImportStore(s => s.state.kind !==
  "idle" && s.state.kind !== "complete")` and renders a disabled +
  tooltipped state when an import is in flight.
- `src/App.tsx` — new page routing branch + mount the
  `onImportProgress` / `onImportTranscriptionReady` listeners that
  write to `importStore`
- `src/stores/appStore.ts` — `Page` type adds `"imports"`
- `src/lib/tauri.ts` — types + bindings + `TranscriptionRecord` extension
- `src/features/history/HistoryPage.tsx` — listener + badge
- `tauri.conf.json` / capability files — `dialog:allow-open`; verify
  `dragDropEnabled` remains enabled for the main webview (Tauri drag
  drop is config-driven, not a separate capability permission)

**Not modified (deliberately):**
- `src-tauri/src/storage/settings.rs` + `AppSettings` — no new settings
- `src/features/notes/NotesPage.tsx` — auto-append behavior unchanged
