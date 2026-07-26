//! Executes a resolved [`CommandIntent`] against the OS.
//!
//! These are the low-level primitives.  Focus restoration (so keystrokes land in
//! the user's target app, not the overlay) and event emission are the caller's
//! job — see the command path in `pipeline`.  App launching lives in
//! [`super::app_index`]; this module covers key chords, media keys, and window
//! actions.

use enigo::{Direction, Enigo, Key, Keyboard, Settings};

use crate::actions::intent::{KeyChord, MediaAction, WindowAction};

/// Primary chord modifier — Ctrl on Windows/Linux, Cmd on macOS.
#[cfg(target_os = "macos")]
const PRIMARY_MOD: Key = Key::Meta;
#[cfg(not(target_os = "macos"))]
const PRIMARY_MOD: Key = Key::Control;

/// Press `mods` (in order), click `key`, release `mods` (reverse order).
fn chord(enigo: &mut Enigo, mods: &[Key], key: Key) -> Result<(), String> {
    for m in mods {
        enigo
            .key(*m, Direction::Press)
            .map_err(|e| format!("keystroke failed: {e}"))?;
    }
    let click = enigo
        .key(key, Direction::Click)
        .map_err(|e| format!("keystroke failed: {e}"));
    // Always release modifiers, even if the click failed, so we never leave a
    // modifier stuck down in the target app.
    for m in mods.iter().rev() {
        let _ = enigo.key(*m, Direction::Release);
    }
    click
}

/// Fire a keyboard chord into the foreground app.
pub fn run_chord(c: KeyChord) -> Result<(), String> {
    let mut enigo =
        Enigo::new(&Settings::default()).map_err(|e| format!("keystroke engine init failed: {e}"))?;
    match c {
        KeyChord::Copy => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('c')),
        KeyChord::Paste => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('v')),
        KeyChord::Cut => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('x')),
        KeyChord::Undo => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('z')),
        // Ctrl/Cmd+Shift+Z — matches the inline-dictation redo and has the
        // widest modern-editor coverage (Ctrl+Y no-ops in Chrome/ProseMirror inputs).
        KeyChord::Redo => chord(&mut enigo, &[PRIMARY_MOD, Key::Shift], Key::Unicode('z')),
        KeyChord::SelectAll => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('a')),
        KeyChord::Save => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('s')),
        KeyChord::NewTab => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('t')),
        KeyChord::CloseTab => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('w')),
        KeyChord::Screenshot => screenshot(&mut enigo),
        KeyChord::ShowDesktop => show_desktop(&mut enigo),
        KeyChord::Refresh => chord(&mut enigo, &[], Key::F5),
        KeyChord::Find => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('f')),
        KeyChord::NewWindow => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('n')),
        KeyChord::ReopenTab => chord(&mut enigo, &[PRIMARY_MOD, Key::Shift], Key::Unicode('t')),
        KeyChord::NextTab => chord(&mut enigo, &[PRIMARY_MOD], Key::Tab),
        KeyChord::PrevTab => chord(&mut enigo, &[PRIMARY_MOD, Key::Shift], Key::Tab),
        KeyChord::PageDown => chord(&mut enigo, &[], Key::PageDown),
        KeyChord::PageUp => chord(&mut enigo, &[], Key::PageUp),
    }
}

#[cfg(target_os = "windows")]
fn show_desktop(enigo: &mut Enigo) -> Result<(), String> {
    // Win+D — toggle "show the desktop" (minimize/restore all). Reversible.
    chord(enigo, &[Key::Meta], Key::Unicode('d'))
}

#[cfg(not(target_os = "windows"))]
fn show_desktop(_enigo: &mut Enigo) -> Result<(), String> {
    Err("Show desktop is only wired up on Windows".into())
}

#[cfg(target_os = "windows")]
fn screenshot(enigo: &mut Enigo) -> Result<(), String> {
    // Win+Shift+S — the built-in region snip.
    chord(enigo, &[Key::Meta, Key::Shift], Key::Unicode('s'))
}

#[cfg(not(target_os = "windows"))]
fn screenshot(_enigo: &mut Enigo) -> Result<(), String> {
    Err("Screenshot is only wired up on Windows".into())
}

/// Fire a media-transport / volume key via raw virtual-key codes (stable across
/// keyboards; `enigo`'s media-key coverage varies by version).
#[cfg(windows)]
pub fn run_media(a: MediaAction) -> Result<(), String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        keybd_event, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    };

    // Mute/Unmute are set deterministically via WASAPI (below) rather than the
    // VK_VOLUME_MUTE toggle, so a directional request ("unmute", "sound on")
    // can't invert the state and silence audio that's already on.
    match a {
        MediaAction::Mute => return set_system_mute(true),
        MediaAction::Unmute => return set_system_mute(false),
        _ => {}
    }

    // VK_MEDIA_* / VK_VOLUME_* codes.
    let vk: u8 = match a {
        MediaAction::PlayPause => 0xB3,
        MediaAction::NextTrack => 0xB0,
        MediaAction::PrevTrack => 0xB1,
        MediaAction::VolumeUp => 0xAF,
        MediaAction::VolumeDown => 0xAE,
        MediaAction::Mute | MediaAction::Unmute => unreachable!("handled above"),
    };
    // Windows moves master volume only ~2% per VK_VOLUME_UP/DOWN press, so a
    // single press reads as "nothing happened". Repeat for an audible step
    // (~8%). Transport keys stay single-press.
    let repeats = if matches!(a, MediaAction::VolumeUp | MediaAction::VolumeDown) {
        4
    } else {
        1
    };
    unsafe {
        for _ in 0..repeats {
            keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY, 0);
            keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
        }
    }
    Ok(())
}

