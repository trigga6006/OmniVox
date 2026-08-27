//! Always-on, bounded diagnostic log for model loading.
//!
//! Windowed release builds have no console, so `eprintln!` evaporates —
//! which is how GPU→CPU fallbacks stayed invisible for months.  One line
//! per model-load event lands in `%AppData%\omnivox\model-load.log` so
//! "the app feels slow today" can be checked against what actually loaded.
//!
//! Unlike `llm::diaglog` this is NOT env-gated — it writes a handful of lines
//! per app session — but it uses the same size-capped, rotating writer so the
//! file cannot grow without bound across a long-lived install.

use std::path::PathBuf;
use std::sync::OnceLock;

/// Same bound as `llm::diaglog`: ~1 MiB per file plus its rotated backups.
const MAX_LOG_BYTES: u64 = 1_048_576;

fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| dirs::data_dir().map(|d| d.join("omnivox").join("model-load.log")))
        .as_ref()
}

pub fn log(msg: &str) {
    let Some(path) = log_path() else { return };
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("{ts} {msg}\n");
    let _ = crate::llm::diaglog::write_bounded_line(path, line.as_bytes(), MAX_LOG_BYTES);
    // Mirror to stderr for dev runs with a console attached.
    eprint!("{line}");
}
