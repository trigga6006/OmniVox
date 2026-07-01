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
        KeyChord::Redo => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('y')),
        KeyChord::SelectAll => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('a')),
        KeyChord::Save => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('s')),
        KeyChord::NewTab => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('t')),
        KeyChord::CloseTab => chord(&mut enigo, &[PRIMARY_MOD], Key::Unicode('w')),
        KeyChord::Screenshot => screenshot(&mut enigo),
        KeyChord::ShowDesktop => show_desktop(&mut enigo),
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

    // VK_MEDIA_* / VK_VOLUME_* codes.
    let vk: u8 = match a {
        MediaAction::PlayPause => 0xB3,
        MediaAction::NextTrack => 0xB0,
        MediaAction::PrevTrack => 0xB1,
        MediaAction::Mute => 0xAD,
        MediaAction::VolumeUp => 0xAF,
        MediaAction::VolumeDown => 0xAE,
    };
    unsafe {
        keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY, 0);
        keybd_event(vk, 0, KEYEVENTF_EXTENDEDKEY | KEYEVENTF_KEYUP, 0);
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

/// Open a URL / website in the user's DEFAULT browser.
#[cfg(windows)]
pub fn run_open_url(target: &str) -> Result<(), String> {
    let t = target.trim();
    if t.is_empty() {
        return Err("No URL to open".into());
    }
    let url = if t.starts_with("http://") || t.starts_with("https://") {
        t.to_string()
    } else {
        format!("https://{t}")
    };
    open_in_default_browser(&url)
}

#[cfg(not(windows))]
pub fn run_open_url(_target: &str) -> Result<(), String> {
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
