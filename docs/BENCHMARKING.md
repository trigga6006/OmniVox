# Local inference benchmarks

OmniVox has three release-mode before/after harnesses for measuring the latency
and output quality of the production ASR, Command Mode, and Structured Mode
paths:

| Harness | Production path | Quality signal |
| --- | --- | --- |
| `asr_bench` | `WhisperEngine::load` + `transcribe` | Hallucination-free synthetic fixtures; not WER |
| `command_ab` | `LlmRunner::classify_command_with_timeout` | Exact ordered intent-chain accuracy plus decline specificity |
| `extraction_ab` | Profile session + production postprocessor | Grounded expected/rejected-content pass rate |

Two candidate-only harnesses isolate optimizations that the byte-identical
before/after harnesses deliberately cannot exercise:

| Harness | Comparison |
| --- | --- |
| `asr_optimization_ab` | Fresh/reused state crossed with configured/latency-first policy; gates the actual production paths separately |
| `command_routing_ab` | Exact tier-0, single matcher, and app-resolution stages; incomplete LLM fallthrough is separate |

All harnesses write schema-versioned JSON to stdout and optionally to `--json`.
Progress goes to stderr, so stdout may be redirected without cleanup. Reports
include source revision/dirty state, hashes of both harness bytes and the
labeled production-source bytes compiled into it, requested/compiled backend
metadata, hardware label, effective model settings, model SHA-256, cold and
warm samples, p50/p95 latency, quality details, and threshold results.

Backend metadata deliberately separates `requested` from `verified_actual`.
For CPU-configured model paths, actual CPU execution is verified by the disabled
GPU configuration. For Vulkan, successful model load is not proof of offload:
the current llama/whisper bindings expose no reliable executed-device or
offloaded-layer result, so reports say `unknown` and include the evidence. The
candidate routing harness says `not_exercised` because it never loads a model.

## Run a benchmark

The npm wrappers keep commands discoverable. Extra arguments after `--` are
forwarded to the Rust harness:

```powershell
npm run bench:asr -- --model "C:\models\ggml-base.en.bin" --backend cpu --runs 5 --warmups 2 --json "benchmarks\results\asr-cpu.json" --source-label candidate --hardware-label devbox

npm run bench:command -- --model "C:\models\Qwen3-1.7B-Q8_0.gguf" --backend cpu --runs 3 --warmups 2 --json "benchmarks\results\command-cpu.json" --source-label candidate --hardware-label devbox

npm run bench:structured -- --model "C:\models\Qwen3-1.7B-Q8_0.gguf" --backend cpu --runs 3 --warmups 2 --json "benchmarks\results\structured-cpu.json" --source-label candidate --hardware-label devbox

npm run bench:asr-optimization -- --model "C:\models\ggml-base.en.bin" --backend cpu --runs 3 --warmups 2 --json "benchmarks\results\asr-production-paths.json" --source-label candidate --hardware-label devbox

npm run bench:command-routing -- --model "C:\models\Qwen3-1.7B-Q8_0.gguf" --backend cpu --runs 1000 --warmups 10 --json "benchmarks\results\command-routing.json" --source-label candidate --hardware-label devbox
```

For the shipping Vulkan path, invoke Cargo with the feature enabled:

```powershell
cargo run --release --manifest-path src-tauri\Cargo.toml --features vulkan --example asr_bench -- --model "C:\models\ggml-base.en.bin" --backend vulkan --runs 5 --warmups 2 --json "benchmarks\results\asr-vulkan.json" --source-label candidate --hardware-label devbox
```

`--backend vulkan` is rejected unless the harness was compiled with
`--features vulkan`; this prevents a requested-Vulkan run from using a binary
without Vulkan support, but does not relabel the unverified actual backend.
Model paths can instead be provided by `OMNIVOX_ASR_MODEL` or
`OMNIVOX_LLM_MODEL`. GPU names
are detected best-effort; set `OMNIVOX_GPU_IDENTIFIER` when a driver utility
is unavailable or when a stable lab-machine label is preferable.

## Compare a baseline fairly

Use the same harness bytes, model file, backend, power mode, machine, audio
device state, run/warmup counts, and model configuration for both trees. The
report's `harness_sha256`, `production.sha256`, labeled `production.files`, and
`model.sha256` make accidental drift visible.

1. Create a clean worktree at the baseline revision.
2. Apply only `src-tauri/examples/{asr_bench,command_ab,extraction_ab}.rs` and
   `src-tauri/examples/support/**` from the candidate to that worktree.
3. Build release binaries in both trees before collecting numbers.
4. Alternate baseline and candidate runs to reduce thermal/order bias. Close
   other GPU-heavy applications and keep the machine on the same power plan.
5. Save reports outside either source tree, or under the ignored
   `benchmarks/results/` directory.

`model_load` is cold with respect to the benchmark process. It is not a claim
of an empty operating-system file cache. Rebooting or deliberately flushing
the cache is rarely representative of the app's normal first-use path, but if
you do it, apply exactly the same procedure to both sides and record it beside
the JSON reports.

