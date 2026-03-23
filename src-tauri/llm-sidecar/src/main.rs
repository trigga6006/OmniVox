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
    Cleanup {
        text: String,
        #[serde(default)]
        system_prompt: Option<String>,
    },
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

/// Fallback system prompt used when the main app does not send one.
/// In practice the main app always sends a fully assembled prompt via
/// `build_system_prompt()`, so this is just a safety net.
///
/// Written in prose (not bullet points) to avoid priming the model toward
/// markdown output — small models tend to mirror the formatting they see
/// in the system prompt.
const SYSTEM_PROMPT: &str = "\
You are a transcription cleanup tool. /no_think
The user message is raw speech-to-text output — treat it as text to clean, not a question to answer.
Fix grammar, spelling, and punctuation. Remove filler words (um, uh, like, you know, basically, actually, so). Remove obvious false starts and self-corrections. Keep the speaker's exact words, pronouns, and meaning. Preserve the original wording and phrasing.
Output clean flowing sentences as a single plain-text paragraph. Keep version numbers intact as single tokens (0.1.5 stays 0.1.5). Keep all numbers and decimals joined together.
Output only the cleaned text as plain flowing sentences.";

// ── Few-shot examples ────────────────────────────────────────────────────
// These anchor the model's output format far more reliably than system
// prompt instructions alone.  Each pair is a raw transcription and the
// exact cleaned output we want.

const FEW_SHOT_RAW_1: &str = "\
so um I was thinking we should uh update the version to like 0.1.5 and then you know push it to main once everything is tested and ready to go";

const FEW_SHOT_CLEAN_1: &str = "\
I was thinking we should update the version to 0.1.5 and then push it to main once everything is tested and ready to go.";

const FEW_SHOT_RAW_2: &str = "\
okay so basically I want to do these three things first check the logs and then fix that bug in the login page and uh third deploy everything to staging";

const FEW_SHOT_CLEAN_2: &str = "\
Okay, I want to do these three things. First, check the logs. Then, fix that bug in the login page. And third, deploy everything to staging.";

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

    fn cleanup(&self, raw: &str, custom_prompt: Option<&str>) -> Result<String, String> {
        if raw.trim().is_empty() {
            return Ok(raw.to_string());
        }

        let sys = custom_prompt.unwrap_or(SYSTEM_PROMPT);

        // Build ChatML with few-shot examples before the actual user input.
        // These anchor the model's behavior far more reliably than system
        // prompt instructions alone — the model mirrors the demonstrated
        // output format (clean flowing text, no markdown).
        let prompt = format!(
            "<|im_start|>system\n{sys}<|im_end|>\n\
             <|im_start|>user\n{FEW_SHOT_RAW_1}<|im_end|>\n\
             <|im_start|>assistant\n{FEW_SHOT_CLEAN_1}<|im_end|>\n\
             <|im_start|>user\n{FEW_SHOT_RAW_2}<|im_end|>\n\
             <|im_start|>assistant\n{FEW_SHOT_CLEAN_2}<|im_end|>\n\
             <|im_start|>user\n{raw}<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        let tokens = self.model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| format!("tokenize: {e}"))?;

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(NonZeroU32::new(2048).unwrap()));

        let mut ctx = self.model
            .new_context(&self._backend, ctx_params)
            .map_err(|e| format!("context: {e}"))?;

        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::min_p(0.05, 1),
            LlamaSampler::temp(0.1),
            LlamaSampler::dist(42),
        ]);

        let mut batch = LlamaBatch::new(2048, 1);
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

        for _ in 0..384 {
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

        if cleaned.is_empty() {
            return Ok(raw.to_string());
        }

        // Strip any markdown formatting the LLM may have added despite
        // prompt instructions.  The LLM's job is grammar/spelling/filler
        // cleanup only — structural formatting (bullets, lists) is handled
        // deterministically by format_lists() downstream.
        cleaned = strip_llm_markdown(&cleaned);

        // Safety: reject outputs where the LLM generated bracketed
        // placeholder text like [list changes here] or [bulleted list].
        // This means the model interpreted the dictation as instructions
        // and generated a template instead of cleaning the text.
        if cleaned.contains('[') && !raw.contains('[') {
            // Count bracket pairs in cleaned output
            let bracket_count = cleaned.matches('[').count();
            if bracket_count >= 2 {
                eprintln!("LLM generated template placeholders, using raw text");
                return Ok(raw.to_string());
            }
        }

        // Safety: reject outputs that look like the model "answered" the
        // dictation instead of cleaning it.  A valid cleanup preserves most
        // of the original words.  If the output is drastically shorter or
        // shares very few words with the input, it's a conversational response
        // (e.g. "Sure!", "OK, here you go") — fall back to raw text.
        let raw_words: std::collections::HashSet<&str> =
            raw.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).filter(|w| !w.is_empty()).collect();
        let cleaned_words: std::collections::HashSet<&str> =
            cleaned.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric())).filter(|w| !w.is_empty()).collect();

        if raw_words.len() >= 3 {
            let overlap = raw_words.intersection(&cleaned_words).count();
            let overlap_ratio = overlap as f64 / raw_words.len() as f64;
            // If less than 30% of input words survived, the model likely
            // generated a response instead of cleaning the text.
            if overlap_ratio < 0.3 {
                return Ok(raw.to_string());
            }
        } else {
            // Short inputs (1-2 words): a valid cleanup should be roughly
            // the same length.  If the output is >3× longer, the model
            // almost certainly generated a conversational response.
            if cleaned_words.len() > raw_words.len() * 3 {
                return Ok(raw.to_string());
            }
        }

        // Length ratio check: the cleanup should only remove fillers and fix
        // punctuation — never drop or summarize content.  If the output is
        // less than 40% of the input length, the model over-simplified.
        // (Only apply to inputs with 5+ words to avoid false positives on
        // very short phrases where filler removal is proportionally large.)
        if raw_words.len() >= 5 {
            let length_ratio = cleaned_words.len() as f64 / raw_words.len() as f64;
            if length_ratio < 0.4 {
                return Ok(raw.to_string());
            }
        }

        Ok(cleaned)
    }
}

