//! Structured Mode extraction A/B harness.
//!
//! Runs a fixed, labeled set of dictations through each profile in the
//! compile-time registry (the SAME prompt + grammar + postprocess the app
//! uses, including the anti-fabrication grounding pass) and scores the
//! rendered Markdown with grounded-content checks — case-insensitive
//! substring expectations plus fabrication rejects, not exact matches.
//! Use it to iterate on profile prompts or A/B candidate models before
//! shipping a prompt change (any byte change re-warms the KV cache once).
//!
//! Usage:
//!   cargo run --release --example extraction_ab -- --model <model.gguf> --backend cpu
//!   cargo run --release --example extraction_ab --features vulkan -- --model <model.gguf> --backend vulkan
//!
//! One KV session per profile mirrors the production hot path. Reports use the
//! same JSON schema as the Command Mode and ASR benchmarks.

mod support;

use omnivoice_lib::llm::engine::{LlamaEngine, LlmEngine};
use omnivoice_lib::llm::profiles;
use omnivoice_lib::llm::types::LlmConfig;
use serde::Serialize;
use serde_json::json;
use std::time::Instant;

const PRODUCTION_DEADLINE_MS: u64 = 8_000;
const STRUCTURED_WARMUP_DICTATIONS: &[&str] = &[
    "Summarize the launch checklist: verify backups, notify support, and deploy after six pm.",
    "Write a short note that the design review moved to Tuesday morning.",
    "Organize these ideas: simpler onboarding, clearer errors, and faster startup.",
];

const USAGE: &str = "Structured Mode model benchmark

Usage:
  cargo run --release --example extraction_ab -- --model <model.gguf> --backend cpu [options]
  cargo run --release --example extraction_ab --features vulkan -- --model <model.gguf> --backend vulkan [options]

Options:
  --runs <n>                Measured corpus repetitions (default: 1)
  --warmups <n>             Unmeasured warmup inferences per profile (default: 1)
  --json <path>             Also write the JSON report to this path
  --max-p95-ms <ms>         Fail when warm inference p95 exceeds this value
  --min-quality-pct <pct>   Fail when grounded case pass rate is below this value
  --source-label <label>    Label this source tree, e.g. baseline or candidate
  --hardware-label <label>  Stable identifier for the benchmark machine

The model may also be supplied through OMNIVOX_LLM_MODEL. Cold end-to-end time
is model load + default-profile session ready + first inference and is assessed
against the app's default 8-second Structured Mode deadline. Screen-context
capture/merge is intentionally excluded and reported as a separate scope.";

struct Case {
    name: &'static str,
    dictation: &'static str,
    /// Case-insensitive substrings that MUST appear in the rendered markdown
    /// (grounded content the profile has no excuse to lose).
    expect: &'static [&'static str],
    /// Case-insensitive substrings that must NOT appear (fabrication guards:
    /// invented recipients, sections the dictation doesn't support, …).
    reject: &'static [&'static str],
}

const AGENT_CASES: &[Case] = &[
    Case {
        name: "implementation with files + constraint + urgency",
        dictation: "Refactor the checkout flow in billing.tsx and cart.tsx. Keep the Stripe integration working, don't touch the webhook handlers. This is urgent.",
        expect: &["checkout", "billing.tsx", "stripe", "urgency"],
        reject: &[],
    },
    Case {
        name: "implementation with expected behavior",
        dictation: "When I click a context mode in the settings menu it closes the menu, but I want the menu to stay open while I switch between modes, unless I click off of it.",
        expect: &["menu", "stay open"],
        reject: &[],
    },
    Case {
        name: "short single idea stays goal-only-ish",
        dictation: "Make the structured panel slide out smoothly instead of popping in.",
        expect: &["panel", "smooth"],
        reject: &["billing", "stripe"],
    },
    Case {
        name: "exploration produces questions",
        dictation: "I've been thinking about how the transcription pipeline would handle more languages. Whisper is multilingual out of the box but I'm worried about memory, and I don't know how voice detection behaves for non-English speech. Just want to map the space.",
        expect: &["language", "## open questions"],
        reject: &[],
    },
    Case {
        name: "advice produces options",
        dictation: "I need to decide between keeping the overlay in the same window versus splitting it into its own always-on-top window. Same window is simpler but z-order fights the taskbar, separate window means more IPC. Leaning towards separate but not sure.",
        expect: &["window", "## options"],
        reject: &[],
    },
    Case {
        name: "constraint vs behavior separation",
        dictation: "Add keyboard navigation to the history list. Arrow keys should move the selection and enter should copy the entry. Don't break the existing mouse click behavior.",
        expect: &["keyboard", "arrow", "mouse"],
        reject: &[],
    },
    Case {
        name: "long mixed intent: fix plus think-through",
        dictation: "Two things. First, the history page search is case sensitive and it shouldn't be, searching whisper should match capital Whisper, probably in the search query in history dot rs. Second, more of a think-through: should history move to its own database file? Backups get simpler but I don't know what it does to migrations. Don't change the export code.",
        expect: &["case", "history", "export"],
        reject: &[],
    },
    Case {
        name: "non-coding planning dictation",
        dictation: "I need to plan my sister's surprise birthday dinner for the twentieth. Book the back room at the Italian place, order the flourless chocolate cake she likes, and keep the guest list under twelve people so it stays quiet.",
        expect: &["birthday", "cake"],
        reject: &["```"],
    },
    Case {
        name: "meta-preface gets stripped",
        dictation: "Another quick tweak, I want you to make the waveform bars in the pill a little taller so they're visible from across the room.",
        expect: &["waveform", "taller"],
        reject: &[],
    },
    Case {
        name: "short input does not fabricate slots",
        dictation: "Fix the typo in the settings page header, it says Dictaton instead of Dictation.",
        expect: &["typo", "dictation"],
        reject: &["billing", "stripe", "database"],
    },
];

