use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;

use crate::error::{AppError, AppResult};
use crate::llm::grammar::{COMMAND_INTENT_ROOT, COMMAND_INTENT_V1};
use crate::llm::profiles::{self, Profile};
use crate::llm::prompt::format_profile_prompt;
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

    /// Command-Mode fallback: map a free-form spoken command to an ordered
    /// sequence of `CommandIntent`s (one or more — supports multi-step chains),
    /// or an empty Vec if it isn't a recognizable command.  Default impl (used
    /// by test mocks) returns no actions.
    fn classify_command(
        &self,
        _utterance: &str,
    ) -> AppResult<Vec<crate::actions::CommandIntent>> {
        Ok(Vec::new())
    }

    /// Create a stateful extraction session for a dedicated worker thread,
    /// bound to one Structured Mode profile (its system prompt + grammar).
    ///
    /// The default implementation is stateless and ignores the profile —
    /// every call delegates back to `extract_slots_with_context` — so test
    /// mocks compile unchanged.  The production `LlamaEngine` overrides this
    /// with a KV-cache-backed session that prefills the (large, constant)
    /// profile system prompt once instead of on every extraction.
    fn new_session_for(
        &self,
        _profile: &'static Profile,
    ) -> AppResult<Box<dyn LlmSession + '_>>
    where
        Self: Sized,
    {
        Ok(Box::new(StatelessSession(self)))
    }
}

/// A stateful extraction handle owned by a single worker thread.
///
/// Unlike `LlmEngine` this takes `&mut self` and is deliberately NOT `Send` —
/// the production implementation holds a `LlamaContext`, which must stay on
/// the thread that created it.
pub trait LlmSession {
    /// Run one grammar-constrained generation for this session's profile and
    /// return the raw (unparsed) JSON string.  Parsing / grounding / render
    /// live in the profile's `postprocess` so slot shapes can differ.
    fn generate_raw(
        &mut self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<String>;
}

/// Fallback session that re-runs the full prompt on every call.  Only used by
/// mock engines (the production engine overrides `new_session_for`), so it is
/// agent-prompt-shaped: it serializes the mock's `SlotExtraction` back to JSON.
pub struct StatelessSession<'e, E: LlmEngine>(pub &'e E);

