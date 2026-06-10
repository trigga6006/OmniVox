//! Always-on, append-only diagnostic log for model loading.
//!
//! Windowed release builds have no console, so `eprintln!` evaporates —
//! which is how GPU→CPU fallbacks stayed invisible for months.  One line
//! per model-load event lands in `%AppData%\omnivox\model-load.log` so
//! "the app feels slow today" can be checked against what actually loaded.
//!
//! Unlike `llm::diaglog` this is NOT env-gated: it writes a handful of
//! lines per app session, so there's no volume concern.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| dirs::data_dir().map(|d| d.join("omnivox").join("model-load.log")))
        .as_ref()
}

pub fn log(msg: &str) {
    let Some(path) = log_path() else { return };
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("{ts} {msg}\n");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
    // Mirror to stderr for dev runs with a console attached.
    eprint!("{line}");
}
