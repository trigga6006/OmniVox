//! Command-Mode Brain A/B harness.
//!
//! Runs a fixed, labeled set of spoken-command utterances through an LLM's
//! `classify_command` path (the SAME grammar + prompt the app uses) and scores
//! how well the model maps speech → the closed `CommandIntent` enum. Use it to
//! A/B the current Qwen3-1.7B "command brain" against function-calling-tuned
//! candidates (xLAM-2, Hammer2.1, …) by pointing it at different GGUFs.
//!
//! Usage:
//!   cargo run --example command_ab --features vulkan -- <path-to-model.gguf>
//!
//! Notes:
//!   - Runs on CPU (use_gpu=false) so it never fights the live app for the GPU.
//!     ACCURACY is hardware-independent (grammar-constrained greedy decode);
//!     the printed latency is a CPU proxy, not the app's Vulkan latency.
//!   - It deliberately bypasses the tier-1 matcher so EVERY case stresses the
//!     LLM — including the easy fixed phrases the matcher normally handles.

use omnivoice_lib::actions::CommandIntent;
use omnivoice_lib::llm::engine::{LlamaEngine, LlmEngine};
use omnivoice_lib::llm::types::LlmConfig;

/// (utterance, expected_action, expected_target). `expected_action == "none"`
/// means the model SHOULD decline (not a command / unsupported) → predict None.
const CASES: &[(&str, &str, &str)] = &[
    // open_app — varied natural phrasings
    ("open spotify", "open_app", "spotify"),
    ("bring up chrome", "open_app", "chrome"),
    ("i want to listen to some music", "open_app", "spotify"),
    ("fire up discord", "open_app", "discord"),
    ("can you launch visual studio code for me", "open_app", "visual studio code"),
    ("pull up my email", "open_app", "outlook"),
    // web_search / open_url
    ("make a google search", "web_search", ""),
    ("search for the best pizza in chicago", "web_search", "best pizza in chicago"),
    ("look up the weather in denver", "web_search", "weather in denver"),
    ("google how to tie a tie", "web_search", "how to tie a tie"),
    ("go to youtube", "open_url", "youtube.com"),
    ("open the github website", "open_url", "github.com"),
    // keyboard chords
    ("copy that", "copy", ""),
    ("paste it here", "paste", ""),
    ("undo that", "undo", ""),
    ("select everything", "select_all", ""),
    ("save the file", "save", ""),
    ("open a new tab", "new_tab", ""),
    ("close this tab", "close_tab", ""),
    ("take a screenshot", "screenshot", ""),
    // NEW: show_desktop + close_window
    ("show me the desktop", "show_desktop", ""),
    ("minimize everything", "show_desktop", ""),
    ("hide all my windows", "show_desktop", ""),
    ("close this window", "close_window", ""),
    ("close this window please", "close_window", ""),
    ("get this window off my screen", "close_window", ""),
    // media + window
    ("turn it down", "volume_down", ""),
    ("turn the volume up", "volume_up", ""),
    ("skip this song", "next_track", ""),
    ("mute it", "mute", ""),
    ("pause the music", "play_pause", ""),
    ("shrink this window", "minimize", ""),
    ("make this full screen", "maximize", ""),
    // none — not a command / unsupported
    ("what time is it", "none", ""),
    ("close internet explorer", "none", ""),   // named-app close not supported
    ("quit photoshop", "none", ""),            // named-app quit not supported
    ("the quick brown fox jumps over the lazy dog", "none", ""),
    ("remind me to call my mom at five", "none", ""),
];

/// Variant-level match (ignores free-text targets) — "did it pick the right action?"
fn action_eq(a: &CommandIntent, b: &CommandIntent) -> bool {
    use CommandIntent::*;
    match (a, b) {
        (OpenApp(_), OpenApp(_)) => true,
        (WebSearch(_), WebSearch(_)) => true,
        (OpenUrl(_), OpenUrl(_)) => true,
        (KeyChord(x), KeyChord(y)) => x == y,
        (Media(x), Media(y)) => x == y,
        (Window(x), Window(y)) => x == y,
        (CloseWindow, CloseWindow) => true,
        _ => false,
    }
}

