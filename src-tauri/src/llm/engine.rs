use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::error::{AppError, AppResult};
use crate::llm::grammar::{SLOT_EXTRACTION_ROOT, SLOT_EXTRACTION_V1};
use crate::llm::prompt::{format_prompt, format_prompt_with_context};
use crate::llm::schema::SlotExtraction;
use crate::llm::types::{LlmConfig, LlmInferenceResult};

/// Blocking trait implemented by the production `LlamaEngine` and by the test
/// mock.  The pipeline takes `Arc<dyn LlmEngine>`-adjacent values so tests can
/// exercise the Structured Mode branch without loading a real model.
pub trait LlmEngine: Send + Sync {
    /// Run one grammar-constrained slot extraction on `user_text`.
    fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction>;

    /// Slot extraction with optional screen context.
    ///
    /// Default impl ignores context and delegates to `extract_slots` so test
    /// mocks compile unchanged.  The production `LlamaEngine` overrides this
    /// to feed the tokens into the user turn for verbatim reconciliation.
    fn extract_slots_with_context(
        &self,
        user_text: &str,
        _screen_tokens: &[String],
        _source_app: Option<&str>,
    ) -> AppResult<SlotExtraction> {
        self.extract_slots(user_text)
    }

    /// Raw single-shot inference — exposed for diagnostics and the Settings
    /// "Test" button.  Default impl just calls `extract_slots` and serializes
    /// the result; concrete engines may override to expose the unparsed JSON.
    fn extract_raw(&self, user_text: &str) -> AppResult<LlmInferenceResult> {
        let t0 = std::time::Instant::now();
        let slots = self.extract_slots(user_text)?;
        Ok(LlmInferenceResult {
            raw_json: serde_json::to_string(&slots)
                .map_err(|e| AppError::Llm(format!("serialize: {e}")))?,
            duration_ms: t0.elapsed().as_millis() as u64,
            model_name: String::new(),
        })
    }
}

/// Lazily-initialized llama.cpp backend.  Init must happen exactly once per
/// process — subsequent calls return `BackendAlreadyInitialized`.  We treat
/// that as a benign race and keep the first backend.
fn backend() -> AppResult<&'static LlamaBackend> {
    static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();
    if let Some(b) = BACKEND.get() {
        return Ok(b);
    }
    let b = LlamaBackend::init()
        .map_err(|e| AppError::Llm(format!("llama backend init failed: {e}")))?;
    // Ignore the return value — if another thread raced us we use whichever
    // won.  Both are equivalent; LlamaBackend is stateless beyond its init.
    let _ = BACKEND.set(b);
    // Safe: we just filled it or another thread did.
    Ok(BACKEND.get().expect("backend just initialized"))
}

/// Production llama.cpp-backed slot extractor.
///
/// Owns a loaded `LlamaModel`.  A fresh `LlamaContext` is created per
/// extraction — context setup is milliseconds, and the KV cache is dead
/// weight once an extraction finishes, so there's no value in caching it.
pub struct LlamaEngine {
    model: LlamaModel,
    config: LlmConfig,
    model_name: String,
}

// SAFETY: after construction the LlamaModel is read-only — llama.cpp itself
// guards its internal mutable state behind per-context mutexes.  Contexts are
// always constructed inside the same function call as inference and dropped
// before returning, so no aliasing across threads.
unsafe impl Send for LlamaEngine {}
unsafe impl Sync for LlamaEngine {}

impl LlamaEngine {
    /// Load a GGUF model from disk.  Mirrors `WhisperEngine::load` — expensive
    /// (hundreds of ms for small quantized LLMs), so callers should run it on a
    /// dedicated 256 MB-stack thread as `models::load_and_activate_model` does.
    pub fn load(config: LlmConfig) -> AppResult<Self> {
        let path = Path::new(&config.model_path);
        if !path.exists() {
            return Err(AppError::Llm(format!(
                "LLM model file not found: {}",
                config.model_path
            )));
        }

        let backend = backend()?;

        // `n_gpu_layers = -1` means "offload everything llama.cpp can"; when
        // the `vulkan`/`cuda` feature is off this reduces to CPU anyway, so
        // it's safe to set unconditionally and honor `use_gpu` by toggling
        // between 0 (forced CPU) and i32::MAX (all layers).
        let model_params = LlamaModelParams::default().with_n_gpu_layers(if config.use_gpu {
            u32::MAX
        } else {
            0
        });

        let model_name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("llm-model")
            .to_string();

        let model = LlamaModel::load_from_file(backend, path, &model_params)
            .map_err(|e| AppError::Llm(format!("Failed to load LLM model: {e}")))?;

        // Validate context/KV-cache allocation during load so GPU OOM or
        // backend-driver problems fall into the caller's CPU fallback path
        // instead of surfacing on the first Structured Mode dictation.
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(config.n_ctx))
            .with_n_batch(config.n_ctx.max(512))
            .with_n_threads(config.n_threads);
        let ctx_probe = model
            .new_context(backend, ctx_params)
            .map_err(|e| AppError::Llm(format!("Failed to create LLM context: {e}")))?;
        drop(ctx_probe);

