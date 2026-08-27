//! Reproducible local ASR benchmark using deterministic synthetic audio.
//!
//! The fixtures contain no recorded voice or corpus material. They measure
//! cold/warm inference cost and hallucination behavior at fixed audio lengths;
//! they deliberately do not claim to measure word error rate.
//!
//! `--model` selects the engine by shape: a single GGML file benchmarks
//! whisper.cpp, a directory benchmarks the sherpa-onnx Parakeet transducer.

mod support;
#[path = "support/synthetic_audio.rs"]
mod synthetic_audio;

use omnivoice_lib::asr::engine::{AsrEngine, WhisperEngine};
use omnivoice_lib::asr::parakeet::ParakeetEngine;
use omnivoice_lib::asr::types::AsrConfig;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Instant;

const USAGE: &str = "ASR benchmark (Whisper GGML file, or Parakeet model directory)

Usage:
  cargo run --release --example asr_bench -- --model <model.bin> --backend cpu [options]
  cargo run --release --example asr_bench --features vulkan -- --model <model.bin> --backend vulkan [options]
  cargo run --release --example asr_bench -- --model <parakeet-model-dir> --backend cpu [options]

Options:
  --runs <n>                Measured fixture repetitions (default: 1)
  --warmups <n>             Unmeasured distinct 3-second warmups (default: 1)
  --json <path>             Also write the JSON report to this path
  --max-p95-ms <ms>         Fail when combined warm inference p95 exceeds this value
  --min-quality-pct <pct>   Fail when hallucination-free rate is below this value
  --source-label <label>    Label this source tree, e.g. baseline or candidate
  --hardware-label <label>  Stable identifier for the benchmark machine

The model may also be supplied through OMNIVOX_ASR_MODEL.";

#[derive(Serialize)]
struct FixtureResult {
    run: usize,
    fixture_id: String,
    fixture_description: &'static str,
    fixture_pcm_sha256: String,
    audio_duration_ms: u64,
    inference_ms: u64,
    realtime_factor: f64,
    hallucination_free: bool,
    observed_text: String,
    observed_chars: usize,
}

fn transcribe(engine: &dyn AsrEngine, samples: &[f32]) -> Result<(u64, String), String> {
    let started = Instant::now();
    let result = engine
        .transcribe(samples)
        .map_err(|error| error.to_string())?;
    Ok((support::elapsed_ms(started), result.text))
}

