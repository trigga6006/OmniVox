use omnivoice_lib::llm::engine::{LlamaEngine, LlmEngine};
use omnivoice_lib::llm::types::LlmConfig;

fn main() {
    let model_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| {
            r"C:\Users\fowle\AppData\Roaming\omnivox\llm_models\Qwen3-1.7B-Q4_K_M.gguf"
                .to_string()
        });

    let input = std::env::args().nth(2).unwrap_or_else(|| {
        "Hi, Codex. I want you to go through this entire codebase. Tighten up the dictation app, make sure the structured mode plumbing is solid, and keep the UI and model setup aligned with the current Qwen path."
            .to_string()
    });

    let config = LlmConfig {
        model_path,
        n_threads: std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(2).max(2).min(8) as i32)
            .unwrap_or(4),
        use_gpu: false,
        n_ctx: 2048,
        max_tokens: 192,
    };

    eprintln!("[probe] loading model...");
    let engine = LlamaEngine::load(config).expect("failed to load model");

    eprintln!("[probe] running extract_raw...");
    match engine.extract_raw(&input) {
        Ok(raw) => {
            println!("RAW_JSON:\n{}\n", raw.raw_json);
            println!("RAW_META: duration_ms={}, model_name={}", raw.duration_ms, raw.model_name);
        }
        Err(e) => {
            eprintln!("extract_raw failed: {e}");
        }
    }

    eprintln!("[probe] running extract_slots...");
    match engine.extract_slots(&input) {
        Ok(slots) => {
            println!("SLOTS_OK:\n{:?}", slots);
        }
        Err(e) => {
            eprintln!("extract_slots failed: {e}");
        }
    }
}
