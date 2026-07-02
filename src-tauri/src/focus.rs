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
    use windows_sys::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, GetForegroundWindow, GetWindowThreadProcessId, SetForegroundWindow,
    };

    let current = unsafe { GetForegroundWindow() };
    if !current.is_null() && current as isize == hwnd {
        return;
    }

    let target = hwnd as *mut std::ffi::c_void;

    unsafe {
        // Windows refuses SetForegroundWindow across processes unless the
        // calling thread shares an input queue with the thread that currently
        // owns the foreground. Temporarily attach the foreground thread and our
        // own thread to the target window's thread so the change is honored,
        // then detach. This is the standard AttachThreadInput technique.
        let fg_thread = if current.is_null() {
            0
        } else {
            GetWindowThreadProcessId(current, std::ptr::null_mut())
        };
        let target_thread = GetWindowThreadProcessId(target, std::ptr::null_mut());
        let our_thread = GetCurrentThreadId();

        // Skip any attach where the thread ids coincide (attaching a thread to
        // itself is invalid) or where a thread id could not be resolved.
        let attach_fg = target_thread != 0 && fg_thread != 0 && fg_thread != target_thread;
        let attach_ours =
            target_thread != 0 && our_thread != target_thread && our_thread != fg_thread;

        if attach_fg {
            AttachThreadInput(fg_thread, target_thread, 1);
        }
        if attach_ours {
            AttachThreadInput(our_thread, target_thread, 1);
        }

        SetForegroundWindow(target);
        BringWindowToTop(target);

        if attach_ours {
            AttachThreadInput(our_thread, target_thread, 0);
        }
        if attach_fg {
            AttachThreadInput(fg_thread, target_thread, 0);
        }
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