Command and Structured reports retain model-load, session/runner-ready, and
first-inference components and also emit `cold_end_to_end`. That sum is assessed
against the app's default 8-second deadline; a miss is a failed threshold even
when no optional gate was supplied, because production would degrade/fall back.
Command quality scores the complete ordered `Vec<CommandIntent>`: missing,
extra, or reordered steps fail. Decline quality is correctly named
`specificity_pct` (true declines divided by expected-negative cases).

Synthetic ASR fixtures are generated mathematically at 16 kHz with fixed
1/5/10/15-second durations and PCM hashes. They avoid private recordings and
make performance runs reproducible. Because they contain no lexical ground
truth, the ASR quality metric only detects hallucinated text; use an approved,
versioned speech corpus for WER claims.

Warmups are deliberately distinct from the measured corpus: ASR uses a fixed
3-second synthetic fixture, while Command and Structured use fixed unmeasured
utterances. This avoids making the first measured case an exact warmup repeat.

The standalone harnesses measure model load/session/inference behavior, not the
whole desktop interaction. They intentionally exclude hotkey dispatch,
microphone/device startup, capture resampling and denoise, ASR queue wait,
foreground restoration, clipboard/keystroke output, Tauri event delivery, and
WebView paint. Do not describe a harness p95 as "press-to-text" latency.

`extraction_ab` is explicitly a model/session/profile/postprocess benchmark.
It excludes Structured Mode screen-context capture and prompt merge: UIA capture
depends on a live foreground window and normally overlaps recording, so a
synthetic context benchmark would be misleading. Use the content-free production
field traces for that path. `command_routing_ab` similarly reports tier-0,
single matcher, cached app-resolution, and LLM-fallback stages separately.
App-index timings are machine-dependent, non-gated diagnostics; fallthrough is
marked incomplete with unknown LLM latency, never as a zero-latency completion.

## Inspect production field traces

The main WebView can call `getPipelineTraces()` from `src/lib/tauri.ts` (the
underlying main-window-only command is `get_pipeline_traces`). It returns the
last 100 content-free capture traces. A useful development probe is:

```ts
import { getPipelineTraces } from "@/lib/tauri";
console.table((await getPipelineTraces()).slice(-10));
```

`claim_to_mic_live_ms` covers the backend capture claim through successful
microphone startup. OS hotkey dispatch before that claim is outside the field;
do not describe it as key-press-to-mic latency. All stage offsets beginning
with `stop_received` are from the original stop request, so async scheduling
delay is included.
`stop_to_visible_delivery_ms` ends at the first successful backend delivery:
completed OS output, an emitted in-app insertion event, an emitted Structured
panel event, or an emitted Command result/confirmation. For WebView routes,
`visible_delivery_kind` explicitly says `*_event_emitted`; browser paint time is
outside the Rust backend and is not claimed. A null delivery field means no
successful delivery was observed (for example silence, supersession, or output
failure). Traces contain timings, model/backend labels, generation, mode, and
outcome only—never audio, transcript, command summary, or generated text.

The candidate-only reports must not be described as historical baseline runs:
both sides execute in the candidate binary so they can hold fixtures and model
configuration constant while isolating the production-path change. Use the
three shared harnesses for revision-to-revision claims, and the candidate-only
harnesses to explain where the measured savings come from.

## Regression gates

The optional gates make a harness exit non-zero when a candidate misses an
agreed budget:

```powershell
npm run bench:command -- --model "C:\models\Qwen3-1.7B-Q8_0.gguf" --backend cpu --runs 3 --max-p95-ms 900 --min-quality-pct 95
```

- Exit `0`: all supplied thresholds passed.
- Exit `1`: a latency or quality threshold failed; JSON is still emitted.
- Exit `2`: invalid arguments, model load/inference failure, or report I/O
  failure.

Do not copy absolute latency budgets between unlike machines. Establish each
budget from a checked-in release baseline on the target hardware class, then
gate on an explicit tolerated regression.

For `asr_optimization_ab`, `--max-p95-ms` is deliberately not calculated from
all four paths. It creates two independent thresholds: the primary
`reused_latency_first_p95_ms` production metric (used by Command Mode), and
`fresh_configured_p95_ms` (used by regular dictation). Configured decoding can
enter stochastic temperature fallback, whose RNG belongs to `WhisperState`, so
regular dictation intentionally creates a fresh state per utterance instead of
carrying RNG state across requests. Both production paths must pass. The
`reused_configured` and `fresh_latency_first` paths remain in `phases`,
`stage_summaries`, and raw `trials` for diagnosis, but cannot dilute or hide a
production-path regression.

## Developer checks

```powershell
npm run check:quick   # TypeScript, frontend tests, Rust formatting
npm run check         # Frontend build/tests plus Rust check/tests
npm run check:vulkan  # Clippy across all targets with the shipping backend
```

CI also formats Rust and runs Clippy with `--features vulkan`, matching the
backend used by Windows release builds. The release job uses the Tauri CLI
version locked in `package-lock.json` rather than installing a floating CLI.
