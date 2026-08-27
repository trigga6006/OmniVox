# OmniVox optimization and benchmark report — 2026-07-28

## Outcome

This pass optimized the complete local dictation pipeline: microphone startup,
audio conversion, ASR scheduling and session reuse, deterministic Command Mode,
local LLM session lifecycle, Structured Mode delivery, output/focus handling,
model integrity and switching, settings synchronization, retention/privacy, and
content-free field telemetry.

The clearest measured inference gain is Command Mode: warm p50 improved by
33.5–53.4% and warm p95 by 16.7–36.8%, with equal or better exact ordered-chain
quality. Structured Mode retained identical quality and is essentially flat at
warm inference latency (p95 changes from -0.7% to +2.0%), while its cold
end-to-end path improved by 2.4–5.8%. The fair configured-ASR harness is also
essentially flat, but the candidate-only production-path isolation shows that
retaining a Whisper session cuts the latency-first ASR p95 by 20.5–58.3% for
all five installed models. Latency-first decoding is used where the application
can do so safely (notably Command Mode); regular dictation keeps its configured
quality policy and a fresh state per utterance because this synthetic suite
cannot establish WER parity for either decode-policy or RNG-state changes.

## Test method

- Historical baseline: `fdc7edf0795f869052e88366c98e8284392b8a07`
- Candidate branch: `codex/omnivox-full-optimization`
- Machine: Windows x86-64, Ryzen 9 7900X (24 logical CPUs), Radeon RX 7800 XT,
  32 GB RAM
- Models: the same five installed Whisper binaries and the same Qwen3 0.6B/1.7B
  Q4_K_M files, verified by SHA-256 in every report
- Repetitions: 3 measured runs after 2 distinct warmups for every fair report;
  3 measured runs after 1 warmup for each candidate-only ASR path; 1,000 runs
  after 10 warmups for deterministic routing
- Execution: serial workloads; baseline/candidate order alternated between
  configurations; frozen release executables; identical schema-v2 fair harness
  and model hashes
- Backend: shipping paths requested Vulkan and were compiled with Vulkan. The
  current llama.cpp/whisper bindings cannot prove executed-device/offload count,
  so those reports correctly say `verified_actual: unknown`. The CPU controls
  force zero GPU layers and report `verified_actual: cpu`.
- Environment caveat: an already-running installed `omnivoice.exe` (PID 31392,
  about 2.59 GiB private bytes when checked) was left untouched throughout. This
  is not an empty-machine benchmark, but it is a consistent resident-app load
  across alternating baseline and candidate runs.

All 11 baseline/candidate report pairs passed automated validation: schema,
fair-harness hash, model hash, configuration, hardware label, source label, and
threshold outcome match as required. All 6 candidate-only reports also passed.
There were 28 successful reports and no benchmark process failures.

Raw JSON, stdout, stderr, and frozen executables are under:

- `C:\t\omnivox-bench-results\baseline-v2`
- `C:\t\omnivox-bench-results\candidate-v2`
- `C:\t\omnivox-bench-results\candidate-only-v2`

## Fair before/after results

Positive delta means faster. Times are milliseconds.

### Local ASR — configured production policy

The warm p50/p95 columns combine all 1/5/10/15-second synthetic-fixture samples
(12 measured samples per revision/model). Synthetic output quality detects
hallucination/path drift; it is not WER.

| Model | Load baseline → candidate | Warm p50 baseline → candidate | Warm p95 baseline → candidate | Quality |
| --- | ---: | ---: | ---: | ---: |
| base.en | 298 → 302 (-1.3%) | 64 → 67 (-4.7%) | 452 → 459 (-1.5%) | 25.0% → 25.0% |
| small.en | 561 → 564 (-0.5%) | 144 → 145 (-0.7%) | 1027 → 1043 (-1.6%) | 25.0% → 25.0% |
| medium.en | 1374 → 1340 (+2.5%) | 292 → 288 (+1.4%) | 1811 → 1776 (+1.9%) | 0.0% → 0.0% |
| distil-large-v3 | 1356 → 1419 (-4.6%) | 207 → 208 (-0.5%) | 239 → 237 (+0.8%) | 0.0% → 0.0% |
| large-v3-turbo | 1443 → 1400 (+3.0%) | 196 → 193 (+1.5%) | 208 → 208 (0.0%) | 0.0% → 0.0% |

