//! Fair Command Mode model benchmark.
//!
//! Runs a fixed corpus through the app's persistent production `LlmRunner`
//! command path and scores the complete ordered intent chain. Missing, extra,
//! or reordered steps are failures.

mod support;

use omnivoice_lib::actions::CommandIntent;
use omnivoice_lib::llm::engine::LlamaEngine;
use omnivoice_lib::llm::profiles;
use omnivoice_lib::llm::runner::LlmRunner;
use omnivoice_lib::llm::types::LlmConfig;
use serde::Serialize;
use serde_json::json;
use std::time::{Duration, Instant};
use support::command_corpus::{CommandCase, COMMAND_CASES, COMMAND_WARMUP_UTTERANCES};

const PRODUCTION_DEADLINE_MS: u64 = 8_000;

const USAGE: &str = "Command Mode model benchmark

Usage:
  cargo run --release --example command_ab -- --model <model.gguf> --backend cpu [options]
  cargo run --release --example command_ab --features vulkan -- --model <model.gguf> --backend vulkan [options]

Options:
  --runs <n>                Measured corpus repetitions (default: 1)
  --warmups <n>             Unmeasured warmup inferences (default: 1)
  --json <path>             Also write the JSON report to this path
  --max-p95-ms <ms>         Fail when warm inference p95 exceeds this value
  --min-quality-pct <pct>   Fail when full ordered-chain accuracy is below this value
  --source-label <label>    Label this source tree, e.g. baseline or candidate
  --hardware-label <label>  Stable identifier for the benchmark machine

The model may also be supplied through OMNIVOX_LLM_MODEL. Cold end-to-end time
is load + runner ready/profile warm + first inference and is assessed against
the app's default 8-second Command Mode deadline.";

fn action_eq(a: &CommandIntent, b: &CommandIntent) -> bool {
    use CommandIntent::*;
    match (a, b) {
        (OpenApp(_), OpenApp(_)) | (WebSearch(_), WebSearch(_)) | (OpenUrl(_), OpenUrl(_)) => true,
        (KeyChord(x), KeyChord(y)) => x == y,
        (Media(x), Media(y)) => x == y,
        (Window(x), Window(y)) => x == y,
        (CloseWindow, CloseWindow) => true,
        _ => false,
    }
}

fn full_eq(a: &CommandIntent, b: &CommandIntent) -> bool {
    use CommandIntent::*;
    match (a, b) {
        (OpenApp(x), OpenApp(y)) | (WebSearch(x), WebSearch(y)) | (OpenUrl(x), OpenUrl(y)) => {
            x.trim().eq_ignore_ascii_case(y.trim())
        }
        _ => a == b,
    }
}

fn expected_intents(case: &CommandCase) -> Result<Vec<CommandIntent>, String> {
    case.expected
        .iter()
        .map(|step| {
            CommandIntent::from_llm(step.action, step.target).ok_or_else(|| {
                format!(
                    "invalid corpus expectation: action={} target={}",
                    step.action, step.target
                )
            })
        })
        .collect()
}

fn chain_eq(
    actual: &[CommandIntent],
    expected: &[CommandIntent],
    eq: impl Fn(&CommandIntent, &CommandIntent) -> bool,
) -> bool {
    actual.len() == expected.len()
        && actual
            .iter()
            .zip(expected)
            .all(|(actual, expected)| eq(actual, expected))
}

fn format_intents(intents: &[CommandIntent]) -> String {
    if intents.is_empty() {
        "none".to_string()
    } else {
        format!("{intents:?}")
    }
}

#[derive(Serialize)]
struct Miss {
    run: usize,
    utterance: &'static str,
    predicted: String,
    expected: String,
    error: Option<String>,
}