        Ok(Self {
            model,
            config,
            model_name,
        })
    }

    /// Run the generation loop for a single extraction.
    ///
    /// Invariants:
    /// - The sampler chain ends in `greedy` (matches hardcoded temp=0.0).
    /// - The grammar sampler is first in the chain, so token selection is
    ///   restricted to the GBNF alphabet before greedy picks the max-logit.
    /// - EOG stops generation; `max_tokens` bounds runaway loops.
    fn generate_json(
        &self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<String> {
        let backend = backend()?;

        // Build a context sized just above the prompt + output budget.  Any
        // spare capacity wastes KV cache RAM.
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(self.config.n_ctx))
            .with_n_batch(self.config.n_ctx.max(512))
            .with_n_threads(self.config.n_threads);

        let mut ctx = self
            .model
            .new_context(backend, ctx_params)
            .map_err(|e| AppError::Llm(format!("Failed to create LLM context: {e}")))?;

        let prompt = if screen_tokens.is_empty() {
            format_prompt(user_text)
        } else {
            format_prompt_with_context(user_text, screen_tokens, source_app)
        };

        // Tokenize prompt — BOS is prepended automatically where the template
        // calls for it.
        let prompt_tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| AppError::Llm(format!("Tokenize failed: {e}")))?;

        let n_ctx = ctx.n_ctx() as usize;
        if prompt_tokens.len() >= n_ctx {
            return Err(AppError::Llm(format!(
                "Prompt tokens ({}) exceed context size ({}).",
                prompt_tokens.len(),
                n_ctx
            )));
        }

        // Prime the KV cache with the full prompt.  Only the last token needs
        // logits since that's what we'll sample from.
        let mut batch = LlamaBatch::new(n_ctx, 1);
        let last_idx = prompt_tokens.len() - 1;
        for (i, tok) in prompt_tokens.iter().enumerate() {
            batch
                .add(*tok, i as i32, &[0], i == last_idx)
                .map_err(|e| AppError::Llm(format!("batch add: {e}")))?;
        }
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("prompt decode: {e}")))?;

        // Sampler chain: grammar filters illegal tokens → greedy picks argmax.
        let grammar = LlamaSampler::grammar(&self.model, SLOT_EXTRACTION_V1, SLOT_EXTRACTION_ROOT)
            .map_err(|e| AppError::Llm(format!("grammar init: {e:?}")))?;
        let mut sampler = LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()]);

        let mut out = String::new();
        let mut n_past = prompt_tokens.len() as i32;
        let max_tokens = self.config.max_tokens as usize;
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        for _ in 0..max_tokens {
            // Sample from the freshest logits (last position we decoded).
            let token = sampler.sample(&ctx, -1);

            if self.model.is_eog_token(token) {
                break;
            }

            let piece = self
                .model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|e| AppError::Llm(format!("decode token: {e}")))?;
            out.push_str(&piece);

            // Feed the sampled token back in for the next step.
            batch.clear();
            batch
                .add(token, n_past, &[0], true)
                .map_err(|e| AppError::Llm(format!("batch add (gen): {e}")))?;
            n_past += 1;
            ctx.decode(&mut batch)
                .map_err(|e| AppError::Llm(format!("gen decode: {e}")))?;
        }

        Ok(out)
    }
}

impl LlmEngine for LlamaEngine {
    fn extract_slots(&self, user_text: &str) -> AppResult<SlotExtraction> {
        self.extract_slots_with_context(user_text, &[], None)
    }

    fn extract_slots_with_context(
        &self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<SlotExtraction> {
        let t0 = std::time::Instant::now();
        crate::llm::diaglog::log(&format!(
            "extract_slots: model={} input_chars={} screen_tokens={} app={:?} input_preview={:?}",
            self.model_name,
            user_text.chars().count(),
            screen_tokens.len(),
            source_app,
            &user_text.chars().take(120).collect::<String>()
        ));
        let raw = match self.generate_json(user_text, screen_tokens, source_app) {
            Ok(r) => r,
            Err(e) => {
                crate::llm::diaglog::log(&format!(
                    "generate_json FAILED after {}ms: {e}",
                    t0.elapsed().as_millis()
                ));
                return Err(e);
            }
        };
        let trimmed = raw.trim();
        crate::llm::diaglog::log(&format!(
            "generate_json ok in {}ms raw_len={} raw={:?}",
            t0.elapsed().as_millis(),
            trimmed.len(),
            &trimmed.chars().take(400).collect::<String>()
        ));
        serde_json::from_str::<SlotExtraction>(trimmed)
            // Use the raw-input-aware normalizer so we can drop slot items
            // the model fabricated without any support from the dictation.
            // This is the main defense against the "threshold-length
            // fabrication" problem: when the input is short, the model's
            // helpful-bias invents context/constraints to pad the JSON,
            // and this pass strips anything with zero content-word overlap.
            .map(|slots| slots.normalize_with_raw(user_text))
            .map_err(|e| AppError::Llm(format!("parse LLM JSON failed: {e}")))
    }

    fn extract_raw(&self, user_text: &str) -> AppResult<LlmInferenceResult> {
        let t0 = std::time::Instant::now();
        let raw = self.generate_json(user_text, &[], None)?;
        Ok(LlmInferenceResult {
            raw_json: raw,
            duration_ms: t0.elapsed().as_millis() as u64,
            model_name: self.model_name.clone(),
        })
    }
}

#[cfg(test)]
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LlamaEngine>();
};
