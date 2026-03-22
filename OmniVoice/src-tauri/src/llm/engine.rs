use std::num::NonZeroU32;
use std::pin::pin;

use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;

use crate::error::{AppError, AppResult};
use crate::llm::types::LlmConfig;

const SYSTEM_PROMPT: &str = "\
You are a dictation cleanup assistant. Clean the following transcribed speech:
- Remove filler words (um, uh, like, you know, so, basically, actually)
- Fix grammar, spelling, and punctuation
- Handle self-corrections (keep the intended word, remove false starts)
- Preserve the speaker's intended meaning exactly
- Do not add information or change meaning
Output ONLY the cleaned text, nothing else.";

/// Local LLM engine for AI text cleanup, powered by llama.cpp.
///
/// Loads a quantized GGUF model and runs inference to clean up raw
/// transcription output (filler words, grammar, self-corrections).
pub struct LlmEngine {
    _backend: LlamaBackend,
    model: LlamaModel,
    config: LlmConfig,
}

// LlamaModel is Send+Sync. LlamaBackend wraps global llama.cpp state.
// The engine is behind Mutex<Option<LlmEngine>> in AppState, ensuring
// exclusive single-threaded access to all operations.
unsafe impl Send for LlmEngine {}
unsafe impl Sync for LlmEngine {}

impl LlmEngine {
    /// Load a GGUF model from disk. This is expensive (~3-5s) and should
    /// only be called once when the user enables AI cleanup.
    pub fn load(config: LlmConfig) -> AppResult<Self> {
        let backend = LlamaBackend::init()
            .map_err(|e| AppError::Llm(format!("Failed to init llama backend: {e}")))?;

        let model_params = pin!(LlamaModelParams::default());

        let model = LlamaModel::load_from_file(&backend, &config.model_path, &model_params)
            .map_err(|e| AppError::Llm(format!("Failed to load LLM model: {e}")))?;

        Ok(Self {
            _backend: backend,
            model,
            config,
        })
    }

    /// Run the cleanup LLM on raw transcription text.
    ///
    /// Formats a ChatML prompt with the system instruction and user text,
    /// runs inference, and returns the cleaned text. Falls back gracefully
    /// if inference produces no output.
    pub fn cleanup_text(&self, raw_text: &str) -> AppResult<String> {
        if raw_text.trim().is_empty() {
            return Ok(raw_text.to_string());
        }

        // Build ChatML prompt (Qwen3 format)
        let prompt = format!(
            "<|im_start|>system\n{SYSTEM_PROMPT}<|im_end|>\n\
             <|im_start|>user\n{raw_text}<|im_end|>\n\
             <|im_start|>assistant\n"
        );

        // Tokenize
        let tokens = self
            .model
            .str_to_token(&prompt, AddBos::Always)
            .map_err(|e| AppError::Llm(format!("Tokenization failed: {e}")))?;

        // Create a fresh context for this inference call.
        // LlamaContext is !Send, but it lives entirely within this function.
        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(Some(
                NonZeroU32::new(self.config.context_size).unwrap_or(NonZeroU32::new(512).unwrap()),
            ));

        let mut ctx = self
            .model
            .new_context(&self._backend, ctx_params)
            .map_err(|e| AppError::Llm(format!("Failed to create context: {e}")))?;

        // Set up sampler: low temperature for deterministic cleanup
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::min_p(0.05, 1),
            LlamaSampler::temp(self.config.temperature),
            LlamaSampler::dist(42),
        ]);

        // Feed prompt tokens into a batch
        let mut batch = LlamaBatch::new(self.config.context_size as usize, 1);
        let last_index = (tokens.len() - 1) as i32;

        for (i, token) in (0_i32..).zip(tokens.iter().copied()) {
            let is_last = i == last_index;
            batch
                .add(token, i, &[0], is_last)
                .map_err(|e| AppError::Llm(format!("Batch add failed: {e}")))?;
        }

        // Decode the prompt
        ctx.decode(&mut batch)
            .map_err(|e| AppError::Llm(format!("Prompt decode failed: {e}")))?;

        // Generate tokens
        let mut output = String::new();
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let max_tokens = self.config.max_tokens as i32;
        let mut n_cur = batch.n_tokens();

        for _ in 0..max_tokens {
            let token = sampler.sample(&ctx, batch.n_tokens() - 1);
            sampler.accept(token);

            // Stop at end of generation
            if self.model.is_eog_token(token) {
                break;
            }

            // Convert token to text
            if let Ok(piece) = self.model.token_to_piece(token, &mut decoder, false, None) {
                // Stop if we hit the end-of-turn marker
                if piece.contains("<|im_end|>") || piece.contains("<|im_start|>") {
                    break;
                }
                output.push_str(&piece);
            }

            // Prepare next decode step
            batch.clear();
            batch
                .add(token, n_cur, &[0], true)
                .map_err(|e| AppError::Llm(format!("Batch add failed: {e}")))?;

            ctx.decode(&mut batch)
                .map_err(|e| AppError::Llm(format!("Decode failed: {e}")))?;

            n_cur += 1;
        }

        let cleaned = output.trim().to_string();

        // If the model produced empty output, fall back to the original
        if cleaned.is_empty() {
            Ok(raw_text.to_string())
        } else {
            Ok(cleaned)
        }
    }
}