/// Set the default render endpoint's mute state deterministically via WASAPI.
/// Unlike VK_VOLUME_MUTE (a blind toggle), this honors the requested direction —
/// `set_system_mute(false)` can never silence audio that's already on. Mirrors
/// the COM setup in `audio::ducking`.
#[cfg(windows)]
fn set_system_mute(muted: bool) -> Result<(), String> {
    use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
    use windows::Win32::Media::Audio::{
        eMultimedia, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("audio endpoint enumerate failed: {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eMultimedia)
            .map_err(|e| format!("no default audio endpoint: {e}"))?;
        let vol = device
            .Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None)
            .map_err(|e| format!("activate endpoint volume failed: {e}"))?;
        vol.SetMute(muted, std::ptr::null())
            .map_err(|e| format!("set mute failed: {e}"))?;
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run_media(_a: MediaAction) -> Result<(), String> {
    Err("Media keys are only supported on Windows".into())
}

/// Minimize/maximize a target window (the foreground window captured at command
/// start). `hwnd` is the platform handle from `focus::capture_foreground_window`.
#[cfg(windows)]
pub fn run_window(a: WindowAction, hwnd: Option<isize>) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MAXIMIZE, SW_MINIMIZE};

    let hwnd = hwnd.ok_or("No target window to act on")?;
    let cmd = match a {
        WindowAction::Minimize => SW_MINIMIZE,
        WindowAction::Maximize => SW_MAXIMIZE,
    };
    unsafe {
        ShowWindow(hwnd as *mut core::ffi::c_void, cmd);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run_window(_a: WindowAction, _hwnd: Option<isize>) -> Result<(), String> {
    Err("Window control is only supported on Windows".into())
}

/// Gracefully close a target window via WM_CLOSE (so the app runs its OWN
/// save-prompt — never a forced kill). `hwnd` is the foreground window captured
/// at command start. Consequential, so the pipeline confirms before calling.
#[cfg(windows)]
pub fn run_close_window(hwnd: Option<isize>) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_CLOSE};

    let hwnd = hwnd.ok_or("No target window to close")?;
    // PostMessage (not SendMessage) so we don't block on the app's own
    // close handling (e.g. an unsaved-changes dialog).
    let ok = unsafe { PostMessageW(hwnd as *mut core::ffi::c_void, WM_CLOSE, 0, 0) };
    if ok == 0 {
        return Err("Failed to close window".into());
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run_close_window(_hwnd: Option<isize>) -> Result<(), String> {
    Err("Closing windows is only supported on Windows".into())
}

/// Restore a previously minimized window ("undo that" after a minimize).
#[cfg(windows)]
pub fn run_restore_window(hwnd: isize) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_RESTORE};
    unsafe {
        ShowWindow(hwnd as *mut core::ffi::c_void, SW_RESTORE);
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn run_restore_window(_hwnd: isize) -> Result<(), String> {
    Err("Window control is only supported on Windows".into())
}

/// Best-effort window title for confirm UX ("Close \"Untitled - Notepad\"?").
/// Empty string if it can't be read.
#[cfg(windows)]
pub fn window_title(hwnd: isize) -> String {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowTextW;

    let mut buf = [0u16; 256];
    let len = unsafe {
        GetWindowTextW(
            hwnd as *mut core::ffi::c_void,
            buf.as_mut_ptr(),
            buf.len() as i32,
        )
    };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

#[cfg(not(windows))]
pub fn window_title(_hwnd: isize) -> String {
    String::new()
}

/// Type text into `target` via the dictation path's clipboard-verified paste
/// (prior clipboard restored).  Restores focus to the bound target and VERIFIES
/// its identity (IsWindow + foreground + PID) before touching the clipboard, and
/// RE-VERIFIES between the paste and the submitting Enter — a submit is the whole
/// risk, so the window must still be the one we confirmed at both gates.
///
/// `should_cancel` is polled before the paste and again between the paste and the
/// submitting Enter: a "stop"/"cancel" spoken while we restore focus or hold the
/// post-paste guard must abort before the (consequential) Enter fires — the
/// caller's pre-check alone can't see a stop that lands during this blocking
/// work (B2-13).
pub fn run_type_text(
    text: &str,
    submit: bool,
    target: crate::focus::WindowTarget,
    should_cancel: impl Fn() -> bool,
) -> Result<(), String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("No text to type".into());
    }
    if should_cancel() {
        return Err("stopped".into());
    }
    // Bind + verify the target before pasting: refuse to type into a window that
    // isn't the one this command was aimed at.
    crate::focus::restore_foreground_window(target.hwnd, target.pid);
    if !crate::focus::verify_foreground_target(target.hwnd, target.pid) {
        return Err("Target window is not in focus — refusing to type".into());
    }
    // Re-check cancellation immediately before the paste (focus restore slept).
    if should_cancel() {
        return Err("stopped".into());
    }
    crate::output::router::OutputRouter::new()
        .paste_text(text, true)
        .map_err(|e| e.to_string())?;
    if submit {
        // A stop spoken during the paste + post-paste guard must abort the
        // submit (B2-13) before we re-verify identity and press Enter.
        if should_cancel() {
            return Err("stopped".into());
        }
        // Re-check identity between the paste and the Enter: a focus change in
        // that window must not turn the paste into a submit somewhere else.
        if !crate::focus::verify_foreground_target(target.hwnd, target.pid) {
            return Err("Target window changed before send — not pressing Enter".into());
        }
        // paste_text already held the post-paste guard, so the text has landed
        // by the time Enter fires.
        let mut enigo = Enigo::new(&Settings::default())
            .map_err(|e| format!("keystroke engine init failed: {e}"))?;
        enigo
            .key(Key::Return, Direction::Click)
            .map_err(|e| format!("Send (Enter) failed: {e}"))?;
    }
    Ok(())
}

/// Open a web/Google search in the user's DEFAULT browser.
#[cfg(windows)]
pub fn run_web_search(query: &str) -> Result<(), String> {
    let url = if query.trim().is_empty() {
        "https://www.google.com".to_string()
    } else {
        format!(
            "https://www.google.com/search?q={}",
            percent_encode(query.trim())
        )
    };
    open_in_default_browser(&url)
}

#[cfg(not(windows))]
pub fn run_web_search(_query: &str) -> Result<(), String> {
    Err("Web search is only supported on Windows".into())
}

/// Normalize + validate a spoken/LLM URL before opening it.  Prefixes `https://`
/// when no scheme is present, then parses with the `url` crate and REFUSES
/// anything that isn't a plain http/https URL with a host and no embedded
/// credentials.  Userinfo (`https://trusted.com@evil.example`) is the key attack:
/// it navigates to `evil.example` while reading as `trusted.com`, defeating URL
/// grounding.  Pure + platform-independent so it can be unit tested.
pub(crate) fn validate_open_url(target: &str) -> Result<String, String> {
    let t = target.trim();
    if t.is_empty() {
        return Err("No URL to open".into());
    }
    let normalized = if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    let parsed =
        url::Url::parse(&normalized).map_err(|_| "Not a valid URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Refusing to open a non-web URL".into());
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Refusing to open a URL with embedded credentials".into());
    }
    match parsed.host_str() {
        Some(host) if !host.is_empty() => {}
        _ => return Err("URL has no host".into()),
    }
    Ok(normalized)
}

