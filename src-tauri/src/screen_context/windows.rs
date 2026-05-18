//! Windows UI Automation capture of visible text in the foreground window.
//!
//! Strategy: walk the Content view of the UIA tree starting from the
//! foreground HWND, depth- and node-capped, collecting text from every
//! element that exposes a usable text source.  Three sources, in priority:
//!
//!   1. `TextPattern.GetVisibleRanges()` — the gold standard.  Code editors,
//!      terminals, web text frames return the exact visible characters.
//!   2. `Value` property — typed text in edit controls, address bars.
//!   3. `Name` property — labels, tab titles, file-tree items, buttons.
//!
//! All capture runs on a dedicated thread with COM initialized as MTA.
//! A wall-clock watchdog terminates the walk at ~250 ms.  No errors
//! propagate; failure and timeout paths return no captured text.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{IUnknown, Interface, BSTR};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationTreeWalker, TreeScope_Subtree, UIA_NamePropertyId,
    UIA_TextPatternId, UIA_ValueValuePropertyId,
};

const MAX_NODES: usize = 500;
const MAX_TEXT_BYTES: usize = 8 * 1024;
const TIMEOUT_MS: u64 = 250;
const MAX_DEPTH: usize = 12;

/// Capture the visible text of the foreground window.  Returns the joined
/// best-effort text, or `None` on timeout or hard failure.
pub fn capture_text(hwnd: isize) -> Option<String> {
    let abort = Arc::new(AtomicBool::new(false));
    let abort_for_thread = Arc::clone(&abort);

    let join = std::thread::Builder::new()
        .name("omnivox-uia".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(move || -> Option<String> {
            unsafe { capture_inner(hwnd, &abort_for_thread) }
        })
        .ok()?;

    let deadline = Instant::now() + Duration::from_millis(TIMEOUT_MS);
    loop {
        if join.is_finished() {
            return join.join().ok().flatten();
        }
        if Instant::now() >= deadline {
            abort.store(true, Ordering::Relaxed);
            // Detach — we can't safely interrupt mid-COM call, but we've
            // signaled the walker to stop on its next node.  The thread
            // will finish promptly and clean itself up.
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

unsafe fn capture_inner(hwnd: isize, abort: &AtomicBool) -> Option<String> {
    // CoInit failure is non-fatal (S_FALSE means already initialized on this
    // thread, which won't happen since we just spawned).  Treat any HRESULT
    // failure as "skip this capture".
    let init_hr = CoInitializeEx(None, COINIT_MULTITHREADED);
    if init_hr.is_err() {
        super::diaglog::log(&format!("uia: CoInitializeEx failed: {init_hr:?}"));
        return None;
    }

    let result = (|| -> Option<String> {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None::<&IUnknown>, CLSCTX_INPROC_SERVER).ok()?;

        let root = automation.ElementFromHandle(HWND(hwnd as *mut _)).ok()?;

        let walker = automation.ContentViewWalker().ok()?;

        let mut buf = String::new();
        let mut nodes: usize = 0;
        let mut stack: Vec<(IUIAutomationElement, usize)> = vec![(root, 0)];

        while let Some((elem, depth)) = stack.pop() {
            if abort.load(Ordering::Relaxed) {
                super::diaglog::log("uia: aborted by watchdog");
                break;
            }
            if nodes >= MAX_NODES || buf.len() >= MAX_TEXT_BYTES {
                break;
            }
            nodes += 1;

            extract_element_text(&elem, &mut buf);

            if buf.len() >= MAX_TEXT_BYTES {
                break;
            }

            if depth + 1 > MAX_DEPTH {
                continue;
            }

            // Push children in reverse so we walk left-to-right (nice for
            // logs; UIA itself doesn't guarantee visual order anyway).
            push_children(&walker, &elem, depth + 1, &mut stack);
        }

        if buf.len() > MAX_TEXT_BYTES {
            buf.truncate(MAX_TEXT_BYTES);
        }
        Some(buf)
    })();

    CoUninitialize();
    result
}

unsafe fn extract_element_text(elem: &IUIAutomationElement, buf: &mut String) {
    // 1. TextPattern — preferred for code editors, terminals, doc viewers.
    if let Ok(pattern_unk) = elem.GetCurrentPattern(UIA_TextPatternId) {
        if let Ok(text_pattern) = pattern_unk.cast::<IUIAutomationTextPattern>() {
            if let Ok(ranges) = text_pattern.GetVisibleRanges() {
                if let Ok(len) = ranges.Length() {
                    for i in 0..len {
                        if buf.len() >= MAX_TEXT_BYTES {
                            return;
                        }
                        if let Ok(range) = ranges.GetElement(i) {
                            // -1 means "all text".  We don't need a hard
                            // per-range cap because the outer MAX_TEXT_BYTES
                            // check below stops us anyway.
                            if let Ok(text) = range.GetText(2048) {
                                append_unique_chunk(buf, &bstr_to_string(&text));
                            }
                        }
                    }
                }
            }
        }
    }

    if buf.len() >= MAX_TEXT_BYTES {
        return;
    }

    // 2. Value property (edit boxes, address bars).
    if let Ok(value) = elem.GetCurrentPropertyValue(UIA_ValueValuePropertyId) {
        if let Some(s) = variant_to_string(&value) {
            append_unique_chunk(buf, &s);
        }
    }

    if buf.len() >= MAX_TEXT_BYTES {
        return;
    }

    // 3. Name — labels, tab titles, file tree items, button text.
    if let Ok(name) = elem.GetCurrentPropertyValue(UIA_NamePropertyId) {
        if let Some(s) = variant_to_string(&name) {
            append_unique_chunk(buf, &s);
        }
    }
}

unsafe fn push_children(
    walker: &IUIAutomationTreeWalker,
    parent: &IUIAutomationElement,
    depth: usize,
    stack: &mut Vec<(IUIAutomationElement, usize)>,
) {
    // Walker.GetFirstChildElement / GetNextSiblingElement returns Err
    // for empty results, which we silently ignore.
    let mut child = match walker.GetFirstChildElement(parent) {
        Ok(c) => c,
        Err(_) => return,
    };
    let mut local: Vec<IUIAutomationElement> = Vec::new();
    loop {
        local.push(child.clone());
        if local.len() >= 64 {
            // Wide nodes (huge listboxes, file trees) — don't push more.
            break;
        }
        match walker.GetNextSiblingElement(&child) {
            Ok(next) => child = next,
            Err(_) => break,
        }
    }
    for c in local.into_iter().rev() {
        stack.push((c, depth));
    }
    // A NULL HRESULT_FROM_WIN32 (no more siblings) ends the loop above
    // naturally — TreeScope_Subtree alternative would be cleaner but
    // creates a single huge node array that bypasses our incremental cap.
    let _ = TreeScope_Subtree;
}

fn bstr_to_string(b: &BSTR) -> String {
    b.to_string()
}

unsafe fn variant_to_string(v: &VARIANT) -> Option<String> {
    let s: BSTR = v.try_into().ok()?;
    let out = s.to_string();
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Append `chunk` to `buf` with a separating space if useful, dropping
/// near-duplicates of the immediately preceding chunk.  Trims the chunk
/// itself; very-short chunks (1-2 chars, common for icon buttons) are
/// dropped to keep noise out of the token extractor.
fn append_unique_chunk(buf: &mut String, chunk: &str) {
    let trimmed = chunk.trim();
    if trimmed.len() < 2 {
        return;
    }
    if buf.ends_with(trimmed) {
        return;
    }
    if !buf.is_empty() {
        buf.push(' ');
    }
    buf.push_str(trimmed);
}
