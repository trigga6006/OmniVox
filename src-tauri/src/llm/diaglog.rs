//! Append-only diagnostic log for Structured Mode.
//!
//! Tauri GUI apps on Windows have no attached console, so `eprintln!` goes
//! nowhere the user can see. When explicitly enabled with
//! `OMNIVOX_STRUCTURED_MODE_LOG=1`, this writes extraction diagnostics in the
//! app data dir (`%AppData%\omnivox\structured-mode.log`).

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

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
    PATH.get_or_init(|| {
        dirs::data_dir().map(|d| d.join("omnivox").join("structured-mode.log"))
    })
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