const EMAIL_CASES: &[Case] = &[
    Case {
        name: "named recipient + deadline + sign-off",
        dictation: "Write an email to Sarah about the quarterly report. The March numbers are still missing and I can't finalize the deck until she sends them. Ask her to get them to me by Thursday. Sign it thanks, Ben.",
        expect: &["to: sarah", "march", "thursday", "thanks"],
        reject: &[],
    },
    Case {
        name: "role recipient (landlord)",
        dictation: "Email the landlord. The kitchen faucet has been leaking for a week and it's getting worse. I already tried tightening it myself. I'd like a plumber to come out this week.",
        expect: &["landlord", "faucet", "plumber"],
        reject: &[],
    },
    Case {
        name: "no recipient dictated -> no To line",
        dictation: "Quick email. Following up on the invoice from last month, we still haven't received payment. Payment terms were net thirty so it's now overdue. Please confirm when we can expect it.",
        expect: &["invoice", "overdue"],
        reject: &["to:"],
    },
    Case {
        name: "no sign-off dictated -> none invented",
        dictation: "Email the team that Thursday's standup moves to two pm because the conference room is double booked. The zoom link stays the same.",
        expect: &["standup", "zoom"],
        reject: &["regards", "sincerely"],
    },
    Case {
        name: "sick day to manager",
        dictation: "Write to my manager that I'm feeling under the weather and taking a sick day today. I'll keep an eye on urgent messages but the sprint demo prep is covered by Dana. Sign it thanks, Alex.",
        expect: &["sick day", "dana", "alex"],
        reject: &[],
    },
    Case {
        name: "interview follow-up",
        dictation: "Email to Priya. Thank you for taking the time to interview me yesterday for the platform engineer role. The team's migration project sounds exactly like the work I want to do. Happy to provide references or work samples.",
        expect: &["to: priya", "interview", "references"],
        reject: &[],
    },
    Case {
        name: "numbers preserved verbatim",
        dictation: "Email support that order 45211 arrived with a cracked screen. I want a replacement, not a refund. The original delivery took three weeks so please expedite this one.",
        expect: &["45211", "cracked", "replacement"],
        reject: &[],
    },
    Case {
        name: "short email does not pad pleasantries",
        dictation: "Email Bob that the four o'clock meeting is cancelled and we'll pick it up async in the doc.",
        expect: &["to: bob", "cancelled"],
        reject: &["hope this", "finds you well"],
    },
    Case {
        name: "multi-point request email",
        dictation: "Write an email to the venue coordinator. We're confirming the workshop for June twelfth. We need the projector, two microphones, and seating for forty. Catering arrives at eleven thirty, so the room has to be open by eleven. Can they confirm parking validation is included?",
        expect: &["june", "projector", "forty", "parking"],
        reject: &[],
    },
];

