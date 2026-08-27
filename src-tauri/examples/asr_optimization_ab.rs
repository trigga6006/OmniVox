//! Candidate-only microbenchmark for isolating final-ASR optimizations.
//!
//! This is intentionally separate from `asr_bench`: the fair baseline harness
//! is byte-frozen across source trees, while this example calls candidate-only
//! APIs to cross fresh/reused decode state with configured/latency-first decode.

mod support;
#[path = "support/synthetic_audio.rs"]
mod synthetic_audio;

use omnivoice_lib::asr::engine::{WhisperEngine, WhisperSession};
use omnivoice_lib::asr::types::{AsrConfig, TranscriptionOptions, TranscriptionResult};
use serde::Serialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const USAGE: &str = "Candidate ASR optimization microbenchmark

Usage:
  cargo run --release --example asr_optimization_ab -- --model <model.bin> --backend cpu [options]
  cargo run --release --example asr_optimization_ab --features vulkan -- --model <model.bin> --backend vulkan [options]

Options:
  --runs <n>                Measured repetitions per path (default: 1)
  --warmups <n>             Unmeasured repetitions per path (default: 1)
  --json <path>             Also write the JSON report to this path
  --max-p95-ms <ms>         Fail when either production path p95 exceeds this value;
                            reused_latency_first is primary, fresh_configured is secondary
  --min-quality-pct <pct>   Fail when synthetic hallucination-free rate is below this value
  --source-label <label>    Label this source tree (candidate is recommended)
  --hardware-label <label>  Stable identifier for the benchmark machine

The model may also be supplied through OMNIVOX_ASR_MODEL.";

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
enum PathKind {
    FreshConfigured,
    ReusedConfigured,
    FreshLatencyFirst,
    ReusedLatencyFirst,
}

impl PathKind {
    const ALL: [Self; 4] = [
        Self::FreshConfigured,
        Self::ReusedConfigured,
        Self::FreshLatencyFirst,
        Self::ReusedLatencyFirst,
    ];

    const fn label(self) -> &'static str {
        match self {
            Self::FreshConfigured => "fresh_configured",
            Self::ReusedConfigured => "reused_configured",
            Self::FreshLatencyFirst => "fresh_latency_first",
            Self::ReusedLatencyFirst => "reused_latency_first",
        }
    }
}

#[derive(Serialize)]
struct TrialResult {
    run: usize,
    path: PathKind,
    wall_ms: u64,
    realtime_factor: f64,
    state_setup_us: u64,
    inference_us: u64,
    result_extraction_us: u64,
    observed_text: String,
    observed_text_sha256: String,
    observed_chars: usize,
    hallucination_free: bool,
    matches_fresh_configured_for_run: bool,
}

#[derive(Serialize)]
struct MicroSummary {
    count: usize,
    mean_us: f64,
    p50_us: u64,
    p95_us: u64,
}

#[derive(Serialize)]
struct PathStageSummary {
    path: PathKind,
    wall: MicroSummary,
    state_setup: MicroSummary,
    inference: MicroSummary,
    result_extraction: MicroSummary,
    distinct_output_hashes: Vec<String>,
}

fn production_latency_thresholds(
    path_p95_ms: &BTreeMap<PathKind, u64>,
    maximum: Option<u64>,
) -> Result<Vec<support::ThresholdResult>, String> {
    let Some(maximum) = maximum else {
        return Ok(Vec::new());
    };
    // Primary first: Command Mode retains deterministic latency-first state.
    // Configured dictation uses a fresh state so temperature-fallback RNG does
    // not carry across utterances. Diagnostic paths cannot dilute either gate.
    [PathKind::ReusedLatencyFirst, PathKind::FreshConfigured]
        .into_iter()
        .map(|path| {
            let actual = path_p95_ms
                .get(&path)
                .copied()
                .ok_or_else(|| format!("missing production-path samples for {}", path.label()))?;
            Ok(support::ThresholdResult {
                metric: format!("{}_p95_ms", path.label()),
                operator: "<=",
                expected: maximum as f64,
                actual: actual as f64,
                passed: actual <= maximum,
            })
        })
        .collect()
}

