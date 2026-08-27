//! Privacy-bounded diagnostics for the opt-in screen-context pipeline.
//!
//! Logging itself requires `OMNIVOX_SCREEN_CONTEXT_LOG=1`. Events are typed so
//! visible screen text, extracted tokens, window titles, executable paths, and
//! process names cannot enter the log. Writes are serialized and rotated.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_LOG_BYTES: u64 = 512 * 1024;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Event {
    NoForegroundWindow,
    SkippedExcludedApplication,
    Capture {
        source_known: bool,
        chars: usize,
        tokens: usize,
        duration_ms: u128,
    },
    ComInitializationFailed,
    WatchdogAborted,
}

impl Event {
    fn render(self) -> String {
        match self {
            Self::NoForegroundWindow => "capture outcome=no_foreground".into(),
            Self::SkippedExcludedApplication => "capture outcome=excluded_application".into(),
            Self::Capture {
                source_known,
                chars,
                tokens,
                duration_ms,
            } => format!(
                "capture outcome=ok source_known={source_known} chars={chars} tokens={tokens} duration_ms={duration_ms}"
            ),
            Self::ComInitializationFailed => "uia outcome=com_initialization_failed".into(),
            Self::WatchdogAborted => "uia outcome=watchdog_abort".into(),
        }
    }
}

fn enabled() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("OMNIVOX_SCREEN_CONTEXT_LOG")
            .map(|v| !v.is_empty() && v != "0" && !v.eq_ignore_ascii_case("false"))
            .unwrap_or(false)
    })
}

fn log_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| dirs::data_dir().map(|d| d.join("omnivox").join("screen_context.log")))
        .as_ref()
}

pub(crate) fn log(event: Event) {
    if !enabled() {
        return;
    }
    let Some(path) = log_path() else { return };
    let _ = write_event(path, event, MAX_LOG_BYTES);
}

/// Render and append one event.
///
/// Bounding, rotation and write serialization are the shared writer's job —
/// this file used to carry a third divergent copy of them, which is exactly how
/// two of the three drifted apart. The 512 KiB cap and the single-line clamp
/// both survive: the clamp now lives in the shared helper.
fn write_event(path: &Path, event: Event, max_bytes: u64) -> std::io::Result<()> {
    let timestamp = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("{timestamp} {}\n", event.render());
    crate::llm::diaglog::write_bounded_line(path, line.as_bytes(), max_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_log(label: &str) -> (PathBuf, PathBuf) {
        let dir = std::env::temp_dir().join(format!(
            "omnivox-screen-diag-{label}-{}",
            uuid::Uuid::new_v4()
        ));
        (dir.join("screen_context.log"), dir)
    }

    #[test]
    fn events_expose_only_minimized_capture_metadata() {
        let rendered = Event::Capture {
            source_known: true,
            chars: 8192,
            tokens: 30,
            duration_ms: 249,
        }
        .render();
        assert_eq!(
            rendered,
            "capture outcome=ok source_known=true chars=8192 tokens=30 duration_ms=249"
        );
        assert!(!rendered.contains(".exe"));
        assert!(!rendered.contains('\\'));
    }

    #[test]
    fn bounded_writer_rotates_old_logs() {
        let (path, dir) = temp_log("rotate");
        write_event(&path, Event::WatchdogAborted, 80).unwrap();
        write_event(&path, Event::ComInitializationFailed, 80).unwrap();
        assert!(path.is_file());
        assert!(path.with_file_name("screen_context.log.1").is_file());
        assert!(std::fs::metadata(&path).unwrap().len() <= 80);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn concurrent_writes_are_complete_and_not_interleaved() {
        let (path, dir) = temp_log("concurrent");
        let path = std::sync::Arc::new(path);
        let workers: Vec<_> = (0..16)
            .map(|index| {
                let path = std::sync::Arc::clone(&path);
                std::thread::spawn(move || {
                    write_event(
                        &path,
                        Event::Capture {
                            source_known: index % 2 == 0,
                            chars: index,
                            tokens: 1,
                            duration_ms: 2,
                        },
                        64 * 1024,
                    )
                    .unwrap();
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        let contents = std::fs::read_to_string(path.as_ref()).unwrap();
        assert_eq!(contents.lines().count(), 16);
        assert!(contents
            .lines()
            .all(|line| line.contains("capture outcome=ok source_known=")));
        std::fs::remove_dir_all(dir).ok();
    }
}