Configured ASR inference is statistically flat in this small synthetic run.
The important production wins not represented by this table are bounded
latest-only scheduling, stale-inference cancellation, zero unbounded audio
backlog, capture-time conversion improvements, safe deterministic session reuse, and
lower-latency decoding for Command Mode. The candidate-only table below isolates
the retained-session/decoding effects.

### Command Mode — production LLM path

Quality is exact ordered intent-chain accuracy over 123 measured cases per
revision/configuration, including multi-step chains and declines. Cold E2E is
model load + ready/profile warm + first inference and remained under the
production 8-second deadline in every run.

| Configuration | Cold E2E baseline → candidate | Warm p50 baseline → candidate | Warm p95 baseline → candidate | Exact-chain quality |
| --- | ---: | ---: | ---: | ---: |
| Qwen3 0.6B Q4, Vulkan requested | 3685 → 3613 (+2.0%) | 457 → 266 (+41.8%) | 868 → 723 (+16.7%) | 75.61% → 75.61% |
| Qwen3 1.7B Q4, Vulkan requested | 1890 → 1820 (+3.7%) | 556 → 259 (+53.4%) | 903 → 571 (+36.8%) | 85.37% → 85.37% |
| Qwen3 1.7B Q4, verified CPU | 3135 → 3076 (+1.9%) | 1145 → 761 (+33.5%) | 1821 → 1460 (+19.8%) | 85.37% → 87.80% |

### Structured Mode — profile/session/postprocessor path

Quality is grounded expected/rejected-content pass rate over 84 measured cases
per revision/configuration. Screen-context capture and WebView paint are not
synthetically included; production field traces cover the backend portions of
that path without recording content.

| Configuration | Cold E2E baseline → candidate | Warm p50 baseline → candidate | Warm p95 baseline → candidate | Grounded quality |
| --- | ---: | ---: | ---: | ---: |
| Qwen3 0.6B Q4, Vulkan requested | 4725 → 4613 (+2.4%) | 2211 → 2290 (-3.6%) | 4558 → 4485 (+1.6%) | 96.43% → 96.43% |
| Qwen3 1.7B Q4, Vulkan requested | 2060 → 2009 (+2.5%) | 1876 → 1909 (-1.8%) | 3471 → 3400 (+2.0%) | 92.86% → 92.86% |
| Qwen3 1.7B Q4, verified CPU | 3575 → 3368 (+5.8%) | 3607 → 3648 (-1.1%) | 5605 → 5647 (-0.7%) | 92.86% → 92.86% |

Structured warm inference is effectively unchanged at this sample size because
the baseline harness already retained three prefix-warmed 4096-token profile
sessions. The candidate removed one redundant context-allocation probe, which
explains the consistent cold-path improvement; paired warm samples split nearly
evenly between faster and slower. The remaining wins are lifecycle and
end-to-end UX improvements: ready handshakes, bounded deadlines and
cancellation, model-switch rollback, stable mounted panel capability, exact
generation routing, and safe pre-claim validation. Further token/context
reductions would need a representative private-safe quality corpus; they were
not guessed from synthetic prompts.

## Candidate-only causal measurements

These are not historical before/after claims. Both sides run in the candidate
binary to isolate production-path choices while holding fixtures and model
configuration constant.

### Fresh configured dictation and retained latency-first Command Mode

The fixture is a fixed synthetic 10-second input. `Fresh fast → retained fast`
isolates session reuse. The regular-dictation and Command columns show the two
actual production policies, but their difference also includes decoding policy
and therefore is not an accuracy-neutral optimization claim. Regular dictation
deliberately remains configured and uses a fresh state; `retained configured`
is kept only as a diagnostic because temperature fallback samples from RNG
state owned by the retained `WhisperState`.

| Model | Fresh fast p95 → retained fast p95 | Session-reuse gain | Regular dictation p95 (fresh configured) | Command ASR p95 (retained fast) |
| --- | ---: | ---: | ---: | ---: |
| base.en | 48 → 20 | 58.3% | 64 | 20 |
| small.en | 75 → 39 | 48.0% | 958 | 39 |
| medium.en | 170 → 122 | 28.2% | 1489 | 122 |
| distil-large-v3 | 166 → 132 | 20.5% | 209 | 132 |
| large-v3-turbo | 175 → 132 | 24.6% | 194 | 132 |