/// Open a URL / website in the user's DEFAULT browser.
#[cfg(windows)]
pub fn run_open_url(target: &str) -> Result<(), String> {
    let url = validate_open_url(target)?;
    open_in_default_browser(&url)
}

#[cfg(not(windows))]
pub fn run_open_url(target: &str) -> Result<(), String> {
    // Validate on every platform (keeps the checks/tests honest) but only
    // Windows can actually launch the browser.
    let _ = validate_open_url(target)?;
    Err("Opening URLs is only supported on Windows".into())
}

/// Launch a URL via the shell so it opens in whatever the user set as their
/// DEFAULT browser (Arc, Chrome, …) — `explorer.exe <url>` routes through the
/// OS default handler rather than a hard-coded browser.
#[cfg(windows)]
fn open_in_default_browser(url: &str) -> Result<(), String> {
    std::process::Command::new("explorer.exe")
        .arg(url)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Failed to open browser: {e}"))
}

/// Minimal percent-encoding for a search query (RFC 3986 unreserved kept,
/// space → '+').
#[cfg(windows)]
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::validate_open_url;

    #[test]
    fn accepts_plain_domain_and_prefixes_https() {
        assert_eq!(validate_open_url("github.com").unwrap(), "https://github.com");
        assert_eq!(
            validate_open_url("https://youtube.com").unwrap(),
            "https://youtube.com"
        );
    }

    #[test]
    fn rejects_embedded_credentials() {
        // The M1 attack: reads as github.com, navigates to evil.example.
        assert!(validate_open_url("github.com@evil.example").is_err());
        assert!(validate_open_url("https://github.com@evil.example").is_err());
        assert!(validate_open_url("http://user:pass@evil.example").is_err());
    }

    #[test]
    fn rejects_non_web_and_malformed() {
        assert!(validate_open_url("javascript:alert(1)").is_err());
        assert!(validate_open_url("").is_err());
        assert!(validate_open_url("   ").is_err());
    }
}
