/// A window plus the process that owned it when captured.  Carried on the
/// [`crate::state::CommandContext`] so every side-effecting Command-Mode
/// primitive can re-verify it is still acting on the SAME window (not a recycled
/// HWND, and not a window a concurrent focus change swapped in).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowTarget {
    pub hwnd: isize,
    pub pid: Option<u32>,
}

/// Pure identity decision, factored out of the Win32 calls so it can be unit
/// tested.  A side-effecting primitive may fire only when the window still
/// exists, is the current foreground, and (when we captured one) is owned by the
/// same process it was at capture time.
///
/// `pid_required` (true on Windows, where a foreground window's owning pid is
/// always readable) fails closed when no `expected_pid` was captured: a pid was
/// obtainable, so its absence means we never anchored identity and must refuse
/// rather than accept on the HWND alone (B2-6).
pub(crate) fn foreground_identity_ok(
    is_window: bool,
    foreground: isize,
    target: isize,
    fg_pid: Option<u32>,
    expected_pid: Option<u32>,
    pid_required: bool,
) -> bool {
    if !is_window {
        return false;
    }
    if foreground != target {
        return false;
    }
    match (expected_pid, fg_pid) {
        (Some(expected), Some(actual)) => expected == actual,
        // We had a PID to check but the live window has none — refuse.
        (Some(_), None) => false,
        // No captured PID.  On a platform where a pid is always capturable
        // (Windows), refuse — identity was never anchored.  Elsewhere,
        // foreground + IsWindow already matched.
        (None, _) => !pid_required,
    }
}

/// PID that currently owns `hwnd`, or `None` if it can't be resolved.
#[cfg(target_os = "windows")]
pub(crate) fn pid_for_hwnd(hwnd: isize) -> Option<u32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd as *mut core::ffi::c_void, &mut pid);
        if pid == 0 {
            None
        } else {
            Some(pid)
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn pid_for_hwnd(_hwnd: isize) -> Option<u32> {
    None
}

/// Verify `hwnd` still exists, is the current foreground window, and is owned by
/// `expected_pid` (when known).  The gate before any focus-dependent primitive
/// (paste / Enter / keystroke).  Windows-only; other platforms return `true`
/// (focus identity is not enforced there).
#[cfg(target_os = "windows")]
pub fn verify_foreground_target(hwnd: isize, expected_pid: Option<u32>) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow};
    unsafe {
        let is_window = IsWindow(hwnd as *mut core::ffi::c_void) != 0;
        let fg = GetForegroundWindow() as isize;
        let fg_pid = if fg != 0 { pid_for_hwnd(fg) } else { None };
        // pid_required = true: on Windows a foreground window's pid is always
        // readable, so a missing expected_pid means identity was never bound.
        foreground_identity_ok(is_window, fg, hwnd, fg_pid, expected_pid, true)
    }
}

#[cfg(not(target_os = "windows"))]
pub fn verify_foreground_target(_hwnd: isize, _expected_pid: Option<u32>) -> bool {
    true
}

/// Verify `hwnd` still exists and is owned by `expected_pid` (when known),
/// WITHOUT requiring it to be foreground — for actions on a specific window that
/// need not be focused (WM_CLOSE, minimize/restore, undo).  Guards against a
/// recycled/reused HWND (M8).
#[cfg(target_os = "windows")]
pub fn window_identity_ok(hwnd: isize, expected_pid: Option<u32>) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::IsWindow;
    unsafe {
        if IsWindow(hwnd as *mut core::ffi::c_void) == 0 {
            return false;
        }
    }
    match expected_pid {
        Some(expected) => pid_for_hwnd(hwnd) == Some(expected),
        // No captured pid to anchor against.  On Windows a pid is always
        // capturable, so its absence means identity was never bound — refuse
        // rather than act on a bare (recyclable) HWND (B2-6).
        None => false,
    }
}

#[cfg(not(target_os = "windows"))]
pub fn window_identity_ok(_hwnd: isize, _expected_pid: Option<u32>) -> bool {
    true
}