const NOTES_CASES: &[Case] = &[
    Case {
        name: "appointment notes with topic shift",
        dictation: "Notes from the dentist visit. Small cavity on the lower left molar, they'll fill it next month. I should switch to a soft brush and floss daily. For scheduling, the filling is June ninth and the next cleaning is in six months.",
        expect: &["cavity", "floss", "june"],
        reject: &[],
    },
    Case {
        name: "standup notes get section headings",
        dictation: "Team standup notes. On the migration, the staging run finished in four hours and we found two broken indexes, Priya is writing the fix. On hiring, the backend candidate declined so we're reopening the req and adding a referral bonus.",
        expect: &["### ", "migration", "priya", "hiring"],
        reject: &[],
    },
    Case {
        name: "flat idea list stays flat",
        dictation: "Ideas for the podcast. Invite the founder of the coffee roastery. An episode about pricing mistakes. Maybe a listener Q and A at the end of each month.",
        expect: &["podcast", "roastery", "pricing"],
        reject: &[],
    },
    Case {
        name: "errand list",
        dictation: "Errands for Saturday. Drop the library books before noon. Pick up the dry cleaning. Get milk, eggs, and the good coffee beans. Swing by the hardware store for picture hooks.",
        expect: &["library", "dry cleaning", "milk", "picture hooks"],
        reject: &[],
    },
    Case {
        name: "lecture notes",
        dictation: "Lecture notes on photosynthesis. The light reactions happen in the thylakoid membrane and produce ATP. The Calvin cycle fixes carbon in the stroma. Exam question hint: know the difference between C3 and C4 plants.",
        expect: &["thylakoid", "calvin", "c4"],
        reject: &[],
    },
    Case {
        name: "book notes",
        dictation: "Notes on the book. The author's main argument is that habits form around cues, not goals. The two-minute rule: start with a version that takes two minutes. I want to try habit stacking with my morning coffee.",
        expect: &["habits", "two-minute", "stacking"],
        reject: &[],
    },
    Case {
        name: "project planning with phases",
        dictation: "Planning the garden project. For the beds, we need two raised frames on the south side and a soil delivery, roughly one cubic yard. For planting, tomatoes and basil in May, garlic in the fall. Budget cap is three hundred dollars total.",
        expect: &["raised", "tomatoes", "three hundred"],
        reject: &[],
    },
    Case {
        name: "short note does not invent structure",
        dictation: "Note that the wifi password changed to sunflower ninety two.",
        expect: &["wifi", "sunflower"],
        reject: &["router", "firmware"],
    },
    Case {
        name: "meeting decisions and follow-ups",
        dictation: "Notes from the vendor call. They can ship the new badge printers by the eighth. Pricing drops four percent if we commit to two years. Decision: we're piloting five units first. Follow up with legal about the data processing addendum.",
        expect: &["badge printers", "four percent", "legal"],
        reject: &[],
    },
];

#[derive(Serialize)]
struct CaseFailure {
    run: usize,
    profile: &'static str,
    case: &'static str,
    missing: Vec<&'static str>,
    leaked: Vec<&'static str>,
    error: Option<String>,
}

