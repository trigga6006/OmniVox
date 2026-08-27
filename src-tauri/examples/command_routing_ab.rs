//! Candidate-only microbenchmark for the production Command Mode router.
//!
//! Exercises the exact tier-0 classifier, then invokes the production sequence
//! matcher at most once. App-index resolution is a machine-dependent, non-gated
//! diagnostic. LLM fallthrough latency is explicitly unmeasured here.

mod support;

use omnivoice_lib::actions::app_index;
use omnivoice_lib::actions::matcher::{match_command_sequence, match_tier0_command, Tier0Command};
use omnivoice_lib::actions::CommandIntent;
use serde::Serialize;
use serde_json::json;
use std::collections::BTreeSet;
use std::hint::black_box;
use std::time::Instant;
use support::command_corpus::{CommandCase, COMMAND_CASES, COMMAND_WARMUP_UTTERANCES};

const USAGE: &str = "Candidate Command Mode routing microbenchmark

Usage:
  cargo run --release --example command_routing_ab -- --model <model.gguf> --backend cpu [options]

Options:
  --runs <n>                Measured corpus repetitions (default: 1; use 1000+ for stable ns timings)
  --warmups <n>             Unmeasured distinct routing warmups (default: 1)
  --json <path>             Also write the JSON report to this path
  --max-p95-ms <ms>         Fail when routed-corpus p95 exceeds this value
  --min-quality-pct <pct>   Fail when deterministic matched-case precision is below this value
  --source-label <label>    Label this source tree (candidate is recommended)
  --hardware-label <label>  Stable identifier for the benchmark machine

The model is identified and hashed for comparison with command_ab, but is not
loaded. Every fallthrough is marked incomplete with unknown LLM latency.
App-index initialization/resolution is machine-dependent and never quality- or
latency-gated.";

const TIER0_CASES: &[(&str, Tier0Command)] = &[
    ("stop", Tier0Command::Cancel),
    ("please cancel that", Tier0Command::Cancel),
    ("never mind", Tier0Command::Cancel),
    ("abort please", Tier0Command::Cancel),
    ("undo that", Tier0Command::Undo),
    ("could you undo the last command please", Tier0Command::Undo),
];

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

fn command_corpus_sha256() -> String {
    let mut bytes = Vec::new();
    for case in COMMAND_CASES {
        bytes.extend_from_slice(case.utterance.as_bytes());
        bytes.push(0);
        for step in case.expected {
            bytes.extend_from_slice(step.action.as_bytes());
            bytes.push(0);
            bytes.extend_from_slice(step.target.as_bytes());
            bytes.push(0xfe);
        }
        bytes.push(0xff);
    }
    support::harness_sha256(&[bytes.as_slice()])
}

#[derive(Serialize)]
struct RouteTrial {
    run: usize,
    utterance: &'static str,
    expected: String,
    predicted: String,
    route: &'static str,
    tier0_result: Option<&'static str>,
    tier0_check_ns: u64,
    matcher_calls: u8,
    matcher_ns: Option<u64>,
    app_resolution_ns: Option<u64>,
    app_resolved: Option<bool>,
    falls_through_to_llm: bool,
    llm_fallback_latency_ms: Option<u64>,
    completed_without_llm: bool,
    action_correct_when_matched: Option<bool>,
    full_correct_when_matched: Option<bool>,
}

#[derive(Serialize)]
struct NanoSummary {
    count: usize,
    min_ns: u64,
    max_ns: u64,
    mean_ns: f64,
    mean_us: f64,
    p50_ns: u64,
    p95_ns: u64,
}