/// Full match — variant AND (case-insensitively) the target string.
fn full_eq(a: &CommandIntent, b: &CommandIntent) -> bool {
    use CommandIntent::*;
    match (a, b) {
        (OpenApp(x), OpenApp(y)) | (WebSearch(x), WebSearch(y)) | (OpenUrl(x), OpenUrl(y)) => {
            x.trim().eq_ignore_ascii_case(y.trim())
        }
        _ => a == b,
    }
}

fn expected_intent(action: &str, target: &str) -> Option<CommandIntent> {
    if action == "none" {
        None
    } else {
        CommandIntent::from_llm(action, target)
    }
}

fn fmt(o: &Option<CommandIntent>) -> String {
    match o {
        None => "none".to_string(),
        Some(i) => format!("{i:?}"),
    }
}

fn main() {
    let model_path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: cargo run --example command_ab --features vulkan -- <model.gguf>");
        std::process::exit(2);
    });

    let config = LlmConfig {
        model_path: model_path.clone(),
        n_threads: std::thread::available_parallelism()
            .map(|n| (n.get().saturating_sub(2)).clamp(2, 8) as i32)
            .unwrap_or(4),
        use_gpu: false,
        n_ctx: 1024,
        max_tokens: 64,
    };

    eprintln!("[command_ab] loading {model_path} ...");
    let engine = LlamaEngine::load(config).expect("failed to load model");

    let mut action_hits = 0usize;
    let mut full_hits = 0usize;
    let mut none_total = 0usize;
    let mut none_hits = 0usize;
    let mut total_ms = 0u128;
    let mut misses: Vec<String> = Vec::new();

    for (utt, act, tgt) in CASES {
        let expected = expected_intent(act, tgt);
        if *act == "none" {
            none_total += 1;
        }

        let t0 = std::time::Instant::now();
        // classify_command now returns an ordered chain; this A/B harness scores
        // single-intent accuracy, so collapse to the first step.
        let predicted = engine
            .classify_command(utt)
            .unwrap_or_default()
            .into_iter()
            .next();
        total_ms += t0.elapsed().as_millis();

        let action_ok = match (&predicted, &expected) {
            (None, None) => true,
            (Some(p), Some(e)) => action_eq(p, e),
            _ => false,
        };
        let full_ok = match (&predicted, &expected) {
            (None, None) => true,
            (Some(p), Some(e)) => full_eq(p, e),
            _ => false,
        };
        if action_ok {
            action_hits += 1;
        }
        if full_ok {
            full_hits += 1;
        }
        if *act == "none" && predicted.is_none() {
            none_hits += 1;
        }
        if !action_ok {
            misses.push(format!(
                "  ✗ {:<48} got {:<28} want {}",
                format!("\"{utt}\""),
                fmt(&predicted),
                fmt(&expected),
            ));
        }
    }

    let n = CASES.len();
    println!("\n================ Command Brain A/B ================");
    println!("model:            {model_path}");
    println!("cases:            {n}");
    println!(
        "action accuracy:  {action_hits}/{n}  ({:.1}%)",
        100.0 * action_hits as f64 / n as f64
    );
    println!(
        "full accuracy:    {full_hits}/{n}  ({:.1}%)   (action + target string)",
        100.0 * full_hits as f64 / n as f64
    );
    println!(
        "none precision:   {none_hits}/{none_total}  (correctly declined non-commands)"
    );
    println!("avg latency:      {} ms/utterance (CPU proxy)", total_ms / n as u128);
    if misses.is_empty() {
        println!("\nno action misses 🎉");
    } else {
        println!("\naction misses ({}):", misses.len());
        for m in &misses {
            println!("{m}");
        }
    }
    println!("==================================================\n");
}
