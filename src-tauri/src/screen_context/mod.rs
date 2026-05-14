//! Screen-context capture for verbatim transcription of technical strings.
//!
//! Reads visible text from the foreground window via UI Automation (Windows),
//! extracts a ranked list of "atypical" tokens (file paths, identifiers, CLI
//! flags, etc.), and surfaces them to the rest of the pipeline.  Phase 1 feeds
//! the tokens into Whisper as an `initial_prompt` to bias decoding.  Phase 2
//! also passes them into Qwen so Structured Mode can reconcile phonetic
//! guesses with verbatim screen tokens.
//!
//! Failure mode: every public entry point is infallible — on any error
//! (no foreground hwnd, COM init failure, UIA timeout) we return an empty
//! `ScreenContext` so the pipeline proceeds with vocabulary-only biasing.

pub mod diaglog;
mod extract;

#[cfg(target_os = "windows")]
mod windows;

#[cfg(test)]
mod tests;

use std::time::Instant;

/// Snapshot of visible screen text and the technical tokens extracted from it.
#[derive(Debug, Clone)]
pub struct ScreenContext {
    /// Joined visible text from the foreground window, capped at ~8 KB.
    pub raw_text: String,
    /// Ranked technical tokens (file paths, identifiers, slash commands, …).
    /// Top-N results, deduplicated case-insensitively, original casing kept.
    pub tokens: Vec<String>,
    /// Process executable name of the foreground window (e.g. `Code.exe`).
    pub source_app: Option<String>,
    /// When the capture completed.  Set even on empty results.
    pub captured_at: Instant,
}

impl Default for ScreenContext {
    fn default() -> Self {
        Self {
            raw_text: String::new(),
            tokens: Vec::new(),
            source_app: None,
            captured_at: Instant::now(),
        }
    }
}

impl ScreenContext {
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }
}

/// Maximum tokens we feed Whisper — well below whisper.cpp's ~224-token
/// `initial_prompt` cap, leaving room for the user's vocabulary entries.
pub const MAX_SCREEN_TOKENS: usize = 30;

/// Token cap for Whisper's `initial_prompt` after merging vocabulary +
/// screen tokens.  whisper.cpp truncates silently above ~224 tokens; we
/// stay conservative.  Counted in *whitespace-separated* terms, not real
/// BPE tokens — close enough for our purposes.
const WHISPER_PROMPT_TERM_CAP: usize = 180;

/// Screen-context contribution to the Whisper prompt — much tighter than
/// the overall `MAX_SCREEN_TOKENS` (which Phase 2 / Qwen also sees).  A
/// long prompt biases Whisper too aggressively toward prompt content even
/// when the user said something unrelated; 15 alphabetic-dominant tokens
/// is enough to nail verbatim recognition without overpowering the
/// decoder.
const WHISPER_SCREEN_TOKEN_CAP: usize = 15;

/// Process names we never capture screen context for — would either feed
/// our own UI back into the model (loop) or has no useful technical text.
const SKIP_APPS: &[&str] = &["omnivox.exe", "omnivoice.exe"];

/// Capture screen context for the given foreground window handle.
///
/// Returns `Default` (empty context) on any error.  Caller must NOT treat
/// failure as fatal — the pipeline degrades gracefully to vocabulary-only
/// biasing when this returns empty.
///
/// Wall-clock target: < 250 ms.  A watchdog terminates the UIA walk if it
/// exceeds that; the partial result so far is returned.
pub fn capture(hwnd: Option<isize>) -> ScreenContext {
    let Some(hwnd) = hwnd else {
        diaglog::log("capture: no foreground hwnd, skipping");
        return ScreenContext::default();
    };

    let source_app = process_name_from_hwnd(hwnd);

    if let Some(name) = source_app.as_deref() {
        if SKIP_APPS.iter().any(|&a| a.eq_ignore_ascii_case(name)) {
            diaglog::log(&format!("capture: skipping own app ({name})"));
            return ScreenContext {
                source_app,
                ..Default::default()
            };
        }
    }

    let t0 = Instant::now();
    let raw_text = platform_capture(hwnd);
    let dur_ms = t0.elapsed().as_millis();

    if raw_text.is_empty() {
        diaglog::log(&format!(
            "capture: 0 chars from app={:?} dur={}ms",
            source_app, dur_ms
        ));
        return ScreenContext {
            source_app,
            ..Default::default()
        };
    }

    let tokens = extract::rank_tokens(&raw_text, MAX_SCREEN_TOKENS);

    diaglog::log(&format!(
        "capture: app={:?} chars={} tokens={} dur={}ms",
        source_app,
        raw_text.len(),
        tokens.len(),
        dur_ms
    ));

    ScreenContext {
        raw_text,
        tokens,
        source_app,
        captured_at: Instant::now(),
    }
}

#[cfg(target_os = "windows")]
fn platform_capture(hwnd: isize) -> String {
    windows::capture_text(hwnd).unwrap_or_default()
}

#[cfg(not(target_os = "windows"))]
fn platform_capture(_hwnd: isize) -> String {
    String::new()
}

#[cfg(target_os = "windows")]
fn process_name_from_hwnd(hwnd: isize) -> Option<String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd as *mut std::ffi::c_void, &mut pid as *mut u32);
        if pid == 0 {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut buf = [0u16; 260];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len);
        CloseHandle(handle);
        if ok == 0 || len == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit('\\').next().map(|s| s.to_string())
    }
}

#[cfg(not(target_os = "windows"))]
fn process_name_from_hwnd(_hwnd: isize) -> Option<String> {
    None
}

/// Build the merged Whisper `initial_prompt` from existing vocabulary +
/// screen tokens, respecting the term cap.  Returns `None` when nothing
/// would be added (lets callers cleanly restore the prior prompt).
///
/// Vocabulary tokens come first (already user-curated) so they never get
/// crowded out.  Screen tokens fill remaining capacity.  Both are joined
/// with spaces — whisper.cpp treats the prompt as a free-form text bias.
pub fn build_initial_prompt(ctx: &ScreenContext, base: Option<&str>) -> Option<String> {
    let base_terms: Vec<&str> = base
        .map(|s| s.split_whitespace().collect())
        .unwrap_or_default();
    if ctx.tokens.is_empty() && base_terms.is_empty() {
        return None;
    }

    let mut out: Vec<String> = Vec::with_capacity(WHISPER_PROMPT_TERM_CAP);
    let mut seen_lower: std::collections::HashSet<String> = std::collections::HashSet::new();

    for term in &base_terms {
        if out.len() >= WHISPER_PROMPT_TERM_CAP {
            break;
        }
        let key = term.to_lowercase();
        if seen_lower.insert(key) {
            out.push((*term).to_string());
        }
    }

    let mut screen_added = 0;
    for token in &ctx.tokens {
        if out.len() >= WHISPER_PROMPT_TERM_CAP || screen_added >= WHISPER_SCREEN_TOKEN_CAP {
            break;
        }
        // Hard filter: alphabetic-dominant only.  Versions, hashes,
        // timestamps, and other numeric-heavy tokens corrupt Whisper's
        // decoding by biasing it toward producing numbers in place of
        // dictated words.  Phase 2 (Qwen) gets the full list separately.
        if !extract::is_useful_for_whisper(token) {
            continue;
        }
        let key = token.to_lowercase();
        if seen_lower.insert(key) {
            out.push(token.clone());
            screen_added += 1;
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out.join(" "))
    }
}
