//! Privacy-bounded diagnostic logging for Structured and Command modes.
//!
//! File logging is disabled unless `OMNIVOX_STRUCTURED_MODE_LOG=1`. Even when
//! enabled, transcript/model-output fields are redacted before a bounded,
//! rotated log is written under the application data directory.

use serde::Serialize;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Serialize)]
pub struct ExtractionRecord {
    pub timestamp: String,
    pub duration_ms: u64,
    pub input_chars: usize,
    pub truncated_chars: usize,
    pub output_chars: usize,
    /// A short status category, never transcript or model output.
    pub outcome: String,
}

const RECENT_CAP: usize = 20;
const MAX_LOG_BYTES: u64 = 1_048_576;
const LOG_BACKUPS: usize = 2;
const MAX_LINE_BYTES: usize = 4_096;
static RECENT: Mutex<VecDeque<ExtractionRecord>> = Mutex::new(VecDeque::new());
static LOG_WRITE_LOCK: Mutex<()> = Mutex::new(());

pub fn record(mut rec: ExtractionRecord) {
    rec.outcome = outcome_category(&rec.outcome).into();
    if let Ok(mut q) = RECENT.lock() {
        if q.len() == RECENT_CAP {
            q.pop_front();
        }
        q.push_back(rec);
    }
}

fn outcome_category(outcome: &str) -> &'static str {
    let normalized = outcome.to_ascii_lowercase();
    if outcome.eq_ignore_ascii_case("ok") {
        "ok"
    } else if normalized.contains("timeout") || normalized.contains("timed out") {
        "timeout"
    } else if normalized.contains("no llm") {
        "unavailable"
    } else {
        "error"
    }
}

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
    let sanitized = sanitize_message(msg);
    let ts = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ");
    let line = format!("{ts} {sanitized}\n");
    let _ = write_bounded_line(path, line.as_bytes(), MAX_LOG_BYTES);
}

