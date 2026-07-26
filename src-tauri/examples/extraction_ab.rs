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
//!   cargo run --release --example extraction_ab --features vulkan -- <model.gguf>
//! or set OMNIVOX_LLM_MODEL to the GGUF path (mirrors llm_probe.rs).
//!
//! Notes:
//!   - Runs on CPU (use_gpu=false) so it never fights the live app for the
//!     GPU. Accuracy is hardware-independent (grammar-constrained greedy
//!     decode); the printed latency is a CPU proxy.
//!   - One KV session per profile: the system prompt prefills once, each
//!     case only pays for its own words — same shape as the app's hot path.

use omnivoice_lib::llm::engine::{LlamaEngine, LlmEngine};
use omnivoice_lib::llm::profiles;
use omnivoice_lib::llm::types::LlmConfig;

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

fn main() {
    let model_path = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("OMNIVOX_LLM_MODEL").ok())
        .or_else(|| {
            dirs::data_dir().map(|d| {
                d.join("omnivox")
                    .join("llm_models")
                    .join("Qwen3-1.7B-Q8_0.gguf")
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .expect("pass a model path as arg 1 or set OMNIVOX_LLM_MODEL");

    let config = LlmConfig {
        model_path: model_path.clone(),
        use_gpu: false,
        ..LlmConfig::default()
    };

    eprintln!("[extraction_ab] loading {model_path} ...");
    let engine = LlamaEngine::load(config).expect("failed to load model");

    let suites: &[(&str, &[Case])] = &[
        ("agent-prompt", AGENT_CASES),
        ("email", EMAIL_CASES),
        ("notes-outline", NOTES_CASES),
    ];

    let mut total = 0usize;
    let mut total_pass = 0usize;

    println!("\n============== Structured Mode extraction A/B ==============");
    println!("model: {model_path}");

    for (profile_id, cases) in suites {
        let profile = profiles::get(profile_id);
        // One warmed session per profile — mirrors the app's hot path and
        // amortizes the system-prompt prefill across the suite.
        let mut session = engine
            .new_session_for(profile)
            .expect("failed to create session");

        let mut pass = 0usize;
        let mut suite_ms = 0u128;
        println!("\n-- profile: {} ({} cases)", profile.id, cases.len());

        for case in *cases {
            let t0 = std::time::Instant::now();
            let outcome = session
                .generate_raw(case.dictation, &[], None)
                .and_then(|raw| (profile.postprocess)(&raw, case.dictation));
            let ms = t0.elapsed().as_millis();
            suite_ms += ms;

            match outcome {
                Ok(out) => {
                    let md = out.markdown.to_lowercase();
                    let missing: Vec<&str> = case
                        .expect
                        .iter()
                        .filter(|kw| !md.contains(&kw.to_lowercase()))
                        .copied()
                        .collect();
                    let leaked: Vec<&str> = case
                        .reject
                        .iter()
                        .filter(|kw| md.contains(&kw.to_lowercase()))
                        .copied()
                        .collect();
                    if missing.is_empty() && leaked.is_empty() {
                        pass += 1;
                        println!("  PASS ({ms:>5}ms) {}", case.name);
                    } else {
                        println!("  FAIL ({ms:>5}ms) {}", case.name);
                        if !missing.is_empty() {
                            println!("       missing: {missing:?}");
                        }
                        if !leaked.is_empty() {
                            println!("       leaked (fabrication): {leaked:?}");
                        }
                        println!(
                            "       output: {}",
                            out.markdown.replace('\n', "\n               ")
                        );
                    }
                }
                Err(e) => {
                    println!("  FAIL ({ms:>5}ms) {} — error: {e}", case.name);
                }
            }
        }

        total += cases.len();
        total_pass += pass;
        println!(
            "-- {}: {pass}/{} pass, avg {} ms/case",
            profile.id,
            cases.len(),
            suite_ms / cases.len() as u128
        );
    }

    println!("\ntotal: {total_pass}/{total} pass");
    println!("=============================================================\n");
}