/// True when `hwnd` is a real, visible, titled top-level app window (not a tool
/// window / shell host).  Used by `settle_after_launch` to reject transient
/// notifications and helper windows when correlating a freshly launched app.
#[cfg(target_os = "windows")]
pub(crate) fn is_real_app_window(hwnd: isize) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, GetWindowTextLengthW, IsWindow, IsWindowVisible, GWL_EXSTYLE,
        WS_EX_TOOLWINDOW,
    };
    unsafe {
        let h = hwnd as *mut core::ffi::c_void;
        if IsWindow(h) == 0 || IsWindowVisible(h) == 0 {
            return false;
        }
        let ex = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        GetWindowTextLengthW(h) != 0
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn is_real_app_window(_hwnd: isize) -> bool {
    true
}

/// Snapshot the currently focused window so we can restore it before pasting.
/// Returns a platform-specific handle (HWND on Windows, pid on macOS).
#[cfg(target_os = "windows")]
pub(crate) fn capture_foreground_window() -> Option<isize> {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        None
    } else {
        Some(hwnd as isize)
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn capture_foreground_window() -> Option<isize> {
    // Use NSWorkspace via the objc runtime to get the frontmost app's PID.
    unsafe {
        let cls = objc::runtime::Class::get("NSWorkspace")?;
        let workspace: *mut objc::runtime::Object = objc::msg_send![cls, sharedWorkspace];
        let app: *mut objc::runtime::Object = objc::msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return None;
        }
        let pid: i32 = objc::msg_send![app, processIdentifier];
        if pid > 0 {
            Some(pid as isize)
        } else {
            None
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn capture_foreground_window() -> Option<isize> {
    None
}

/// Like [`capture_foreground_window`], but never returns one of OmniVox's own
/// windows.  Command Mode fires keystrokes / launches at the user's real app, so
/// if OmniVox itself is the foreground window (the user just clicked the app or
/// its overlay pill), walk down the Z-order to the topmost normal window behind
/// us — the app they were last using — and target that instead.  Without this,
/// "copy", "minimize", media keys, etc. bounce off our own UI whenever OmniVox
/// was the last-focused window.
#[cfg(target_os = "windows")]
pub(crate) fn capture_command_target_window() -> Option<isize> {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindow, GetWindowLongPtrW, GetWindowTextLengthW,
        GetWindowThreadProcessId, IsWindowVisible, GWL_EXSTYLE, GW_HWNDNEXT, WS_EX_TOOLWINDOW,
    };

    unsafe {
        let own_pid = GetCurrentProcessId();
        let pid_of = |hwnd: *mut core::ffi::c_void| -> u32 {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, &mut pid);
            pid
        };

        let fg = GetForegroundWindow();
        if fg.is_null() {
            return None;
        }
        // Already a real, non-OmniVox app in front — target it directly.
        if pid_of(fg) != own_pid {
            return Some(fg as isize);
        }

        // Foreground is one of ours — find the first eligible window behind it:
        // visible, owned by another process, not a tool window, and titled
        // (skips shell/host/zero-size helper windows).
        let mut hwnd = fg;
        loop {
            hwnd = GetWindow(hwnd, GW_HWNDNEXT);
            if hwnd.is_null() {
                return None;
            }
            if IsWindowVisible(hwnd) == 0 || pid_of(hwnd) == own_pid {
                continue;
            }
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex & WS_EX_TOOLWINDOW != 0 || GetWindowTextLengthW(hwnd) == 0 {
                continue;
            }
            return Some(hwnd as isize);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn capture_command_target_window() -> Option<isize> {
    capture_foreground_window()
}

/// True when `hwnd` belongs to OmniVox's own process (the main window or the
/// overlay pill).  Dictation aimed at one of our own WebView2 controls can't
/// rely on a synthetic Ctrl+V — the paste doesn't reliably land in the
/// focused web input — so the pipeline routes that case through the frontend
/// (DOM caret insertion) instead.
#[cfg(target_os = "windows")]
pub(crate) fn hwnd_is_own_process(hwnd: isize) -> bool {
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd as *mut core::ffi::c_void, &mut pid);
        pid != 0 && pid == GetCurrentProcessId()
    }
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn hwnd_is_own_process(_hwnd: isize) -> bool {
    false
}

/// Extract the process executable name (for example, "Code.exe") from a
/// platform foreground-window handle.
#[cfg(target_os = "windows")]
pub(crate) fn get_process_name_from_hwnd(hwnd: isize) -> Option<String> {
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

#[cfg(target_os = "macos")]
pub(crate) fn get_process_name_from_hwnd(pid: isize) -> Option<String> {
    let mut buf = [0u8; 4096];
    let ret =
        unsafe { libc::proc_pidpath(pid as i32, buf.as_mut_ptr() as *mut _, buf.len() as u32) };
    if ret <= 0 {
        return None;
    }
    let path = std::str::from_utf8(&buf[..ret as usize]).ok()?;
    path.rsplit('/').next().map(|s| s.to_string())
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn get_process_name_from_hwnd(_hwnd: isize) -> Option<String> {
    None
}

/// Bring `hwnd` to the foreground.  Returns whether it IS the foreground window
/// (owned by `expected_pid`, when known) afterwards — the caller must not treat
/// a failed restore as success (H2: the `SetForegroundWindow` result was
/// previously ignored, so a keystroke could fire into whatever happened to be
/// focused).
///
/// The full identity (foreground HWND **and** owning pid) is verified BEFORE the
/// synthetic deselection arrows — a HWND recycled to another process must be
/// rejected before any key injection, not merely by an HWND-only check (B2-15).
#[cfg(target_os = "windows")]
pub(crate) fn restore_foreground_window(hwnd: isize, expected_pid: Option<u32>) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    let current = unsafe { GetForegroundWindow() };
    if !current.is_null() && current as isize == hwnd {
        // Already foreground — no focus switch, so no select-all to collapse.
        // Still gate "success" on the full identity (B2-6/B2-15).
        return verify_foreground_target(hwnd, expected_pid);
    }

    unsafe {
        SetForegroundWindow(hwnd as *mut std::ffi::c_void);
    }
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Confirm `hwnd` took the foreground AND is still owned by the expected
    // process BEFORE sending the synthetic deselection arrows — enigo injects
    // them into whatever window is focused now, so firing them on a failed
    // restore (or a recycled HWND) would nudge the caret of a stranger
    // window (B2-6/B2-15).
    if !verify_foreground_target(hwnd, expected_pid) {
        return false;
    }
    deselect_after_focus_restore(hwnd, expected_pid);
    true
}

/// After a focus restoration, some controls select all their text. If a text
/// caret is active, send Right/Left arrow keys to collapse the selection
/// without net cursor movement.
#[cfg(target_os = "windows")]
fn deselect_after_focus_restore(hwnd: isize, expected_pid: Option<u32>) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
    };

    unsafe {
        let thread_id =
            GetWindowThreadProcessId(hwnd as *mut std::ffi::c_void, std::ptr::null_mut());
        if thread_id == 0 {
            return;
        }

        let mut gui: GUITHREADINFO = std::mem::zeroed();
        gui.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;

        if GetGUIThreadInfo(thread_id, &mut gui) == 0 {
            return;
        }

        if gui.hwndCaret.is_null() {
            return;
        }
    }

    // Re-verify identity immediately before the enigo injection — the 50 ms
    // settle and the GUI-info probe above are a window in which focus could
    // have changed; the arrows must only land in the still-verified target
    // (B2-15).
    if !verify_foreground_target(hwnd, expected_pid) {
        return;
    }

    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.key(Key::RightArrow, Direction::Click);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = enigo.key(Key::LeftArrow, Direction::Click);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn restore_foreground_window(pid: isize, _expected_pid: Option<u32>) -> bool {
    let mut activated = false;
    unsafe {
        let cls =
            objc::runtime::Class::get("NSRunningApplication").expect("NSRunningApplication class");
        let app: *mut objc::runtime::Object = objc::msg_send![
            cls,
            runningApplicationWithProcessIdentifier: pid as i32
        ];
        if !app.is_null() {
            let _: objc::runtime::BOOL = objc::msg_send![
                app,
                activateWithOptions: 0x02u64
            ];
            activated = true;
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    activated
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn restore_foreground_window(_hwnd: isize, _expected_pid: Option<u32>) -> bool {
    false
}

/// Public wrapper for commands that need to restore focus, such as the
/// Structured panel's Paste button.  Returns whether the target is foreground
/// (and owned by `expected_pid`, when known — `None` fails closed on the
/// deselection arrows, B2-15).
pub fn restore_foreground_window_public(hwnd: isize, expected_pid: Option<u32>) -> bool {
    restore_foreground_window(hwnd, expected_pid)
}

#[cfg(test)]
mod tests {
    use super::foreground_identity_ok;

    #[test]
    fn identity_matches_when_foreground_and_pid_agree() {
        assert!(foreground_identity_ok(true, 10, 10, Some(5), Some(5), true));
    }

    #[test]
    fn identity_rejected_when_foreground_differs() {
        // Window still exists + PID would match, but a different window is
        // foreground now — refuse (the H1/H2 redirect).
        assert!(!foreground_identity_ok(true, 20, 10, Some(5), Some(5), true));
    }

    #[test]
    fn identity_rejected_on_pid_mismatch() {
        // Same HWND value is foreground, but a different process owns it now —
        // a recycled handle (M8). Refuse.
        assert!(!foreground_identity_ok(true, 10, 10, Some(9), Some(5), true));
    }

    #[test]
    fn identity_rejected_when_window_gone() {
        assert!(!foreground_identity_ok(false, 10, 10, Some(5), Some(5), true));
    }

    #[test]
    fn identity_rejected_when_pid_required_but_absent() {
        // pid_required (Windows): no captured pid means identity was never
        // anchored, so a bare foreground+IsWindow match must NOT pass (B2-6).
        assert!(!foreground_identity_ok(true, 10, 10, None, Some(5), true));
        assert!(!foreground_identity_ok(true, 10, 10, None, None, true));
    }

    #[test]
    fn identity_ok_without_expected_pid_when_not_required() {
        // Non-Windows: no pid to correlate — IsWindow + foreground is enough.
        assert!(foreground_identity_ok(true, 10, 10, None, None, false));
    }
}
