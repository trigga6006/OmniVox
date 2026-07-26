//! Append-only diagnostic log for Structured Mode.
//!
//! Tauri GUI apps on Windows have no attached console, so `eprintln!` goes
//! nowhere the user can see. When explicitly enabled with
//! `OMNIVOX_STRUCTURED_MODE_LOG=1`, this writes extraction diagnostics in the
//! app data dir (`%AppData%\omnivox\structured-mode.log`).

use serde::Serialize;
use std::collections::VecDeque;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// One structured-mode extraction attempt.  Kept in an in-memory ring buffer
/// (last [`RECENT_CAP`]) so the Models → LLM panel can show recent activity
/// to end users — no env var, no file access.  The file log below remains a
/// developer feature behind `OMNIVOX_STRUCTURED_MODE_LOG`.
#[derive(Clone, Serialize)]
pub struct ExtractionRecord {
    /// RFC 3339 UTC.
    pub timestamp: String,
    pub duration_ms: u64,
    /// Chars actually sent to the LLM (post-truncation).
    pub input_chars: usize,
    /// Chars dropped by the input cap (0 = nothing truncated).
    pub truncated_chars: usize,
    /// Rendered markdown length; 0 when the extraction failed.
    pub output_chars: usize,
    /// "ok", or the degradation reason shown to the user.
    pub outcome: String,
}

const RECENT_CAP: usize = 20;
static RECENT: Mutex<VecDeque<ExtractionRecord>> = Mutex::new(VecDeque::new());

/// Push an extraction record into the ring buffer.
pub fn record(rec: ExtractionRecord) {
    if let Ok(mut q) = RECENT.lock() {
        if q.len() == RECENT_CAP {
            q.pop_front();
        }
        q.push_back(rec);
    }
}

/// Recent extraction records, newest first.
pub fn recent() -> Vec<ExtractionRecord> {
    RECENT
        .lock()
        .map(|q| q.iter().rev().cloned().collect())
        .unwrap_or_default()
}

fn enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("OMNIVOX_STRUCTURED_MODE_LOG")
            .map(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
    })
}

fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| dirs::data_dir().map(|d| d.join("omnivox").join("structured-mode.log")))
        .as_ref()
}

pub fn log(msg: &str) {
    if !enabled() {
        return;
    }

    let Some(path) = log_path() else { return };
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("{ts} {msg}\n");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}
