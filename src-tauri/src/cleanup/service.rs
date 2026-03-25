use std::time::Instant;

use crate::cleanup::prompts;
use crate::cleanup::types::*;
use crate::error::{AppError, AppResult};

/// Default inference endpoint for local model servers (Ollama default).
const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:11434";

/// Timeout for cleanup inference requests.
const INFERENCE_TIMEOUT_SECS: u64 = 30;

/// Maximum input length (characters) we'll send to cleanup. Longer texts get truncated.
const MAX_INPUT_LENGTH: usize = 8000;

/// Run cleanup inference against a local model server (Ollama-compatible API).
///
/// This is the main entry point for cleanup. It:
/// 1. Builds the appropriate prompt from mode + strength
/// 2. Sends it to the local model server
/// 3. Returns the cleaned text
///
/// The function is async and designed to be called from a Tokio task.
/// Cancellation is handled by dropping the future.
pub async fn run_cleanup(request: &CleanupRequest) -> AppResult<CleanupResult> {
    let start = Instant::now();

    // Truncate very long inputs to stay within model context
    let raw_text = if request.raw_text.len() > MAX_INPUT_LENGTH {
        &request.raw_text[..MAX_INPUT_LENGTH]
    } else {
        &request.raw_text
    };

    // Build prompts
    let system_prompt = prompts::build_system_prompt(request.mode, request.strength);
    let user_prompt = prompts::build_user_prompt(raw_text);

    // Determine endpoint — use model-specific endpoint or default
    let endpoint = crate::cleanup::registry::get_model(request.model_id.as_str())
        .and_then(|m| m.endpoint)
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string());

    // Try Ollama chat API first
    let cleaned_text = call_ollama_chat(&endpoint, &request.model_id, &system_prompt, &user_prompt).await?;

    let duration_ms = start.elapsed().as_millis() as u64;

    Ok(CleanupResult {
        raw_text: request.raw_text.clone(),
        cleaned_text,
        model_id: request.model_id.to_string(),
        mode: request.mode,
        strength: request.strength,
        duration_ms,
        status: CleanupStatus::Success,
    })
}

/// Call the Ollama-compatible /api/chat endpoint.
async fn call_ollama_chat(
    endpoint: &str,
    model_id: &CleanupModelId,
    system_prompt: &str,
    user_prompt: &str,
) -> AppResult<String> {
    let url = format!("{endpoint}/api/chat");

    // Map our model IDs to Ollama model names
    let ollama_model = model_id_to_ollama_name(model_id.as_str());

    let body = serde_json::json!({
        "model": ollama_model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_prompt }
        ],
        "stream": false,
        "options": {
            "temperature": 0.3,
            "top_p": 0.9,
            "num_predict": 2048
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(INFERENCE_TIMEOUT_SECS))
        .build()
        .map_err(|e| AppError::Internal(format!("Failed to create HTTP client: {e}")))?;

    let response = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                AppError::Internal("Cleanup model timed out. The model may still be loading.".to_string())
            } else if e.is_connect() {
                AppError::Internal(
                    "Cannot connect to local model server. Make sure Ollama is running (ollama serve).".to_string()
                )
            } else {
                AppError::Internal(format!("Cleanup request failed: {e}"))
            }
        })?;

    if !response.status().is_success() {
        let status = response.status();
        let body_text = response.text().await.unwrap_or_default();
        return Err(AppError::Internal(format!(
            "Cleanup model returned error {status}: {body_text}"
        )));
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse cleanup response: {e}")))?;

    // Extract the assistant's message content
    let content = json["message"]["content"]
        .as_str()
        .unwrap_or("")
        .trim()
        .to_string();

    if content.is_empty() {
        return Err(AppError::Internal("Cleanup model returned empty response".to_string()));
    }

    Ok(content)
}

/// Map internal model IDs to Ollama model names.
/// Users may have these models pulled under different tags,
/// so we use the most common/standard names.
fn model_id_to_ollama_name(id: &str) -> &str {
    match id {
        "qwen3_5_4b" => "qwen3:4b",
        "phi4_mini_instruct" => "phi4-mini",
        "smollm3_3b" => "smollm3:3b",
        "granite_3_3_2b_instruct" => "granite3.3:2b",
        other => other,
    }
}

/// Check if the local model server is reachable and a given model is available.
pub async fn check_model_availability(model_id: &str) -> AppResult<bool> {
    let endpoint = DEFAULT_ENDPOINT;
    let ollama_model = model_id_to_ollama_name(model_id);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| AppError::Internal(format!("HTTP client error: {e}")))?;

    // Check if Ollama is running by hitting /api/tags
    let url = format!("{endpoint}/api/tags");
    let response = match client.get(&url).send().await {
        Ok(r) => r,
        Err(_) => return Ok(false), // Server not reachable
    };

    if !response.status().is_success() {
        return Ok(false);
    }

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| AppError::Internal(format!("Failed to parse tags response: {e}")))?;

    // Check if the model is in the list
    if let Some(model_list) = json["models"].as_array() {
        let found = model_list.iter().any(|m| {
            m["name"]
                .as_str()
                .map(|name: &str| {
                    // Ollama names can include `:latest` suffix
                    let base = name.split(':').next().unwrap_or(name);
                    let target_base = ollama_model.split(':').next().unwrap_or(ollama_model);
                    base == target_base || name == ollama_model
                })
                .unwrap_or(false)
        });
        return Ok(found);
    }

    Ok(false)
}

/// Check if the Ollama server is reachable.
pub async fn is_server_running() -> bool {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build();

    let Ok(client) = client else { return false };

    client
        .get(format!("{DEFAULT_ENDPOINT}/api/tags"))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}