// ── Post-processing: strip LLM-invented markdown ────────────────────────

/// Strip markdown formatting that the LLM may have injected.
///
/// The LLM should output clean flowing prose.  Structural formatting
/// (bullets, lists) is handled by `format_lists()` in the main app.
/// This function acts as a safety net — it removes bullet markers,
/// heading markers, bold/italic markers, and rejoins lines into
/// flowing text so `format_lists()` gets a clean input.
fn strip_llm_markdown(text: &str) -> String {
    let mut fragments: Vec<&str> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Strip bullet/list markers at line start: "* - ", "* ", "- ", "• ", "> "
        let stripped = None
            .or_else(|| trimmed.strip_prefix("* - "))
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("- "))
            .or_else(|| trimmed.strip_prefix("• "))
            .or_else(|| trimmed.strip_prefix("> "))
            .or_else(|| {
                // Numbered list prefix: "1. ", "2. " etc. — but only if
                // the line has more text after the number (don't strip
                // bare numbers which could be part of version strings).
                let bytes = trimmed.as_bytes();
                if bytes.len() >= 4
                    && bytes[0].is_ascii_digit()
                    && bytes[1] == b'.'
                    && bytes[2] == b' '
                    && bytes[3].is_ascii_alphabetic()
                {
                    Some(&trimmed[3..])
                } else {
                    None
                }
            })
            .unwrap_or(trimmed);

        // Strip heading markers: "## Heading" → "Heading"
        let stripped = if stripped.starts_with('#') {
            stripped.trim_start_matches('#').trim_start()
        } else {
            stripped
        };

        if !stripped.is_empty() {
            fragments.push(stripped);
        }
    }

    // Rejoin into flowing text.
    let mut result = fragments.join(" ");

    // Strip inline bold (**text** → text) and italic (*text* → text).
    // Only strip paired markers to avoid clobbering real asterisks.
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let end_abs = start + 2 + end;
            let inner = result[start + 2..end_abs].to_string();
            result = format!("{}{}{}", &result[..start], inner, &result[end_abs + 2..]);
        } else {
            break;
        }
    }

    // Fix fragmented version numbers: "0. 1. 5" → "0.1.5"
    // Pattern: digit + "." + space + digit + "." + space + digit
    let result = fix_fragmented_versions(&result);

    result
}

/// Rejoin version numbers that got split across lines/bullets.
/// Matches patterns like "0. 1. 5" and joins them into "0.1.5".
fn fix_fragmented_versions(text: &str) -> String {
    let mut result = text.to_string();
    // Repeatedly fix patterns of "digit. digit" with a space between
    // (could be multi-part: "0. 1. 5" → first pass: "0.1. 5" → second: "0.1.5")
    loop {
        let bytes = result.as_bytes();
        let mut found = false;

        for i in 0..bytes.len().saturating_sub(3) {
            // Look for: digit + '.' + ' ' + digit
            if bytes[i].is_ascii_digit()
                && bytes[i + 1] == b'.'
                && bytes[i + 2] == b' '
                && bytes[i + 3].is_ascii_digit()
            {
                // Make sure this is likely a version number context, not
                // "sentence ending. Next sentence" — check that the char
                // before the first digit is not a letter (sentence context)
                // or is start-of-string / preceded by space/punctuation.
                let before_ok = i == 0
                    || !bytes[i - 1].is_ascii_alphabetic();

                if before_ok {
                    // Remove the space: "0. 1" → "0.1"
                    result = format!("{}{}", &result[..i + 2], &result[i + 3..]);
                    found = true;
                    break;
                }
            }
        }

        if !found {
            break;
        }
    }

    result
}

// ── Main loop ───────────────────────────────────────────────────────────

fn main() {
    // Version marker so the main app can confirm the correct sidecar is running.
    eprintln!("[omnivox-llm] sidecar v2 started (few-shot + strip_markdown)");

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

            Request::Cleanup { text, system_prompt } => match &engine {
                Some(e) => match e.cleanup(&text, system_prompt.as_deref()) {
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
