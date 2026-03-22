use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::llm::types::LlmConfig;

/// Request sent to the LLM sidecar process (JSON-line over stdin).
#[derive(Serialize)]
#[serde(tag = "cmd")]
enum Request<'a> {
    #[serde(rename = "load")]
    Load { model_path: &'a str },
    #[serde(rename = "cleanup")]
    Cleanup {
        text: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        system_prompt: Option<&'a str>,
    },
    #[serde(rename = "unload")]
    Unload,
    #[serde(rename = "ping")]
    #[allow(dead_code)]
    Ping,
}

/// Response from the LLM sidecar process (JSON-line over stdout).
#[derive(Deserialize)]
struct Response {
    ok: bool,
    text: Option<String>,
    error: Option<String>,
}

/// LLM engine backed by a child process (`omnivox-llm`).
///
/// The sidecar process links llama-cpp-2 and GGML in its own address space,
/// completely avoiding the symbol collision with whisper-rs in the main process.
pub struct LlmEngine {
    child: Child,
    stdin: Option<ChildStdin>,
    reader: BufReader<ChildStdout>,
    #[allow(dead_code)]
    config: LlmConfig,
}

// The engine is behind Mutex<Option<LlmEngine>> in AppState, so only one
// thread accesses it at a time.
unsafe impl Send for LlmEngine {}
unsafe impl Sync for LlmEngine {}

impl LlmEngine {
    /// Spawn the sidecar, send a `load` command, and wait for it to confirm
    /// that the model is ready.
    pub fn load(config: LlmConfig) -> AppResult<Self> {
        let sidecar_path = find_sidecar()?;

        let mut child = Command::new(&sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| AppError::Llm(format!(
                "Failed to spawn LLM sidecar at {}: {e}", sidecar_path.display()
            )))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| AppError::Llm("failed to capture sidecar stdin".into()))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| AppError::Llm("failed to capture sidecar stdout".into()))?;

        let mut engine = Self {
            child,
            stdin: Some(stdin),
            reader: BufReader::new(stdout),
            config: config.clone(),
        };

        // Ask the sidecar to load the model.
        let resp = engine.send(&Request::Load {
            model_path: config.model_path.to_string_lossy().as_ref(),
        })?;

        if !resp.ok {
            let msg = resp.error.unwrap_or_else(|| "unknown error".into());
            return Err(AppError::Llm(format!("Sidecar model load failed: {msg}")));
        }

        Ok(engine)
    }

    /// Run the cleanup LLM on raw transcription text.
    /// If `system_prompt` is provided, it overrides the sidecar's default prompt.
    pub fn cleanup_text(&mut self, raw_text: &str, system_prompt: Option<&str>) -> AppResult<String> {
        if raw_text.trim().is_empty() {
            return Ok(raw_text.to_string());
        }

        let resp = self.send(&Request::Cleanup { text: raw_text, system_prompt })?;

        if resp.ok {
            Ok(resp.text.unwrap_or_else(|| raw_text.to_string()))
        } else {
            let msg = resp.error.unwrap_or_else(|| "inference failed".into());
            Err(AppError::Llm(msg))
        }
    }

    /// Send a JSON-line request to the sidecar and read one JSON-line response.
    fn send(&mut self, req: &Request<'_>) -> AppResult<Response> {
        let stdin = self.stdin.as_mut()
            .ok_or_else(|| AppError::Llm("sidecar stdin closed".into()))?;

        let json = serde_json::to_string(req)
            .map_err(|e| AppError::Llm(format!("serialize: {e}")))?;

        writeln!(stdin, "{json}")
            .map_err(|e| AppError::Llm(format!("write to sidecar: {e}")))?;
        stdin.flush()
            .map_err(|e| AppError::Llm(format!("flush sidecar: {e}")))?;

        let mut line = String::new();
        self.reader.read_line(&mut line)
            .map_err(|e| AppError::Llm(format!("read from sidecar: {e}")))?;

        if line.trim().is_empty() {
            return Err(AppError::Llm("sidecar returned empty response (crashed?)".into()));
        }

        serde_json::from_str(&line)
            .map_err(|e| AppError::Llm(format!("parse sidecar response: {e}")))
    }
}

impl Drop for LlmEngine {
    fn drop(&mut self) {
        // Try to send an unload command; if the process is already dead
        // this is harmless — the write will fail silently.
        let _ = self.send(&Request::Unload);
        // Close stdin to signal EOF → sidecar exits its read loop.
        // Must happen before wait() or we deadlock.
        self.stdin.take();
        let _ = self.child.wait();
    }
}

/// Locate the sidecar binary.  Searches:
///   1. Next to the main executable  (works in dev builds)
///   2. In the `resources/` subdirectory next to the exe (NSIS bundled app)
fn find_sidecar() -> AppResult<std::path::PathBuf> {
    let exe_name = if cfg!(windows) { "omnivox-llm.exe" } else { "omnivox-llm" };

    if let Ok(self_exe) = std::env::current_exe() {
        if let Some(dir) = self_exe.parent() {
            // 1. Next to our own executable (dev builds)
            let candidate = dir.join(exe_name);
            if candidate.exists() {
                return Ok(candidate);
            }
            // 2. In resources/ subdirectory (Tauri NSIS bundle)
            let candidate = dir.join("resources").join(exe_name);
            if candidate.exists() {
                return Ok(candidate);
            }
        }
    }

    Err(AppError::Llm(format!(
        "LLM sidecar binary '{exe_name}' not found. \
         Build it with: cargo build --manifest-path llm-sidecar/Cargo.toml"
    )))
}