fn summarize_ns(samples: &[u64]) -> NanoSummary {
    if samples.is_empty() {
        return NanoSummary {
            count: 0,
            min_ns: 0,
            max_ns: 0,
            mean_ns: 0.0,
            mean_us: 0.0,
            p50_ns: 0,
            p95_ns: 0,
        };
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let percentile = |pct: f64| {
        let rank = (pct * sorted.len() as f64).ceil() as usize;
        sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
    };
    let mean_ns =
        sorted.iter().map(|&value| value as u128).sum::<u128>() as f64 / sorted.len() as f64;
    NanoSummary {
        count: sorted.len(),
        min_ns: sorted[0],
        max_ns: sorted[sorted.len() - 1],
        mean_ns,
        mean_us: mean_ns / 1_000.0,
        p50_ns: percentile(0.50),
        p95_ns: percentile(0.95),
    }
}

fn elapsed_ns(started: Instant) -> u64 {
    started.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

fn run() -> Result<bool, String> {
    let args = support::parse_common_args("OMNIVOX_LLM_MODEL", USAGE)?;

    // Load the OS app snapshot separately so the first matcher case is not
    // polluted by PowerShell/index initialization. This is diagnostic only.
    let index_started = Instant::now();
    black_box(app_index::resolve("__omnivox_benchmark_probe__"));
    let app_index_initialization_ns = elapsed_ns(index_started);

    for warmup in 0..args.warmups {
        let utterance = COMMAND_WARMUP_UTTERANCES[warmup % COMMAND_WARMUP_UTTERANCES.len()];
        let tier0 = black_box(match_tier0_command(black_box(utterance)));
        if tier0.is_none() {
            black_box(match_command_sequence(black_box(utterance)));
        }
    }

    let mut trials = Vec::with_capacity(args.runs * (TIER0_CASES.len() + COMMAND_CASES.len()));
    let mut corpus_samples_ms = Vec::with_capacity(args.runs);
    let mut tier0_ns = Vec::new();
    let mut matcher_ns = Vec::new();
    let mut app_resolution_ns = Vec::new();
    let mut matched = 0usize;
    let mut action_hits = 0usize;
    let mut full_hits = 0usize;
    let mut false_positives = 0usize;
    let mut wrong_matches = 0usize;
    let mut positive_total = 0usize;
    let mut negative_total = 0usize;
    let mut negative_fallthroughs = 0usize;
    let mut deterministic_misses = 0usize;
    let mut llm_fallthroughs = 0usize;
    let mut tier0_hits = 0usize;
    let mut max_matcher_calls = 0u8;

    for run in 0..args.runs {
        let corpus_started = Instant::now();

        for (utterance, expected_tier0) in TIER0_CASES {
            let started = Instant::now();
            let actual = match_tier0_command(black_box(utterance));
            let elapsed = elapsed_ns(started);
            tier0_ns.push(elapsed);
            tier0_hits += usize::from(actual == Some(*expected_tier0));
            trials.push(RouteTrial {
                run,
                utterance,
                expected: format!("tier0::{expected_tier0:?}"),
                predicted: actual
                    .map(|route| format!("tier0::{route:?}"))
                    .unwrap_or_else(|| "no_tier0_match".to_string()),
                route: match actual {
                    Some(Tier0Command::Cancel) => "tier0_cancel_completed",
                    Some(Tier0Command::Undo) => "tier0_undo_completed",
                    None => "tier0_miss_incomplete",
                },
                tier0_result: actual.map(|route| match route {
                    Tier0Command::Cancel => "cancel",
                    Tier0Command::Undo => "undo",
                }),
                tier0_check_ns: elapsed,
                matcher_calls: 0,
                matcher_ns: None,
                app_resolution_ns: None,
                app_resolved: None,
                falls_through_to_llm: false,
                llm_fallback_latency_ms: None,
                completed_without_llm: actual.is_some(),
                action_correct_when_matched: None,
                full_correct_when_matched: Some(actual == Some(*expected_tier0)),
            });
        }

        for case in COMMAND_CASES {
            let expected = expected_intents(case)?;
            positive_total += usize::from(!expected.is_empty());
            negative_total += usize::from(expected.is_empty());

            let tier0_started = Instant::now();
            let tier0 = match_tier0_command(black_box(case.utterance));
            let tier0_elapsed = elapsed_ns(tier0_started);
            tier0_ns.push(tier0_elapsed);
            if tier0.is_some() {
                return Err(format!(
                    "command corpus unexpectedly shadowed by tier-0: {}",
                    case.utterance
                ));
            }

            let matcher_started = Instant::now();
            let predicted = match_command_sequence(black_box(case.utterance));
            let matcher_elapsed = elapsed_ns(matcher_started);
            matcher_ns.push(matcher_elapsed);
            let matcher_calls = 1u8;
            max_matcher_calls = max_matcher_calls.max(matcher_calls);

            let action_correct = predicted
                .as_ref()
                .map(|actual| chain_eq(actual, &expected, action_eq));
            let full_correct = predicted
                .as_ref()
                .map(|actual| chain_eq(actual, &expected, |a, b| a == b));

            let mut resolution_elapsed = None;
            let mut resolved = None;
            if let Some([CommandIntent::OpenApp(name)]) = predicted.as_deref() {
                let started = Instant::now();
                let result = app_index::resolve(name);
                let elapsed = elapsed_ns(started);
                app_resolution_ns.push(elapsed);
                resolution_elapsed = Some(elapsed);
                resolved = Some(result.is_some());
                black_box(result);
            }

            let matched_deterministically = predicted.is_some();
            let unresolved_app = resolved == Some(false);
            let falls_through_to_llm = !matched_deterministically || unresolved_app;
            let route = if !matched_deterministically {
                "llm_fallback_unmatched_incomplete"
            } else if unresolved_app {
                "llm_fallback_after_app_resolution_incomplete"
            } else if resolved == Some(true) {
                "app_resolution_ready_for_dispatch"
            } else {
                "deterministic_ready_for_dispatch"
            };
            llm_fallthroughs += usize::from(falls_through_to_llm);

            if matched_deterministically {
                matched += 1;
                action_hits += usize::from(action_correct == Some(true));
                full_hits += usize::from(full_correct == Some(true));
                false_positives += usize::from(expected.is_empty());
                wrong_matches += usize::from(full_correct == Some(false) && !expected.is_empty());
            } else if expected.is_empty() {
                negative_fallthroughs += 1;
            } else {
                deterministic_misses += 1;
            }

            trials.push(RouteTrial {
                run,
                utterance: case.utterance,
                expected: format_intents(&expected),
                predicted: predicted
                    .as_ref()
                    .map(|intents| format_intents(intents))
                    .unwrap_or_else(|| "llm_fallback_required".to_string()),
                route,
                tier0_result: None,
                tier0_check_ns: tier0_elapsed,
                matcher_calls,
                matcher_ns: Some(matcher_elapsed),
                app_resolution_ns: resolution_elapsed,
                app_resolved: resolved,
                falls_through_to_llm,
                llm_fallback_latency_ms: None,
                completed_without_llm: matched_deterministically && !unresolved_app,
                action_correct_when_matched: action_correct,
                full_correct_when_matched: full_correct,
            });
        }
        corpus_samples_ms.push(support::elapsed_ms(corpus_started));
    }

    let command_trials = args.runs * COMMAND_CASES.len();
    let fallthroughs = command_trials - matched;
    let coverage_pct = 100.0 * matched as f64 / command_trials as f64;
    let positive_recall_pct = 100.0 * full_hits as f64 / positive_total as f64;
    let matched_precision_pct = if matched == 0 {
        100.0
    } else {
        100.0 * full_hits as f64 / matched as f64
    };
    let action_precision_pct = if matched == 0 {
        100.0
    } else {
        100.0 * action_hits as f64 / matched as f64
    };
    let specificity_pct = if negative_total == 0 {
        100.0
    } else {
        100.0 * negative_fallthroughs as f64 / negative_total as f64
    };
    let safe_route_pct =
        100.0 * (command_trials - false_positives - wrong_matches) as f64 / command_trials as f64;
    let distinct_predictions = trials
        .iter()
        .map(|trial| trial.predicted.clone())
        .collect::<BTreeSet<_>>()
        .len();

    support::finish_report(support::ReportInput {
        benchmark: "candidate_command_routing_ab".to_string(),
        harness_sha256: support::harness_sha256(&[
            include_bytes!("command_routing_ab.rs"),
            include_bytes!("support/mod.rs"),
            include_bytes!("support/command_corpus.rs"),
        ]),
        production: support::production_fingerprint(&[
            ("src/actions/app_index.rs", include_bytes!("../src/actions/app_index.rs")),
            ("src/actions/intent.rs", include_bytes!("../src/actions/intent.rs")),
            ("src/actions/matcher.rs", include_bytes!("../src/actions/matcher.rs")),
            ("src/pipeline.rs", include_bytes!("../src/pipeline.rs")),
        ]),
        backend_observation: support::BackendObservation::not_exercised(
            "This candidate diagnostic does not load or run the model; requested backend applies only to the incomplete LLM fallthrough stage",
        ),
        config: json!({
            "candidate_only": true,
            "runs": args.runs,
            "warmups": args.warmups,
            "command_cases_per_run": COMMAND_CASES.len(),
            "tier0_cases_per_run": TIER0_CASES.len(),
            "multi_step_cases": COMMAND_CASES.iter().filter(|case| case.expected.len() > 1).count(),
            "command_corpus_sha256": command_corpus_sha256(),
            "routing_paths": ["actions::matcher::match_tier0_command", "actions::matcher::match_command_sequence (at most once)", "actions::app_index::resolve for OpenApp only", "unmeasured LLM fallback"],
            "model_loaded": false,
            "model_metadata_purpose": "identifies the all-LLM command_ab comparison target",
            "app_resolution_gated": false,
            "app_resolution_machine_dependent": true,
            "llm_fallthrough_latency_measured": false,
            "timing_note": "per-stage timing is nanoseconds; shared corpus milliseconds may round to zero",
        }),
        phases: vec![
            support::PhaseReport::new(
                "app_index_snapshot_initialization",
                "machine_dependent_diagnostic",
                vec![app_index_initialization_ns / 1_000_000],
            ),
            support::PhaseReport::new("routing_corpus", "warm", corpus_samples_ms),
        ],
        quality: json!({
            "metric": "deterministic_matched_case_precision_pct",
            "quality_pct": matched_precision_pct,
            "coverage_pct": coverage_pct,
            "positive_full_chain_recall_pct": positive_recall_pct,
            "matched_action_chain_precision_pct": action_precision_pct,
            "matched_full_chain_precision_pct": matched_precision_pct,
            "specificity_pct": specificity_pct,
            "safe_route_pct": safe_route_pct,
            "tier0_hits": tier0_hits,
            "tier0_total": args.runs * TIER0_CASES.len(),
            "max_matcher_calls_per_trial": max_matcher_calls,
            "matched_deterministically": matched,
            "matcher_fallthroughs": fallthroughs,
            "llm_fallthroughs_including_unresolved_apps": llm_fallthroughs,
            "deterministic_coverage_misses": deterministic_misses,
            "negative_cases": negative_total,
            "negative_fallthroughs": negative_fallthroughs,
            "false_positives": false_positives,
            "wrong_matches": wrong_matches,
            "distinct_prediction_labels": distinct_predictions,
            "tier0_check_timing": summarize_ns(&tier0_ns),
            "single_matcher_call_timing": summarize_ns(&matcher_ns),
            "app_index_initialization_ns": app_index_initialization_ns,
            "warm_app_resolution_timing": summarize_ns(&app_resolution_ns),
            "trials": trials,
            "limitation": "LLM fallthrough is an incomplete route with unknown latency/correctness; join with command_ab for model cost. App-index diagnostics depend on installed apps and are not gated.",
        }),
        quality_pct: matched_precision_pct,
        additional_thresholds: Vec::new(),
        args,
    })
}

fn main() {
    support::run_or_exit(run);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_corpora_cover_tier_zero_and_fixed_multistep_without_shadowing() {
        assert!(TIER0_CASES
            .iter()
            .all(|(utterance, expected)| match_tier0_command(utterance) == Some(*expected)));
        assert!(COMMAND_CASES
            .iter()
            .all(|case| match_tier0_command(case.utterance).is_none()));

        let multi = COMMAND_CASES
            .iter()
            .filter(|case| case.expected.len() > 1)
            .collect::<Vec<_>>();
        assert!(multi.len() >= 3);
        for case in multi {
            let expected = expected_intents(case).unwrap();
            if expected.len() <= 3 {
                assert_eq!(match_command_sequence(case.utterance), Some(expected));
            } else {
                // The production deterministic tier deliberately caps chains
                // at three and sends longer requests to the LLM intact.
                assert_eq!(match_command_sequence(case.utterance), None);
            }
        }
    }
}
