//! Lightweight LLM sidecar process for OmniVox.
//!
//! Runs as a child process of the main app.  Reads JSON-line commands from
//! stdin and writes JSON-line responses to stdout.  This process isolation
//! avoids the GGML symbol collision between whisper-rs and llama-cpp-2 on
//! Windows — each process has its own copy of GGML in a separate address
//! space.

use std::io::{self, BufRead, Write};
use std::num::NonZeroU32;
use std::pin::pin;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use serde::{Deserialize, Serialize};

// ── Protocol types ──────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "cmd")]
enum Request {
    #[serde(rename = "load")]
    Load { model_path: String },
    #[serde(rename = "cleanup")]
    Cleanup { text: String },
    #[serde(rename = "unload")]
    Unload,
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Serialize)]
struct Response {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl Response {
    fn ok() -> Self {
        Self { ok: true, text: None, error: None }
    }
    fn text(s: String) -> Self {
        Self { ok: true, text: Some(s), error: None }
    }
    fn err(msg: String) -> Self {
        Self { ok: false, text: None, error: Some(msg) }
    }
}

// ── LLM engine (same logic as the previous in-process engine) ───────────

const SYSTEM_PROMPT: &str = "\
You are a dictation cleanup assistant. /no_think
Clean the following transcribed speech:
- Remove filler words (um, uh, like, you know, so, basically, actually)
- Fix grammar, spelling, and punctuation
- Handle self-corrections (keep the intended word, remove false starts)
- Preserve the speaker's intended meaning exactly
- Do not add information or change meaning
Output ONLY the cleaned text, nothing else. No commentary, no tags, no explanation.";

struct Engine {
    _backend: LlamaBackend,
    model: LlamaModel,
}

impl Engine {
    fn load(path: &str) -> Result<Self, String> {
        let mut backend = LlamaBackend::init()
            .map_err(|e| format!("backend init: {e}"))?;
        backend.void_logs();

        let params = pin!(LlamaModelParams::default());
        let model = LlamaModel::load_from_file(&backend, path, &params)
            .map_err(|e| format!("model load: {e}"))?;

        Ok(Self { _backend: backend, model })
    }

    fn cleanup(&self, raw: &str) -> Result<String, String> {
        if raw.trim().is_empty() {
            return Ok(raw.to_string());
        }

        let prompt = format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n\
             <|im_start|>user\n{raw}<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        let tokens = self.model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| format!("tokenize: {e}"))?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(512).unwrap()));

        let mut ctx = self.model
            .new_context(&self._backend, ctx_params)
            .map_err(|e| format!("context: {e}"))?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::min_p(0.05, 1),
            LlamaSampler::temp(0.1),
            LlamaSampler::dist(42),
        ]);

        let mut batch = LlamaBatch::new(512, 1);
        let last_index = (tokens.len() - 1) as i32;
        for (i, token) in (0_i32..).zip(tokens.iter().copied()) {
            batch.add(token, i, &[0], i == last_index)
                .map_err(|e| format!("batch add: {e}"))?;
        }

        ctx.decode(&mut batch)
            .map_err(|e| format!("prompt decode: {e}"))?;

        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut n_cur = batch.n_tokens();

        for _ in 0..256 {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            if self.model.is_eog_token(token) {
                break;
            }

            if let Ok(piece) = self.model.token_to_piece(token, &mut decoder, false, None) {
                if piece.contains("<|im_end|>") || piece.contains("<|im_start|>") {
                    break;
                }
                output.push_str(&piece);
            }

            batch.clear();
            batch.add(token, n_cur, &[0], true)
                .map_err(|e| format!("batch add: {e}"))?;
            ctx.decode(&mut batch)
                .map_err(|e| format!("decode: {e}"))?;
            n_cur += 1;
        }

        let mut cleaned = output.trim().to_string();

        // Strip Qwen3 <think>...</think> blocks if present.
        if let Some(end) = cleaned.find("</think>") {
            cleaned = cleaned[end + "</think>".len()..].trim().to_string();
        }

        if cleaned.is_empty() { Ok(raw.to_string()) } else { Ok(cleaned) }
    }
}

// ── Main loop ───────────────────────────────────────────────────────────

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut engine: Option<Engine> = None;

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.trim().is_empty() {
            continue;
        }

        let req: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let _ = respond(&mut stdout, Response::err(format!("parse: {e}")));
                continue;
            }
        };

        let resp = match req {
            Request::Ping => Response::ok(),

            Request::Load { model_path } => {
                // Load on a thread with a large stack (debug builds need it).
                match std::thread::Builder::new()
                    .stack_size(128 * 1024 * 1024)
                    .spawn(move || Engine::load(&model_path))
                    .and_then(|h| h.join().map_err(|_| io::Error::other("thread panicked")))
                {
                    Ok(Ok(e)) => {
                        engine = Some(e);
                        Response::ok()
                    }
                    Ok(Err(msg)) => Response::err(msg),
                    Err(e) => Response::err(format!("spawn: {e}")),
                }
            }

            Request::Cleanup { text } => match &engine {
                Some(e) => match e.cleanup(&text) {
                    Ok(t) => Response::text(t),
                    Err(msg) => Response::err(msg),
                },
                None => Response::err("no model loaded".into()),
            },

            Request::Unload => {
                engine = None;
                Response::ok()
            }
        };

        if respond(&mut stdout, resp).is_err() {
            break;
        }
    }
}

fn respond(w: &mut impl Write, r: Response) -> io::Result<()> {
    let json = serde_json::to_string(&r).unwrap();
    writeln!(w, "{json}")?;
    w.flush()
}