impl<E: LlmEngine> LlmSession for StatelessSession<'_, E> {
    fn generate_raw(
        &mut self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<String> {
        let slots = self
            .0
            .extract_slots_with_context(user_text, screen_tokens, source_app)?;
        serde_json::to_string(&slots).map_err(|e| AppError::Llm(format!("serialize: {e}")))
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

    /// Run the generation loop for a single extraction using a throwaway
    /// context on the DEFAULT (agent-prompt) profile.  Used by `extract_raw`
    /// (Settings "Test" button) and the stateless fallback path — the
    /// runner's hot path goes through `new_session_for` instead, which keeps
    /// the active profile's system-prompt KV cached.
    fn generate_json(
        &self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<String> {
        let mut session = LlamaSession::new(self, profiles::get(profiles::DEFAULT_PROFILE_ID))?;
        session.generate_json(user_text, screen_tokens, source_app)
    }

    /// Parse + normalize the model's raw JSON against the dictated text.
    fn parse_slots(raw: &str, user_text: &str) -> AppResult<SlotExtraction> {
        serde_json::from_str::<SlotExtraction>(raw.trim())
            // Use the raw-input-aware normalizer so we can drop slot items
            // the model fabricated without any support from the dictation.
            // This is the main defense against the "threshold-length
            // fabrication" problem: when the input is short, the model's
            // helpful-bias invents context/constraints to pad the JSON,
            // and this pass strips anything with zero content-word overlap.
            .map(|slots| slots.normalize_with_raw(user_text))
            .map_err(|e| AppError::Llm(format!("parse LLM JSON failed: {e}")))
    }

    /// One-off Command-Mode classification on a throwaway context.  Deliberately
    /// does NOT reuse the warmed slot-extraction session — different prompt and
    /// grammar — so Command Mode never thrashes Structured Mode's KV cache.
    fn classify_command_raw(&self, utterance: &str) -> AppResult<String> {
        // Profile is irrelevant here — the command prompt/grammar are built
        // inside `generate_command_json` — but the session needs one.
        let mut session = LlamaSession::new(self, profiles::get(profiles::DEFAULT_PROFILE_ID))?;
        session.generate_command_json(utterance)
    }
}

/// KV-cache-backed extraction session, bound to one Structured Mode profile.
///
/// Owns one persistent `LlamaContext`.  Across calls it keeps the longest
/// common token prefix of the previous prompt in the KV cache — in practice
/// the entire system turn (byte-identical every time for a given profile) —
/// so each extraction only prefills the user's own words plus the few dozen
/// template tokens around them.  On CPU this cuts per-extraction prompt
/// processing from seconds to tens of milliseconds.
///
/// Profile switching invalidates the warm prefix by construction: the runner
/// drops this session and builds a fresh one for the new profile.
///
/// Self-healing: any error mid-extraction clears the cache entirely, so a
/// failed decode can never poison the next call's prefix match.
pub struct LlamaSession<'m> {
    engine: &'m LlamaEngine,
    profile: &'static Profile,
    ctx: LlamaContext<'m>,
    // 'static: the batch owns its buffers (allocated via `LlamaBatch::new`);
    // the lifetime parameter only matters for the borrowing `get_one` path.
    batch: LlamaBatch<'static>,
    /// Tokens currently materialized in the KV cache (previous prompt plus
    /// its generated tokens).
    cached_tokens: Vec<LlamaToken>,
}

impl<'m> LlamaSession<'m> {
    /// Allocate a fresh context for this session.  KV memory for the full
    /// `n_ctx` is reserved here and held until the session is dropped — the
    /// runner drops idle sessions to give it back.
    pub fn new(engine: &'m LlamaEngine, profile: &'static Profile) -> AppResult<Self> {
        let backend = backend()?;
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(engine.config.n_ctx))
            .with_n_batch(engine.config.n_ctx.max(512))
            .with_n_threads(engine.config.n_threads);

        let ctx = engine
            .model
            .new_context(backend, ctx_params)
            .map_err(|e| AppError::Llm(format!("Failed to create LLM context: {e}")))?;
        let n_ctx = ctx.n_ctx() as usize;

        Ok(Self {
            engine,
            profile,
            ctx,
            batch: LlamaBatch::new(n_ctx, 1),
            cached_tokens: Vec::new(),
        })
    }

    /// Prefill the constant prompt prefix (system turn + user-turn opener) so
    /// the first real extraction only pays for the user's words.  Called by
    /// the runner right after spawn, while no dictation is waiting.
    pub fn warm(&mut self) -> AppResult<()> {
        let prefix = format!(
            "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n",
            self.profile.system_prompt
        );
        let t0 = std::time::Instant::now();
        let n = self.ensure_prompt(&prefix)?;
        crate::llm::diaglog::log(&format!(
            "session: warmed {} prefix tokens ({}) in {}ms",
            n,
            self.profile.id,
            t0.elapsed().as_millis()
        ));
        Ok(())
    }

    /// Tokenize `prompt` and verify it fits the context (leaving room for at
    /// least one generated token) WITHOUT touching the KV cache.
    ///
    /// Over-length and tokenizer failures are surfaced here — before any cache
    /// mutation — so callers can degrade while leaving the warm prefix intact.
    /// A dense or CJK dictation that tokenizes past `n_ctx` must NOT force a
    /// full re-warm of the next, normal-length dictation.
    fn tokenize_prompt(&self, prompt: &str) -> AppResult<Vec<LlamaToken>> {
        let full = self
            .engine
            .model
            .str_to_token(prompt, AddBos::Always)
            .map_err(|e| AppError::Llm(format!("Tokenize failed: {e}")))?;

        let n_ctx = self.ctx.n_ctx() as usize;
        // Require headroom for at least one generated token.
        if full.len() + 1 >= n_ctx {
            return Err(AppError::Llm(format!(
                "Prompt tokens ({}) exceed context size ({}).",
                full.len(),
                n_ctx
            )));
        }
        Ok(full)
    }

    /// Bring the KV cache to exactly `full` (a prompt's tokenization from
    /// [`tokenize_prompt`]), reusing the longest common token prefix from the
    /// previous call.  Returns the prompt token count (== the position where
    /// generation starts).  This mutates the cache.
    fn decode_prompt(&mut self, full: Vec<LlamaToken>) -> AppResult<usize> {
        // Longest common prefix with what's already in the cache.  When the
        // prompt is identical (warm + warm, or a repeated dictation), keep
        // one token back so the decode below refreshes the sampler's logits.
        let mut common = self
            .cached_tokens
            .iter()
            .zip(full.iter())
            .take_while(|(a, b)| a == b)
            .count();
        if common == full.len() {
            common -= 1;
        }

        // Drop everything past the shared prefix.  `Ok(false)` or `Err` mean
        // the partial removal failed — fall back to a full re-decode.
        let cleared = self
            .ctx
            .clear_kv_cache_seq(Some(0), Some(common as u32), None)
            .unwrap_or(false);
        if !cleared {
            self.ctx.clear_kv_cache();
            common = 0;
        }
        self.cached_tokens.truncate(common);

        // Decode the divergent tail.  Guaranteed to fit in one batch: the
        // batch holds n_ctx tokens and full.len() < n_ctx.
        let last_idx = full.len() - 1;
        self.batch.clear();
        for (i, tok) in full.iter().enumerate().skip(common) {
            self.batch
                .add(*tok, i as i32, &[0], i == last_idx)
                .map_err(|e| AppError::Llm(format!("batch add: {e}")))?;
        }
        self.ctx
            .decode(&mut self.batch)
            .map_err(|e| AppError::Llm(format!("prompt decode: {e}")))?;

        crate::llm::diaglog::log(&format!(
            "session: prompt={} reused={} decoded={}",
            full.len(),
            common,
            full.len() - common
        ));

        self.cached_tokens = full;
        Ok(self.cached_tokens.len())
    }

    /// Bring the KV cache to exactly the tokenization of `prompt`.  Convenience
    /// wrapper used by [`warm`](Self::warm); the extraction/command paths call
    /// the two halves directly so an over-length prompt is rejected (via
    /// [`tokenize_prompt`]) before the cache-mutating [`decode_prompt`] runs.
    fn ensure_prompt(&mut self, prompt: &str) -> AppResult<usize> {
        let full = self.tokenize_prompt(prompt)?;
        self.decode_prompt(full)
    }

    /// Grammar-constrained greedy generation from position `n_prompt`.
    ///
    /// Invariants (unchanged from the original single-shot path):
    /// - The sampler chain ends in `greedy` (matches hardcoded temp=0.0).
    /// - The grammar sampler is first in the chain, so token selection is
    ///   restricted to the GBNF alphabet before greedy picks the max-logit.
    /// - EOG stops generation; `max_tokens` bounds runaway loops.
    fn generate(&mut self, n_prompt: usize, grammar_str: &str, grammar_root: &str) -> AppResult<String> {
        // Fresh sampler per call — the grammar sampler is stateful (tracks
        // the GBNF parse position) and must restart for every generation.
        let grammar = LlamaSampler::grammar(&self.engine.model, grammar_str, grammar_root)
            .map_err(|e| AppError::Llm(format!("grammar init: {e:?}")))?;
        let mut sampler = LlamaSampler::chain_simple([grammar, LlamaSampler::greedy()]);

        let n_ctx = self.ctx.n_ctx() as usize;
        let mut out = String::new();
        let mut n_past = n_prompt;
        let max_tokens = self.engine.config.max_tokens as usize;
        let mut decoder = encoding_rs::UTF_8.new_decoder();

        for _ in 0..max_tokens {
            // Sample from the freshest logits (last position we decoded).
            let token = sampler.sample(&self.ctx, -1);

            if self.engine.model.is_eog_token(token) {
                break;
            }

            let piece = self
                .engine
                .model
                .token_to_piece(token, &mut decoder, false, None)
                .map_err(|e| AppError::Llm(format!("decode token: {e}")))?;
            out.push_str(&piece);

            if n_past >= n_ctx {
                return Err(AppError::Llm(format!(
                    "KV cache full mid-generation ({n_past} tokens)"
                )));
            }

            // Feed the sampled token back in for the next step.
            self.batch.clear();
            self.batch
                .add(token, n_past as i32, &[0], true)
                .map_err(|e| AppError::Llm(format!("batch add (gen): {e}")))?;
            self.ctx
                .decode(&mut self.batch)
                .map_err(|e| AppError::Llm(format!("gen decode: {e}")))?;
            self.cached_tokens.push(token);
            n_past += 1;
        }

        Ok(out)
    }

    /// Full prompt-build → prefill → generate pass under this session's
    /// profile (its system prompt + grammar).  On any error the KV cache is
    /// reset so the failure can't corrupt the next call.
    fn generate_json(
        &mut self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<String> {
        // Screen-context tokens only make sense for profiles whose system
        // prompt documents the SCREEN CONTEXT block (agent-prompt); for the
        // others they would just invite copying on-screen text into output.
        let tokens: &[String] = if self.profile.uses_screen_context {
            screen_tokens
        } else {
            &[]
        };
        let prompt =
            format_profile_prompt(self.profile.system_prompt, user_text, tokens, source_app);

        // Tokenize + length-check first: this does NOT touch the KV cache, so
        // an over-length dictation degrades gracefully while leaving the warm
        // system-prompt prefix intact.  Only the cache-mutating decode/generate
        // steps below trigger a cache reset on failure.
        let full = self.tokenize_prompt(&prompt)?;
        let result = self.decode_prompt(full).and_then(|n_prompt| {
            self.generate(n_prompt, self.profile.grammar, self.profile.grammar_root)
        });

        if result.is_err() {
            self.ctx.clear_kv_cache();
            self.cached_tokens.clear();
        }
        result
    }

    /// Command-Mode fallback: build the command prompt, prefill, and generate
    /// under the command grammar.  Used on a throwaway session so it never
    /// disturbs the warmed slot-extraction KV cache.
    fn generate_command_json(&mut self, utterance: &str) -> AppResult<String> {
        let prompt = crate::llm::prompt::format_command_prompt(utterance);
        // See `generate_json`: reject an over-length prompt before mutating the
        // cache so a too-long command utterance can't thrash the session.
        let full = self.tokenize_prompt(&prompt)?;
        let result = self
            .decode_prompt(full)
            .and_then(|n_prompt| self.generate(n_prompt, COMMAND_INTENT_V1, COMMAND_INTENT_ROOT));
        if result.is_err() {
            self.ctx.clear_kv_cache();
            self.cached_tokens.clear();
        }
        result
    }
}

impl LlmSession for LlamaSession<'_> {
    fn generate_raw(
        &mut self,
        user_text: &str,
        screen_tokens: &[String],
        source_app: Option<&str>,
    ) -> AppResult<String> {
        let t0 = std::time::Instant::now();
        crate::llm::diaglog::log(&format!(
            "session extract: model={} profile={} input_chars={} screen_tokens={} app={:?}",
            self.engine.model_name,
            self.profile.id,
            user_text.chars().count(),
            screen_tokens.len(),
            source_app,
        ));
        let raw = match self.generate_json(user_text, screen_tokens, source_app) {
            Ok(r) => r,
            Err(e) => {
                crate::llm::diaglog::log(&format!(
                    "session generate_json FAILED after {}ms: {e}",
                    t0.elapsed().as_millis()
                ));
                return Err(e);
            }
        };
        crate::llm::diaglog::log(&format!(
            "session generate_json ok in {}ms raw_len={}",
            t0.elapsed().as_millis(),
            raw.trim().len()
        ));
        Ok(raw)
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
        Self::parse_slots(trimmed, user_text)
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

    fn classify_command(
        &self,
        utterance: &str,
    ) -> AppResult<Vec<crate::actions::CommandIntent>> {
        let raw = self.classify_command_raw(utterance)?;
        let intents = crate::actions::CommandIntent::from_llm_list(raw.trim());
        // Surfaces all-or-nothing rejections: a non-empty raw array that maps to
        // 0 intents means a `none`/unsupported/empty-target step rejected the
        // whole utterance.
        crate::llm::diaglog::log(&format!(
            "classify_command: raw={:?} -> {} intent(s)",
            raw.trim(),
            intents.len()
        ));
        Ok(intents)
    }

    /// KV-cache-backed session: prefills the profile's system prompt once at
    /// creation so per-extraction prompt processing only covers the user's
    /// words.
    fn new_session_for(&self, profile: &'static Profile) -> AppResult<Box<dyn LlmSession + '_>> {
        let mut session = LlamaSession::new(self, profile)?;
        // Warm failure is non-fatal — the session still works, the first
        // extraction just pays the full prefill like the old path did.
        if let Err(e) = session.warm() {
            crate::llm::diaglog::log(&format!("session warm failed (continuing cold): {e}"));
        }
        Ok(Box::new(session))
    }
}

#[cfg(test)]
const _: fn() = || {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<LlamaEngine>();
};