fn summarize_us(samples: &[u64]) -> MicroSummary {
    if samples.is_empty() {
        return MicroSummary {
            count: 0,
            mean_us: 0.0,
            p50_us: 0,
            p95_us: 0,
        };
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let percentile = |pct: f64| {
        let rank = (pct * sorted.len() as f64).ceil() as usize;
        sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
    };
    MicroSummary {
        count: sorted.len(),
        mean_us: sorted.iter().map(|&value| value as u128).sum::<u128>() as f64
            / sorted.len() as f64,
        p50_us: percentile(0.50),
        p95_us: percentile(0.95),
    }
}

fn deterministic_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len().saturating_mul(2).min(u32::MAX as usize) as u32;
    let mut wav = Vec::with_capacity(44 + data_len as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36_u32.saturating_add(data_len)).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes());
    wav.extend_from_slice(&synthetic_audio::SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(synthetic_audio::SAMPLE_RATE * 2).to_le_bytes());
    wav.extend_from_slice(&2_u16.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    for sample in samples {
        let pcm = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16;
        wav.extend_from_slice(&pcm.to_le_bytes());
    }
    wav
}

fn transcribe_path(
    engine: &WhisperEngine,
    configured_session: &mut WhisperSession,
    latency_session: &mut WhisperSession,
    audio: &[f32],
    path: PathKind,
) -> Result<(u64, TranscriptionResult), String> {
    let configured = TranscriptionOptions::configured();
    let latency_first = TranscriptionOptions::latency_first(true);
    let started = Instant::now();
    let result = match path {
        PathKind::FreshConfigured => engine.transcribe_with_options(audio, &configured),
        PathKind::ReusedConfigured => {
            engine.transcribe_with_session(configured_session, audio, &configured)
        }
        PathKind::FreshLatencyFirst => engine.transcribe_with_options(audio, &latency_first),
        PathKind::ReusedLatencyFirst => {
            engine.transcribe_with_session(latency_session, audio, &latency_first)
        }
    }
    .map_err(|error| error.to_string())?;
    Ok((support::elapsed_ms(started), result))
}