/// Append one line, rotating first when it would push the file past
/// `max_bytes`.  The single writer for every diagnostic log in the app:
/// [`crate::diag`]'s always-on model-load log and
/// [`crate::screen_context::diaglog`] both go through here.
///
/// The serialization lock lives INSIDE this function rather than at each call
/// site.  Two loader threads writing concurrently could otherwise interleave a
/// rotation with an append — one of them renaming the file the other had just
/// sized — losing lines or leaving an over-cap file behind.  Never call this
/// while already holding [`LOG_WRITE_LOCK`]: a `std::sync::Mutex` is not
/// reentrant.
pub(crate) fn write_bounded_line(path: &Path, line: &[u8], max_bytes: u64) -> std::io::Result<()> {
    let _guard = LOG_WRITE_LOCK
        .lock()
        .map_err(|_| std::io::Error::other("diagnostic log lock poisoned"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // A single line longer than the whole budget would otherwise rotate on
    // every write and still leave an over-cap file; clamp it instead.
    let max_line = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    let line = &line[..line.len().min(max_line)];
    let current = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    if current.saturating_add(line.len() as u64) > max_bytes {
        rotate_logs(path)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line)
}

fn rotate_logs(path: &Path) -> std::io::Result<()> {
    let oldest = rotated_path(path, LOG_BACKUPS)?;
    match std::fs::remove_file(&oldest) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    for index in (1..LOG_BACKUPS).rev() {
        let source = rotated_path(path, index)?;
        let target = rotated_path(path, index + 1)?;
        if source.exists() {
            std::fs::rename(source, target)?;
        }
    }
    if path.exists() {
        std::fs::rename(path, rotated_path(path, 1)?)?;
    }
    Ok(())
}

fn rotated_path(path: &Path, index: usize) -> std::io::Result<PathBuf> {
    let filename = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("diagnostic log path has no filename"))?;
    let mut rotated = OsString::from(filename);
    rotated.push(format!(".{index}"));
    Ok(path.with_file_name(rotated))
}

fn sanitize_message(msg: &str) -> String {
    let mut sanitized = msg.to_string();
    // These keys have carried transcripts, generated JSON, extracted slots,
    // command message targets, or screen content in current/past call sites.
    for key in [
        "raw",
        "input_preview",
        "slots",
        "transcript",
        "utterance",
        "message",
        "content",
        "screen_context",
    ] {
        sanitized = redact_keyed_values(&sanitized, key);
    }
    // Remaining quoted values may include application names, configured
    // executable paths, or user-defined command payloads. Keep the diagnostic
    // shape while removing both Rust-debug double quotes and ad-hoc single
    // quotes used by older call sites.
    sanitized = redact_quoted_values(&sanitized);
    truncate_utf8(&mut sanitized, MAX_LINE_BYTES);
    sanitized
}

fn redact_keyed_values(input: &str, key: &str) -> String {
    let marker = format!("{key}=");
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while let Some(relative) = input[cursor..].find(&marker) {
        let marker_start = cursor + relative;
        let value_start = marker_start + marker.len();
        output.push_str(&input[cursor..value_start]);
        let value_end = sensitive_value_end(input, value_start);
        output.push_str(&format!("<redacted:{} bytes>", value_end - value_start));
        cursor = value_end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn sensitive_value_end(input: &str, start: usize) -> usize {
    let bytes = input.as_bytes();
    if start >= bytes.len() {
        return start;
    }
    match bytes[start] {
        b'"' => quoted_end(bytes, start),
        b'{' | b'[' => balanced_end(bytes, start),
        _ => bytes[start..]
            .iter()
            .position(|b| b.is_ascii_whitespace() || *b == b',')
            .map(|offset| start + offset)
            .unwrap_or(bytes.len()),
    }
}

fn quoted_end(bytes: &[u8], start: usize) -> usize {
    let mut escaped = false;
    for (offset, byte) in bytes[start + 1..].iter().enumerate() {
        if escaped {
            escaped = false;
        } else if *byte == b'\\' {
            escaped = true;
        } else if *byte == b'"' {
            return start + offset + 2;
        }
    }
    bytes.len()
}

fn balanced_end(bytes: &[u8], start: usize) -> usize {
    let opening = bytes[start];
    let closing = if opening == b'{' { b'}' } else { b']' };
    let mut depth = 0_u32;
    let mut quoted = false;
    let mut escaped = false;
    for (offset, byte) in bytes[start..].iter().enumerate() {
        if quoted {
            if escaped {
                escaped = false;
            } else if *byte == b'\\' {
                escaped = true;
            } else if *byte == b'"' {
                quoted = false;
            }
            continue;
        }
        if *byte == b'"' {
            quoted = true;
        } else if *byte == opening {
            depth += 1;
        } else if *byte == closing {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return start + offset + 1;
            }
        }
    }
    bytes.len()
}

fn redact_quoted_values(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        let Some(relative) = bytes[cursor..]
            .iter()
            .position(|b| *b == b'"' || *b == b'\'')
        else {
            output.push_str(&input[cursor..]);
            break;
        };
        let start = cursor + relative;
        output.push_str(&input[cursor..start]);
        let end = quoted_end(bytes, start);
        let delimiter = bytes[start] as char;
        output.push(delimiter);
        output.push_str(&format!("<redacted:{} bytes>", end - start));
        output.push(delimiter);
        cursor = end;
    }
    output
}

fn truncate_utf8(value: &mut String, max_bytes: usize) {
    if value.len() <= max_bytes {
        return;
    }
    const SUFFIX: &str = "...[truncated]";
    let mut boundary = max_bytes.saturating_sub(SUFFIX.len());
    while !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value.truncate(boundary);
    value.push_str(SUFFIX);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_transcript_and_generated_payload_fields() {
        let secret = "private customer message";
        for message in [
            format!("extract input_preview={secret:?} input_chars=24"),
            format!("generate raw={secret:?} -> 1 intent"),
            format!("extract slots={{\"body\":\"{secret}\"}}"),
            format!("LaunchApp: failed to spawn '{secret}'"),
        ] {
            let sanitized = sanitize_message(&message);
            assert!(!sanitized.contains(secret));
            assert!(sanitized.contains("<redacted:"));
        }
    }

    #[test]
    fn diagnostic_outcomes_are_reduced_to_safe_categories() {
        assert_eq!(outcome_category("ok"), "ok");
        assert_eq!(
            outcome_category("Extraction timed out: private text"),
            "timeout"
        );
        assert_eq!(outcome_category("No LLM model available"), "unavailable");
        assert_eq!(outcome_category("parse failed near private text"), "error");
    }

    #[test]
    fn bounded_writer_rotates_old_logs() {
        let dir =
            std::env::temp_dir().join(format!("omnivox-diaglog-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("structured-mode.log");
        write_bounded_line(&path, b"first line that fills the file\n", 32).unwrap();
        write_bounded_line(&path, b"second line rotates the first\n", 32).unwrap();
        assert!(path.is_file());
        assert!(rotated_path(&path, 1).unwrap().is_file());
        std::fs::remove_dir_all(dir).ok();
    }
}