The retained-configured diagnostic did not preserve fresh-configured output
hashes for one of three small-model runs or any of the three turbo runs. This
synthetic fixture cannot quantify lexical accuracy, but it proves that carrying
stochastic fallback state is not semantically neutral. Its three-sample p95 was
also noisy. Production therefore keeps configured dictation fresh and retains
only deterministic latency-first Command Mode state.

### Deterministic command routing

Across 47,000 routing trials:

- tier-0 check p95: 0.6 µs; 6,000/6,000 tier-0 cases hit
- single matcher-call p95: 1.5 µs; at most one matcher call per trial
- whole routed-corpus p95: 4 ms, including machine-dependent app resolution
- deterministic coverage: 63.41%; safe-route rate: 87.80%
- matched full-chain precision: 80.77%; negative specificity: 100%
- unresolved/ambiguous cases are explicitly marked as LLM fallthrough with
  unknown routing-harness latency; the fair Command table supplies model cost

The 838 ms first app-index snapshot is now started after microphone capture is
live, so it does not extend claim-to-mic startup.

## What changed

### Hot path and responsiveness

- Retained deterministic latency-first Whisper state for consecutive Command
  Mode requests; configured dictation keeps fresh-per-utterance state.
- Added one in-flight plus one replaceable pending ASR buffer, stale-generation
  cancellation, and transition locks around reset/switch/delete.
- Optimized capture conversion/downmix, denoise/VAD, normalization, and resample
  paths; auto-switch database/process work now begins after microphone startup.
- Added persistent LLM runner sessions, startup-ready handshake, batch threads,
  cancellation/deadlines, deterministic fast routing, and bounded fallbacks.
- Kept capture generations and delivery generations separate so a new recording
  cannot erase an older valid completion.

### UX correctness and reliability

- Structured panel remains mounted while hidden; capabilities and pending output
  survive transient visibility changes.
- Ship Mode paste + Enter is one serialized, target-reverified transaction.
- Hotkey remapping cancels a starting/live capture rather than orphaning the mic.
- Settings use field-level patches, monotonic revisions, rebase, and rollback.
- Model switches unload before loading the replacement and roll back coherently
  on failure instead of briefly retaining two large engines.

### Integrity, privacy, and security

- Exact immutable model revisions, expected sizes, SHA-256 verification, atomic
  temporary downloads, and legacy verification/migration.
- Split main/overlay/scratchpad capabilities and exact caller-window allowlists.
- One-time generation/HWND/PID Structured bindings; fail-closed macOS target
  restoration with PID verification.
- History database v4 exact counts/O(1) stats, synchronized retention cleanup,
  secure delete/WAL truncation, and a 30-day default for new installs while
  preserving legacy unlimited settings.
- Typed/redacted/bounded screen-context and LLM diagnostics; no free-form content
  is accepted by the logging surface.
- Content-free bounded pipeline traces expose claim-to-mic-live and
  stop-to-backend-delivery without audio, transcripts, prompts, or generated text.

## Verification

- Frontend typecheck: pass
- Frontend tests: 38 passed across 7 files
- Frontend production build: pass (1,663 modules)
- Core Rust tests: 472 passed, 1 ignored, 0 failed; benchmark-example suites:
  21 passed, 0 failed
- Rust formatting and `git diff --check`: pass
- Strict Clippy (`-D warnings`) on default and Vulkan feature sets: pass
- `npm audit`: 0 vulnerabilities
- `cargo audit`: 0 vulnerabilities; 17 unmaintained, 5 unsoundness, and 1 yanked
  upstream/transitive notices remain tracked by `cargo-deny` policy
- `cargo deny` advisories/bans/licenses/sources: pass
- `cargo machete`: no unused direct dependencies

The release workflow and installer now enforce pinned tooling and external
signing inputs. A real signing certificate, publisher thumbprint, and repository
secrets are environment prerequisites and therefore were not fabricated or
exercised in this local pass.

## Interpretation limits

- Synthetic ASR fixtures have no words, so they cannot support WER or human
  dictation-quality claims.
- Standalone inference harnesses exclude OS hotkey dispatch, microphone/device
  startup, live capture preprocessing, queue wait, foreground restoration,
  clipboard/keystroke delivery, Tauri event transit, and WebView paint.
- Production traces end at completed OS output or emitted WebView events; an
  emitted event is not claimed as browser paint completion.
- Vulkan-requested reports do not claim verified GPU offload because the current
  bindings do not expose evidence sufficient to make that assertion.