fn run() -> Result<bool, String> {
    let args = support::parse_common_args("OMNIVOX_ASR_MODEL", USAGE)?;
    let mut config = AsrConfig::default();
    config.model_path = args.model_path.to_string_lossy().into_owned();
    config.language = Some("en".to_string());
    config.translate = false;
    config.use_gpu = args.backend.use_gpu();
    config.initial_prompt = None;
    config.beam_size = Some(if config.use_gpu { 5 } else { 2 });

    eprintln!(
        "[asr_optimization_ab] loading {} on {:?}...",
        args.model_path.display(),
        args.backend
    );
    let started = Instant::now();
    let engine = WhisperEngine::load(config.clone()).map_err(|error| error.to_string())?;
    let load_ms = support::elapsed_ms(started);

    let fixture = synthetic_audio::fixtures()
        .into_iter()
        .find(|fixture| fixture.duration_ms == 10_000)
        .ok_or_else(|| "missing deterministic 10-second fixture".to_string())?;
    let wav = deterministic_wav(&fixture.samples);
    let wav_sha256 = support::harness_sha256(&[wav.as_slice()]);
    let warmup_fixture = synthetic_audio::warmup_fixture();

    let started = Instant::now();
    let mut configured_session = engine.create_session().map_err(|error| error.to_string())?;
    let configured_session_setup_ms = support::elapsed_ms(started);
    let started = Instant::now();
    let mut latency_session = engine.create_session().map_err(|error| error.to_string())?;
    let latency_session_setup_ms = support::elapsed_ms(started);

    for _ in 0..args.warmups {
        for path in PathKind::ALL {
            let _ = transcribe_path(
                &engine,
                &mut configured_session,
                &mut latency_session,
                &warmup_fixture.samples,
                path,
            )?;
        }
    }

    let mut trials = Vec::with_capacity(args.runs * PathKind::ALL.len());
    for run in 0..args.runs {
        eprintln!(
            "[asr_optimization_ab] measured run {}/{}",
            run + 1,
            args.runs
        );
        // Rotate the first path each run to reduce deterministic order bias.
        for offset in 0..PathKind::ALL.len() {
            let path = PathKind::ALL[(run + offset) % PathKind::ALL.len()];
            let (wall_ms, result) = transcribe_path(
                &engine,
                &mut configured_session,
                &mut latency_session,
                &fixture.samples,
                path,
            )?;
            let observed_text_sha256 = support::harness_sha256(&[result.text.as_bytes()]);
            trials.push(TrialResult {
                run,
                path,
                wall_ms,
                realtime_factor: wall_ms as f64 / fixture.duration_ms as f64,
                state_setup_us: result.timings.state_setup_us,
                inference_us: result.timings.inference_us,
                result_extraction_us: result.timings.result_extraction_us,
                observed_chars: result.text.chars().count(),
                hallucination_free: result.text.trim().is_empty(),
                observed_text: result.text,
                observed_text_sha256,
                matches_fresh_configured_for_run: false,
            });
        }
    }

    let reference_hashes = trials
        .iter()
        .filter(|trial| trial.path == PathKind::FreshConfigured)
        .map(|trial| (trial.run, trial.observed_text_sha256.clone()))
        .collect::<BTreeMap<_, _>>();
    for trial in &mut trials {
        trial.matches_fresh_configured_for_run = reference_hashes
            .get(&trial.run)
            .is_some_and(|reference| reference == &trial.observed_text_sha256);
    }

    let mut phases = vec![
        support::PhaseReport::new("model_load", "cold", vec![load_ms]),
        support::PhaseReport::new(
            "configured_session_setup",
            "cold",
            vec![configured_session_setup_ms],
        ),
        support::PhaseReport::new(
            "latency_first_session_setup",
            "cold",
            vec![latency_session_setup_ms],
        ),
    ];
    let mut stage_summaries = Vec::new();
    let mut path_p95_ms = BTreeMap::new();
    for path in PathKind::ALL {
        let path_trials = trials
            .iter()
            .filter(|trial| trial.path == path)
            .collect::<Vec<_>>();
        let wall_ms = path_trials
            .iter()
            .map(|trial| trial.wall_ms)
            .collect::<Vec<_>>();
        let wall_us = wall_ms
            .iter()
            .map(|milliseconds| milliseconds.saturating_mul(1_000))
            .collect::<Vec<_>>();
        let state_setup_us = path_trials
            .iter()
            .map(|trial| trial.state_setup_us)
            .collect::<Vec<_>>();
        let inference_us = path_trials
            .iter()
            .map(|trial| trial.inference_us)
            .collect::<Vec<_>>();
        let result_extraction_us = path_trials
            .iter()
            .map(|trial| trial.result_extraction_us)
            .collect::<Vec<_>>();
        let distinct_output_hashes = path_trials
            .iter()
            .map(|trial| trial.observed_text_sha256.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let wall = summarize_us(&wall_us);
        path_p95_ms.insert(path, wall.p95_us / 1_000);
        phases.push(support::PhaseReport::new(path.label(), "warm", wall_ms));
        stage_summaries.push(PathStageSummary {
            path,
            wall,
            state_setup: summarize_us(&state_setup_us),
            inference: summarize_us(&inference_us),
            result_extraction: summarize_us(&result_extraction_us),
            distinct_output_hashes,
        });
    }

    let hallucination_free = trials
        .iter()
        .filter(|trial| trial.hallucination_free)
        .count();
    let reference_matches = trials
        .iter()
        .filter(|trial| trial.matches_fresh_configured_for_run)
        .count();
    let total = trials.len();
    let hallucination_free_pct = 100.0 * hallucination_free as f64 / total as f64;
    let latency_thresholds = production_latency_thresholds(&path_p95_ms, args.max_p95_ms)?;
    let backend_observation = support::BackendObservation::model_path(
        args.backend,
        "Vulkan was requested and WhisperEngine loaded, but whisper-rs exposes no verified device/offload result for this context; CPU fallback cannot be ruled out",
    );

    support::finish_report_with_latency_thresholds(
        support::ReportInput {
            benchmark: "candidate_asr_optimization_ab".to_string(),
            harness_sha256: support::harness_sha256(&[
                include_bytes!("asr_optimization_ab.rs"),
                include_bytes!("support/mod.rs"),
                include_bytes!("support/synthetic_audio.rs"),
            ]),
            production: support::production_fingerprint(&[
                ("src/asr/engine.rs", include_bytes!("../src/asr/engine.rs")),
                ("src/asr/types.rs", include_bytes!("../src/asr/types.rs")),
                ("src/pipeline.rs", include_bytes!("../src/pipeline.rs")),
            ]),
            backend_observation,
            config: json!({
                "candidate_only": true,
                "runs": args.runs,
                "warmups_per_path": args.warmups,
                "sample_rate_hz": synthetic_audio::SAMPLE_RATE,
                "fixture_id": fixture.id,
                "fixture_description": fixture.description,
                "fixture_duration_ms": fixture.duration_ms,
                "fixture_pcm_sha256": fixture.pcm_sha256,
                "fixture_wav_sha256": wav_sha256,
                "fixture_wav_bytes": wav.len(),
                "warmup_fixture_id": warmup_fixture.id,
                "warmup_fixture_duration_ms": warmup_fixture.duration_ms,
                "warmup_fixture_pcm_sha256": warmup_fixture.pcm_sha256,
                "language": config.language,
                "translate": config.translate,
                "n_threads": config.n_threads,
                "configured_beam_size": config.beam_size,
                "latency_gate": {
                    "flag": "--max-p95-ms",
                    "aggregation": "none",
                    "primary_metric": "reused_latency_first_p95_ms",
                    "secondary_metric": "fresh_configured_p95_ms",
                    "diagnostic_paths_are_not_gated": ["reused_configured", "fresh_latency_first"],
                },
                "paths": [
                    {"label": "fresh_configured", "state": "fresh", "decode": "configured", "role": "production_regular_dictation"},
                    {"label": "reused_configured", "state": "reused", "decode": "configured", "role": "diagnostic_rng_carryover"},
                    {"label": "fresh_latency_first", "state": "fresh", "decode": "greedy_best_of_1_single_segment_no_fallback", "role": "diagnostic"},
                    {"label": "reused_latency_first", "state": "reused", "decode": "greedy_best_of_1_single_segment_no_fallback", "role": "production_command_mode"},
                ],
            }),
            phases,
            quality: json!({
                "metric": "synthetic_hallucination_free_pct",
                "quality_pct": hallucination_free_pct,
                "hallucination_free": hallucination_free,
                "total_trials": total,
                "fresh_configured_reference_matches": reference_matches,
                "fresh_configured_reference_match_pct": 100.0 * reference_matches as f64 / total as f64,
                "stage_summaries": stage_summaries,
                "trials": trials,
                "limitation": "synthetic fixture has no lexical ground truth; text/hash fields detect path drift and hallucination but do not measure WER",
            }),
            quality_pct: hallucination_free_pct,
            additional_thresholds: Vec::new(),
            args,
        },
        latency_thresholds,
    )
}

fn main() {
    support::run_or_exit(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_production_paths_are_gated_with_latency_first_primary() {
        let p95 = BTreeMap::from([
            (PathKind::FreshConfigured, 120),
            (PathKind::FreshLatencyFirst, 2),
            (PathKind::ReusedConfigured, 500),
            (PathKind::ReusedLatencyFirst, 80),
        ]);

        let thresholds = production_latency_thresholds(&p95, Some(100)).unwrap();
        assert_eq!(thresholds.len(), 2);
        assert_eq!(thresholds[0].metric, "reused_latency_first_p95_ms");
        assert_eq!(thresholds[0].actual, 80.0);
        assert!(thresholds[0].passed);
        assert_eq!(thresholds[1].metric, "fresh_configured_p95_ms");
        assert_eq!(thresholds[1].actual, 120.0);
        assert!(!thresholds[1].passed);
    }

    #[test]
    fn no_latency_budget_emits_no_latency_thresholds() {
        assert!(production_latency_thresholds(&BTreeMap::new(), None)
            .unwrap()
            .is_empty());
    }
}
