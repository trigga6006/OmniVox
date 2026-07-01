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

#[cfg(target_os = "windows")]
pub(crate) fn restore_foreground_window(hwnd: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, SetForegroundWindow};

    let current = unsafe { GetForegroundWindow() };
    if !current.is_null() && current as isize == hwnd {
        return;
    }

    unsafe {
        SetForegroundWindow(hwnd as *mut std::ffi::c_void);
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
    deselect_after_focus_restore(hwnd);
}

/// After a focus restoration, some controls select all their text. If a text
/// caret is active, send Right/Left arrow keys to collapse the selection
/// without net cursor movement.
#[cfg(target_os = "windows")]
fn deselect_after_focus_restore(hwnd: isize) {
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

    use enigo::{Direction, Enigo, Key, Keyboard, Settings};
    if let Ok(mut enigo) = Enigo::new(&Settings::default()) {
        let _ = enigo.key(Key::RightArrow, Direction::Click);
        std::thread::sleep(std::time::Duration::from_millis(2));
        let _ = enigo.key(Key::LeftArrow, Direction::Click);
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn restore_foreground_window(pid: isize) {
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
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub(crate) fn restore_foreground_window(_hwnd: isize) {}

/// Public wrapper for commands that need to restore focus, such as the
/// Structured panel's Paste button.
pub fn restore_foreground_window_public(hwnd: isize) {
    restore_foreground_window(hwnd);
}
