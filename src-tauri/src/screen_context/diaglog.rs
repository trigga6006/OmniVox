//! Lightweight diagnostic log for the screen-context capture pipeline.
//!
//! Mirrors `crate::llm::diaglog` — appends to a per-process log file under
//! the user's data dir so dogfooding can verify capture timing, token
//! counts, and which apps return useful UIA text.  Disabled silently if
//! the file can't be opened (no panics, no error spam).
//!
//! Toggle: set the env var `OMNIVOX_SCREEN_CONTEXT_LOG=1` before launching
//! to enable.  Off by default to keep release builds quiet.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::OnceLock;

fn enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("OMNIVOX_SCREEN_CONTEXT_LOG")
            .map(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
    })
}

fn log_path() -> Option<std::path::PathBuf> {
    let dir = dirs::data_dir()?.join("omnivox");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("screen_context.log"))
}

pub fn log(msg: &str) {
    if !enabled() {
        return;
    }
    let Some(path) = log_path() else { return };
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(f, "[{now}] {msg}");
    }
}