fn classify_one(
    runtime: &tokio::runtime::Runtime,
    runner: &LlmRunner,
    utterance: &str,
) -> (Vec<CommandIntent>, Option<String>) {
    match runtime.block_on(runner.classify_command_with_timeout(
        utterance.to_string(),
        Duration::from_millis(PRODUCTION_DEADLINE_MS),
    )) {
        Ok(intents) => (intents, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    }
}

fn run() -> Result<bool, String> {
    let args = support::parse_common_args("OMNIVOX_LLM_MODEL", USAGE)?;
    let config = LlmConfig {
        model_path: args.model_path.to_string_lossy().into_owned(),
        use_gpu: args.backend.use_gpu(),
        ..LlmConfig::default()
    };
    let config_json = json!({
        "runs": args.runs,
        "warmups": args.warmups,
        "cases_per_run": COMMAND_CASES.len(),
        "multi_step_cases": COMMAND_CASES.iter().filter(|case| case.expected.len() > 1).count(),
        "n_threads": config.n_threads,
        "n_ctx": config.n_ctx,
        "max_tokens": config.max_tokens,
        "production_deadline_ms": PRODUCTION_DEADLINE_MS,
        "execution_path": "LlmRunner::classify_command_with_timeout",
        "scoring": "exact ordered intent chain; no missing, extra, or reordered steps",
        "prompt": "current production command prompt",
    });

    eprintln!(
        "[command_ab] loading {} with requested {:?} backend...",
        args.model_path.display(),
        args.backend
    );
    let started = Instant::now();
    let engine = LlamaEngine::load(config).map_err(|error| error.to_string())?;
    let load_ms = support::elapsed_ms(started);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| error.to_string())?;

    let started = Instant::now();
    let runner = LlmRunner::spawn(engine, profiles::get("agent-prompt"))
        .map_err(|error| error.to_string())?;
    let runner_ready_ms = support::elapsed_ms(started);

    let started = Instant::now();
    let (_, cold_error) = classify_one(&runtime, &runner, COMMAND_WARMUP_UTTERANCES[0]);
    let first_inference_ms = support::elapsed_ms(started);
    let cold_end_to_end_ms = load_ms
        .saturating_add(runner_ready_ms)
        .saturating_add(first_inference_ms);

    for warmup in 0..args.warmups {
        let utterance = COMMAND_WARMUP_UTTERANCES[(warmup + 1) % COMMAND_WARMUP_UTTERANCES.len()];
        let _ = classify_one(&runtime, &runner, utterance);
    }

    let mut samples_ms = Vec::with_capacity(args.runs * COMMAND_CASES.len());
    let mut action_chain_hits = 0usize;
    let mut full_chain_hits = 0usize;
    let mut negative_total = 0usize;
    let mut negative_hits = 0usize;
    let mut errors = 0usize;
    let mut misses = Vec::new();
    for run in 0..args.runs {
        eprintln!("[command_ab] measured run {}/{}", run + 1, args.runs);
        for case in COMMAND_CASES {
            let expected = expected_intents(case)?;
            let started = Instant::now();
            let (predicted, error) = classify_one(&runtime, &runner, case.utterance);
            samples_ms.push(support::elapsed_ms(started));

            let action_ok = error.is_none() && chain_eq(&predicted, &expected, action_eq);
            let full_ok = error.is_none() && chain_eq(&predicted, &expected, full_eq);
            action_chain_hits += usize::from(action_ok);
            full_chain_hits += usize::from(full_ok);
            if expected.is_empty() {
                negative_total += 1;
                negative_hits += usize::from(predicted.is_empty() && error.is_none());
            }
            errors += usize::from(error.is_some());
            if !full_ok {
                misses.push(Miss {
                    run,
                    utterance: case.utterance,
                    predicted: format_intents(&predicted),
                    expected: format_intents(&expected),
                    error,
                });
            }
        }
    }

    let total = args.runs * COMMAND_CASES.len();
    let action_chain_accuracy_pct = 100.0 * action_chain_hits as f64 / total as f64;
    let full_chain_accuracy_pct = 100.0 * full_chain_hits as f64 / total as f64;
    let specificity_pct = if negative_total == 0 {
        100.0
    } else {
        100.0 * negative_hits as f64 / negative_total as f64
    };
    let deadline_passed = cold_error.is_none() && cold_end_to_end_ms <= PRODUCTION_DEADLINE_MS;
    let backend_observation = support::BackendObservation::model_path(
        args.backend,
        "Vulkan was requested and the LlamaEngine loaded, but the llama_cpp API used here exposes neither an actual offloaded-layer count nor fallback device",
    );

    support::finish_report(support::ReportInput {
        benchmark: "command_ab".to_string(),
        harness_sha256: support::harness_sha256(&[
            include_bytes!("command_ab.rs"),
            include_bytes!("support/mod.rs"),
            include_bytes!("support/command_corpus.rs"),
        ]),
        production: support::production_fingerprint(&[
            (
                "src/actions/intent.rs",
                include_bytes!("../src/actions/intent.rs"),
            ),
            ("src/llm/engine.rs", include_bytes!("../src/llm/engine.rs")),
            (
                "src/llm/grammar.rs",
                include_bytes!("../src/llm/grammar.rs"),
            ),
            (
                "src/llm/profiles.rs",
                include_bytes!("../src/llm/profiles.rs"),
            ),
            ("src/llm/prompt.rs", include_bytes!("../src/llm/prompt.rs")),
            ("src/llm/runner.rs", include_bytes!("../src/llm/runner.rs")),
            ("src/llm/types.rs", include_bytes!("../src/llm/types.rs")),
        ]),
        backend_observation,
        config: config_json,
        phases: vec![
            support::PhaseReport::new("model_load", "cold_component", vec![load_ms]),
            support::PhaseReport::new(
                "runner_ready_and_profile_warm",
                "cold_component",
                vec![runner_ready_ms],
            ),
            support::PhaseReport::new(
                "first_inference",
                "cold_component",
                vec![first_inference_ms],
            ),
            support::PhaseReport::new(
                "cold_end_to_end",
                "cold_end_to_end",
                vec![cold_end_to_end_ms],
            ),
            support::PhaseReport::new("corpus_inference", "warm", samples_ms),
        ],
        quality: json!({
            "metric": "full_ordered_intent_chain_accuracy_pct",
            "quality_pct": full_chain_accuracy_pct,
            "action_chain_accuracy_pct": action_chain_accuracy_pct,
            "full_chain_accuracy_pct": full_chain_accuracy_pct,
            "specificity_pct": specificity_pct,
            "action_chain_hits": action_chain_hits,
            "full_chain_hits": full_chain_hits,
            "total_cases": total,
            "negative_cases": negative_total,
            "errors": errors,
            "cold_first_inference_error": cold_error,
            "cold_end_to_end_deadline_ms": PRODUCTION_DEADLINE_MS,
            "cold_end_to_end_within_deadline": deadline_passed,
            "misses": misses,
        }),
        quality_pct: full_chain_accuracy_pct,
        additional_thresholds: vec![support::ThresholdResult {
            metric: "cold_end_to_end_ms".to_string(),
            operator: "<=",
            expected: PRODUCTION_DEADLINE_MS as f64,
            actual: cold_end_to_end_ms as f64,
            passed: deadline_passed,
        }],
        args,
    })
}

fn main() {
    support::run_or_exit(run);
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnivoice_lib::actions::KeyChord;

    #[test]
    fn chain_scoring_rejects_missing_extra_and_reordered_steps() {
        let expected = vec![
            CommandIntent::KeyChord(KeyChord::Copy),
            CommandIntent::KeyChord(KeyChord::Paste),
        ];
        assert!(chain_eq(&expected, &expected, full_eq));
        assert!(!chain_eq(&expected[..1], &expected, full_eq));
        let mut extra = expected.clone();
        extra.push(CommandIntent::KeyChord(KeyChord::Save));
        assert!(!chain_eq(&extra, &expected, full_eq));
        let reversed = expected.iter().cloned().rev().collect::<Vec<_>>();
        assert!(!chain_eq(&reversed, &expected, full_eq));
    }
}