fn run() -> Result<bool, String> {
    let args = support::parse_common_args("OMNIVOX_ASR_MODEL", USAGE)?;
    // A model directory means the sherpa-onnx Parakeet transducer, which has no
    // beam/temperature/prompt knobs and always runs on the ONNX CPU provider.
    let parakeet = args.model_path.is_dir();
    let mut config = AsrConfig::default();
    config.model_path = args.model_path.to_string_lossy().into_owned();
    config.language = Some("en".to_string());
    config.translate = false;
    config.use_gpu = args.backend.use_gpu() && !parakeet;
    config.initial_prompt = None;
    config.beam_size = Some(if config.use_gpu { 5 } else { 2 });
    let config_json = if parakeet {
        json!({
            "runs": args.runs,
            "warmups": args.warmups,
            "sample_rate_hz": synthetic_audio::SAMPLE_RATE,
            "fixture_durations_ms": [1000, 5000, 10000, 15000],
            "fixture_provenance": "deterministic mathematical synthesis; no recorded or corpus audio",
            "engine": "parakeet-cpu",
            "n_threads": config.n_threads,
            "decoding_method": "greedy_search (sherpa-onnx default)",
        })
    } else {
        json!({
            "runs": args.runs,
            "warmups": args.warmups,
            "sample_rate_hz": synthetic_audio::SAMPLE_RATE,
            "fixture_durations_ms": [1000, 5000, 10000, 15000],
            "fixture_provenance": "deterministic mathematical synthesis; no recorded or corpus audio",
            "language": config.language,
            "translate": config.translate,
            "n_threads": config.n_threads,
            "beam_size": config.beam_size,
        })
    };

    eprintln!(
        "[asr_bench] loading {} on {:?}...",
        args.model_path.display(),
        args.backend
    );
    let started = Instant::now();
    let engine: Box<dyn AsrEngine> = if parakeet {
        Box::new(
            ParakeetEngine::load(&args.model_path, config.n_threads)
                .map_err(|error| error.to_string())?,
        )
    } else {
        Box::new(WhisperEngine::load(config).map_err(|error| error.to_string())?)
    };
    let engine = engine.as_ref();
    let load_ms = support::elapsed_ms(started);
    let fixtures = synthetic_audio::fixtures();
    let cold_fixture = fixtures
        .iter()
        .find(|fixture| fixture.duration_ms == 10_000)
        .ok_or_else(|| "missing 10-second cold fixture".to_string())?;
    let (cold_ms, _) = transcribe(engine, &cold_fixture.samples)?;
    let warmup_fixture = synthetic_audio::warmup_fixture();
    for _ in 0..args.warmups {
        let _ = transcribe(engine, &warmup_fixture.samples)?;
    }

    let mut by_fixture = BTreeMap::<u64, Vec<u64>>::new();
    let mut results = Vec::with_capacity(args.runs * fixtures.len());
    let mut hallucination_free = 0usize;
    for run in 0..args.runs {
        eprintln!("[asr_bench] measured run {}/{}", run + 1, args.runs);
        for fixture in &fixtures {
            let (inference_ms, observed_text) = transcribe(engine, &fixture.samples)?;
            by_fixture
                .entry(fixture.duration_ms)
                .or_default()
                .push(inference_ms);
            let clean = observed_text.trim().is_empty();
            hallucination_free += usize::from(clean);
            results.push(FixtureResult {
                run,
                fixture_id: fixture.id.clone(),
                fixture_description: fixture.description,
                fixture_pcm_sha256: fixture.pcm_sha256.clone(),
                audio_duration_ms: fixture.duration_ms,
                inference_ms,
                realtime_factor: inference_ms as f64 / fixture.duration_ms as f64,
                hallucination_free: clean,
                observed_chars: observed_text.chars().count(),
                observed_text,
            });
        }
    }

    let total = results.len();
    let hallucination_free_pct = 100.0 * hallucination_free as f64 / total as f64;
    let mean_realtime_factor = results
        .iter()
        .map(|result| result.realtime_factor)
        .sum::<f64>()
        / total as f64;
    let mut phases = vec![
        support::PhaseReport::new("model_load", "cold", vec![load_ms]),
        support::PhaseReport::new("first_10s_inference", "cold", vec![cold_ms]),
    ];
    phases.extend(by_fixture.into_iter().map(|(duration_ms, samples)| {
        support::PhaseReport::new(format!("fixture_{duration_ms}ms"), "warm", samples)
    }));

    let backend_observation = if parakeet {
        support::BackendObservation {
            verified_actual: support::VerifiedActualBackend::Cpu,
            evidence: "Parakeet is configured with the ONNX Runtime CPU provider; no GPU provider is compiled in".to_string(),
        }
    } else {
        support::BackendObservation::model_path(
            args.backend,
            "Vulkan was requested and WhisperEngine loaded, but whisper-rs exposes no verified device/offload result for this context; CPU fallback cannot be ruled out",
        )
    };

    support::finish_report(support::ReportInput {
        benchmark: if parakeet {
            "parakeet_asr".to_string()
        } else {
            "whisper_asr".to_string()
        },
        harness_sha256: support::harness_sha256(&[
            include_bytes!("asr_bench.rs"),
            include_bytes!("support/mod.rs"),
            include_bytes!("support/synthetic_audio.rs"),
        ]),
        production: support::production_fingerprint(&[
            ("src/asr/engine.rs", include_bytes!("../src/asr/engine.rs")),
            (
                "src/asr/parakeet.rs",
                include_bytes!("../src/asr/parakeet.rs"),
            ),
            ("src/asr/types.rs", include_bytes!("../src/asr/types.rs")),
        ]),
        backend_observation,
        config: config_json,
        phases,
        quality: json!({
            "metric": "synthetic_hallucination_free_pct",
            "quality_pct": hallucination_free_pct,
            "hallucination_free": hallucination_free,
            "total_cases": total,
            "mean_realtime_factor": mean_realtime_factor,
            "results": results,
            "limitation": "synthetic fixtures have no lexical content and do not measure word error rate",
        }),
        quality_pct: hallucination_free_pct,
        additional_thresholds: Vec::new(),
        args,
    })
}

fn main() {
    support::run_or_exit(run);
}