#[derive(Serialize)]
struct ProfileQuality {
    profile: &'static str,
    passed: usize,
    total: usize,
    pass_pct: f64,
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
        "warmups_per_profile": args.warmups,
        "n_threads": config.n_threads,
        "n_ctx": config.n_ctx,
        "max_tokens": config.max_tokens,
        "production_deadline_ms": PRODUCTION_DEADLINE_MS,
        "profiles": ["agent-prompt", "email", "notes-outline"],
        "execution_scope": {
            "included": ["LlamaEngine model load", "production profile session creation/KV prompt warm", "grammar-constrained generation", "profile postprocess and grounding"],
            "excluded": ["audio/ASR", "LlmRunner queue and cancellation", "screen-context capture and prompt merge", "overlay/output delivery"],
            "screen_context": "excluded from this fair model A/B; acquisition is target-dependent and normally overlaps recording, so it must not be folded into model inference latency",
        },
    });

    eprintln!(
        "[extraction_ab] loading {} on {:?}...",
        args.model_path.display(),
        args.backend
    );
    let started = Instant::now();
    let engine = LlamaEngine::load(config).map_err(|error| error.to_string())?;
    let load_ms = support::elapsed_ms(started);
    let suites: &[(&str, &[Case])] = &[
        ("agent-prompt", AGENT_CASES),
        ("email", EMAIL_CASES),
        ("notes-outline", NOTES_CASES),
    ];

    let mut session_warm_ms = Vec::with_capacity(suites.len());
    let mut first_inference_ms = Vec::with_capacity(suites.len());
    let mut warm_samples_ms = Vec::new();
    let mut total = 0usize;
    let mut total_pass = 0usize;
    let mut failures = Vec::new();
    let mut profile_quality = Vec::new();
    let mut cold_end_to_end_ms = None;
    let mut cold_first_inference_error = None;

    for (profile_index, (profile_id, cases)) in suites.iter().enumerate() {
        let profile = profiles::get(profile_id);
        let started = Instant::now();
        let mut session = engine
            .new_session_for(profile)
            .map_err(|error| format!("failed to create {} session: {error}", profile.id))?;
        session_warm_ms.push(support::elapsed_ms(started));

        let started = Instant::now();
        let cold_input = STRUCTURED_WARMUP_DICTATIONS[profile_index];
        let first_outcome = session
            .generate_raw(cold_input, &[], None)
            .and_then(|raw| (profile.postprocess)(&raw, cold_input));
        let first_ms = support::elapsed_ms(started);
        first_inference_ms.push(first_ms);
        if profile_index == 0 {
            cold_end_to_end_ms = Some(
                load_ms
                    .saturating_add(session_warm_ms[0])
                    .saturating_add(first_ms),
            );
            cold_first_inference_error = first_outcome.err().map(|error| error.to_string());
        }
        for warmup in 0..args.warmups {
            let warmup_input = STRUCTURED_WARMUP_DICTATIONS
                [(profile_index + warmup + 1) % STRUCTURED_WARMUP_DICTATIONS.len()];
            let _ = session
                .generate_raw(warmup_input, &[], None)
                .and_then(|raw| (profile.postprocess)(&raw, warmup_input));
        }

        let mut profile_pass = 0usize;
        for run in 0..args.runs {
            eprintln!(
                "[extraction_ab] profile={} run {}/{}",
                profile.id,
                run + 1,
                args.runs
            );
            for case in *cases {
                let started = Instant::now();
                let outcome = session
                    .generate_raw(case.dictation, &[], None)
                    .and_then(|raw| (profile.postprocess)(&raw, case.dictation));
                warm_samples_ms.push(support::elapsed_ms(started));
                total += 1;

                match outcome {
                    Ok(output) => {
                        let markdown = output.markdown.to_lowercase();
                        let missing = case
                            .expect
                            .iter()
                            .filter(|keyword| !markdown.contains(&keyword.to_lowercase()))
                            .copied()
                            .collect::<Vec<_>>();
                        let leaked = case
                            .reject
                            .iter()
                            .filter(|keyword| markdown.contains(&keyword.to_lowercase()))
                            .copied()
                            .collect::<Vec<_>>();
                        if missing.is_empty() && leaked.is_empty() {
                            total_pass += 1;
                            profile_pass += 1;
                        } else {
                            failures.push(CaseFailure {
                                run,
                                profile: profile.id,
                                case: case.name,
                                missing,
                                leaked,
                                error: None,
                            });
                        }
                    }
                    Err(error) => failures.push(CaseFailure {
                        run,
                        profile: profile.id,
                        case: case.name,
                        missing: Vec::new(),
                        leaked: Vec::new(),
                        error: Some(error.to_string()),
                    }),
                }
            }
        }
        let profile_total = args.runs * cases.len();
        profile_quality.push(ProfileQuality {
            profile: profile.id,
            passed: profile_pass,
            total: profile_total,
            pass_pct: 100.0 * profile_pass as f64 / profile_total as f64,
        });
    }

    let pass_pct = 100.0 * total_pass as f64 / total as f64;
    let cold_end_to_end_ms = cold_end_to_end_ms
        .ok_or_else(|| "structured suite did not produce a cold end-to-end sample".to_string())?;
    let deadline_passed =
        cold_first_inference_error.is_none() && cold_end_to_end_ms <= PRODUCTION_DEADLINE_MS;
    let backend_observation = support::BackendObservation::model_path(
        args.backend,
        "Vulkan was requested and the LlamaEngine loaded, but the llama_cpp API used here exposes neither an actual offloaded-layer count nor fallback device",
    );
    support::finish_report(support::ReportInput {
        benchmark: "structured_extraction_ab".to_string(),
        harness_sha256: support::harness_sha256(&[
            include_bytes!("extraction_ab.rs"),
            include_bytes!("support/mod.rs"),
        ]),
        production: support::production_fingerprint(&[
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
            ("src/llm/types.rs", include_bytes!("../src/llm/types.rs")),
        ]),
        backend_observation,
        config: config_json,
        phases: vec![
            support::PhaseReport::new("model_load", "cold_component", vec![load_ms]),
            support::PhaseReport::new("profile_session_ready", "cold_component", session_warm_ms),
            support::PhaseReport::new(
                "first_profile_inference",
                "cold_component",
                first_inference_ms,
            ),
            support::PhaseReport::new(
                "cold_end_to_end",
                "cold_end_to_end",
                vec![cold_end_to_end_ms],
            ),
            support::PhaseReport::new("corpus_inference", "warm", warm_samples_ms),
        ],
        quality: json!({
            "metric": "grounded_case_pass_pct",
            "quality_pct": pass_pct,
            "passed_cases": total_pass,
            "total_cases": total,
            "profiles": profile_quality,
            "cold_default_profile": "agent-prompt",
            "cold_first_inference_error": cold_first_inference_error,
            "cold_end_to_end_deadline_ms": PRODUCTION_DEADLINE_MS,
            "cold_end_to_end_within_deadline": deadline_passed,
            "failures": failures,
        }),
        quality_pct: pass_pct,
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
