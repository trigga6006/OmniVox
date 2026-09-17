//! Which GPU did Vulkan actually hand us? Always-on capture plus a one-shot
//! user alert.
//!
//! ggml's Vulkan backend prints its device table through the library log
//! callback once per process (`ggml_vulkan: 0 = <name> (<driver>) | uma: <0|1>
//! | ...`).  That table is the only place the pinned whisper-rs / llama-cpp-2
//! crates expose the integrated-vs-discrete distinction — and it is exactly
//! what went invisible when a driver update left this machine with an
//! iGPU-only Vulkan: every load still reported `backend=gpu_full`, but the
//! model landed in system RAM (an iGPU's "VRAM" is shared memory) at a
//! fraction of the speed.  The callbacks below tee those lines into a small
//! parsed registry, mirroring everything to stderr so dev consoles lose
//! nothing.  After a GPU-backed model load the registry is summarized into the
//! always-on model-load log, and when every visible Vulkan device is
//! integrated the app emits a once-per-session `gpu-environment-warning`
//! event for the overlay banner.

use std::ffi::{c_char, c_void, CStr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

#[derive(Debug, Clone, PartialEq, Eq)]
struct VulkanDevice {
    name: String,
    /// ggml's `uma:` flag — unified memory, i.e. an integrated GPU whose
    /// "video memory" is system RAM.
    integrated: bool,
}

/// Both bundled ggml copies (whisper.cpp's and llama.cpp's) report here, so
/// entries are deduped by value and capped defensively.
static DEVICES: Mutex<Vec<VulkanDevice>> = Mutex::new(Vec::new());
/// Set when a model load actually ran on the GPU backend — keeps CPU-only
/// sessions (setting off, CPU fallback) from ever warning.
static GPU_LOAD_SEEN: AtomicBool = AtomicBool::new(false);
/// The warning fires at most once per app session.
static WARNED: AtomicBool = AtomicBool::new(false);
static INSTALL: Once = Once::new();

const MAX_DEVICES: usize = 16;

/// Install the log-capture callbacks.  Must run before the first model load;
/// idempotent afterwards.
pub fn install_log_capture() {
    INSTALL.call_once(|| unsafe {
        // whisper.cpp and llama.cpp each statically bundle their own ggml, and
        // each routes backend-init lines through whichever hook is set on that
        // copy — so set both the library-level and ggml-level hooks on both.
        whisper_rs::whisper_rs_sys::whisper_log_set(Some(capture_whisper), std::ptr::null_mut());
        whisper_rs::whisper_rs_sys::ggml_log_set(Some(capture_whisper), std::ptr::null_mut());
        llama_cpp_sys_2::llama_log_set(Some(capture_llama), std::ptr::null_mut());
        llama_cpp_sys_2::ggml_log_set(Some(capture_llama), std::ptr::null_mut());
    });
}

// The two sys crates declare structurally identical but nominally distinct
// callback types, so each needs its own extern shim.
unsafe extern "C" fn capture_whisper(
    _level: whisper_rs::whisper_rs_sys::ggml_log_level,
    text: *const c_char,
    _user_data: *mut c_void,
) {
    capture(text);
}

unsafe extern "C" fn capture_llama(
    _level: llama_cpp_sys_2::ggml_log_level,
    text: *const c_char,
    _user_data: *mut c_void,
) {
    capture(text);
}

/// Shared callback body.  Runs on library loader threads mid-FFI, so it must
/// never panic: lossy conversion, checked parsing, no unwraps.
fn capture(text: *const c_char) {
    if text.is_null() {
        return;
    }
    // SAFETY: the libraries pass a valid nul-terminated string; same trust as
    // whisper-rs's own logging trampoline.
    let line = unsafe { CStr::from_ptr(text) }.to_string_lossy();
    if let Some(device) = parse_device_line(&line) {
        if let Ok(mut devices) = DEVICES.lock() {
            if devices.len() < MAX_DEVICES && !devices.contains(&device) {
                devices.push(device);
            }
        }
    }
    // Installing a callback silences the libraries' default stderr output;
    // mirror it so dev consoles still show load progress.  eprint! is a no-op
    // (not a panic) in windowed builds without a console.
    eprint!("{line}");
}

/// Parse one `ggml_vulkan: <idx> = <name> (<driver>) | uma: <0|1> | ...` line.
fn parse_device_line(line: &str) -> Option<VulkanDevice> {
    let rest = line.split("ggml_vulkan: ").nth(1)?;
    let (index, rest) = rest.split_once(" = ")?;
    index.trim().parse::<u32>().ok()?;
    let (described, uma) = rest.split_once(" | uma: ")?;
    // The device name may itself contain parentheses ("AMD Radeon(TM)
    // Graphics"); the driver annotation is the LAST " (...)" group, so strip
    // from the rightmost opener only.
    let name = match described.rfind(" (") {
        Some(at) => &described[..at],
        None => described,
    }
    .trim();
    if name.is_empty() {
        return None;
    }
    Some(VulkanDevice {
        name: name.to_string(),
        integrated: uma.trim_start().starts_with('1'),
    })
}

fn snapshot() -> Vec<VulkanDevice> {
    DEVICES.lock().map(|d| d.clone()).unwrap_or_default()
}

fn describe(device: &VulkanDevice) -> String {
    format!(
        "{} ({})",
        device.name,
        if device.integrated {
            "integrated"
        } else {
            "discrete"
        }
    )
}

/// Record that a model load ran on the GPU backend and summarize the captured
/// Vulkan device table into the always-on model-load log.  `context` names the
/// load ("asr" / "llm").
pub fn note_gpu_load(context: &str) {
    GPU_LOAD_SEEN.store(true, Ordering::Release);
    let devices = snapshot();
    if devices.is_empty() {
        // CPU-only build, capture not installed, or a ggml that logs devices
        // differently — nothing trustworthy to report.
        return;
    }
    let list = devices.iter().map(describe).collect::<Vec<_>>().join(", ");
    crate::diag::log(&format!("gpu env ({context}): vulkan devices = [{list}]"));
}

/// Emit the once-per-session warning if a GPU-backed load happened and every
/// Vulkan device ggml can see is integrated (UMA): model memory is then in
/// system RAM, typically because a dedicated GPU lost its Vulkan driver
/// registration (seen after graphics driver updates).
pub fn maybe_warn_integrated_only(app: &tauri::AppHandle) {
    use tauri::Emitter;

    if !GPU_LOAD_SEEN.load(Ordering::Acquire) {
        return;
    }
    let devices = snapshot();
    if devices.is_empty() || devices.iter().any(|device| !device.integrated) {
        return;
    }
    if WARNED.swap(true, Ordering::AcqRel) {
        return;
    }
    let message = format!(
        "GPU acceleration is running on the integrated GPU ({}) — no dedicated GPU is visible to Vulkan, so model memory sits in system RAM and inference is slower. This usually follows a graphics driver update; reinstalling the GPU driver normally restores it.",
        devices[0].name
    );
    crate::diag::log(&format!("gpu env warning: {message}"));
    let _ = app.emit("gpu-environment-warning", &message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_discrete_device_line() {
        let line = "ggml_vulkan: 0 = AMD Radeon RX 7800 XT (AMD proprietary driver) | uma: 0 | fp16: 1 | bf16: 0 | warp size: 64 | shared memory: 32768 | int dot: 1 | matrix cores: KHR_coopmat\n";
        let device = parse_device_line(line).expect("should parse");
        assert_eq!(device.name, "AMD Radeon RX 7800 XT");
        assert!(!device.integrated);
    }

    #[test]
    fn parses_integrated_device_keeping_tm_parenthetical() {
        let line = "ggml_vulkan: 1 = AMD Radeon(TM) Graphics (AMD proprietary driver) | uma: 1 | fp16: 1 | bf16: 0 | warp size: 32 | shared memory: 32768 | int dot: 1 | matrix cores: none\n";
        let device = parse_device_line(line).expect("should parse");
        assert_eq!(device.name, "AMD Radeon(TM) Graphics");
        assert!(device.integrated);
    }

    #[test]
    fn parses_line_without_driver_annotation() {
        let line = "ggml_vulkan: 0 = NVIDIA GeForce RTX 4090 | uma: 0 | fp16: 1\n";
        let device = parse_device_line(line).expect("should parse");
        assert_eq!(device.name, "NVIDIA GeForce RTX 4090");
        assert!(!device.integrated);
    }

    #[test]
    fn rejects_non_device_lines() {
        assert_eq!(
            parse_device_line("ggml_vulkan: Found 2 Vulkan devices:\n"),
            None
        );
        assert_eq!(
            parse_device_line("llama_model_loader: - kv 17: tokenizer = gpt2\n"),
            None
        );
        assert_eq!(
            parse_device_line("ggml_vulkan: 0 = Some GPU without uma field\n"),
            None
        );
    }
}
