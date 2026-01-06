// Windows platform utilities for process and window information

use super::PlatformUtils;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Threading::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use std::sync::Mutex;

// Implement PlatformUtils trait for Windows
impl PlatformUtils for () {
    fn get_process_name(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
        unsafe {
            get_process_name_impl(pid).map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
        }
    }

    fn get_window_title(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
        unsafe {
            Ok(get_window_title_impl(pid))
        }
    }
}

/// Get process name from process ID
unsafe fn get_process_name_impl(process_id: u32) -> Result<String> {
    let process_handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id)?;

    let mut buffer = vec![0u16; 260]; // MAX_PATH
    let mut size = buffer.len() as u32;

    let result = QueryFullProcessImageNameW(
        process_handle,
        PROCESS_NAME_FORMAT(0),
        PWSTR(buffer.as_mut_ptr()),
        &mut size,
    );

    if result.is_ok() {
        let _ = CloseHandle(process_handle);
        let path = String::from_utf16_lossy(&buffer[..size as usize]);

        // Extract just the filename from the path
        if let Some(name) = path.split('\\').last() {
            return Ok(name.to_string());
        }
        return Ok(path);
    }

    let _ = CloseHandle(process_handle);
    Err(Error::from_win32())
}

/// Get window title for a given process ID
/// For multi-process apps like browsers, finds any window from the same executable
unsafe fn get_window_title_impl(target_pid: u32) -> String {
    // Store found window title in a static mutex
    static WINDOW_TITLE: Mutex<Option<String>> = Mutex::new(None);
    static PROCESS_NAME: Mutex<Option<String>> = Mutex::new(None);

    // Reset state
    *WINDOW_TITLE.lock().unwrap() = None;

    // Get the process name for fallback searching
    let _target_process_name = if let Ok(name) = get_process_name_impl(target_pid) {
        *PROCESS_NAME.lock().unwrap() = Some(name.clone());
        name
    } else {
        *PROCESS_NAME.lock().unwrap() = None;
        String::new()
    };

    // Callback function for EnumWindows
    unsafe extern "system" fn enum_window_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let target_pid = lparam.0 as u32;
        let mut window_pid: u32 = 0;

        GetWindowThreadProcessId(hwnd, Some(&mut window_pid as *mut u32));

        // Check if window is visible and has text
        if IsWindowVisible(hwnd).as_bool() {
            let mut buffer = vec![0u16; 512];
            let length = GetWindowTextW(hwnd, &mut buffer);

            if length > 0 {
                let title = String::from_utf16_lossy(&buffer[..length as usize]);
                if !title.trim().is_empty() {
                    // Priority 1: Exact PID match
                    if window_pid == target_pid {
                        *WINDOW_TITLE.lock().unwrap() = Some(title);
                        return BOOL(0); // Stop enumeration
                    }

                    // Priority 2: Same process name (for multi-process apps like browsers)
                    if let Some(target_name) = PROCESS_NAME.lock().unwrap().as_ref() {
                        if let Ok(window_process_name) = get_process_name_impl(window_pid) {
                            if &window_process_name == target_name {
                                // Only save if we don't have a title yet
                                if WINDOW_TITLE.lock().unwrap().is_none() {
                                    *WINDOW_TITLE.lock().unwrap() = Some(title);
                                }
                            }
                        }
                    }
                }
            }
        }

        BOOL(1) // Continue enumeration
    }

    // Enumerate all top-level windows
    let _ = EnumWindows(Some(enum_window_callback), LPARAM(target_pid as isize));

    // Return the found window title or empty string
    WINDOW_TITLE.lock().unwrap().clone().unwrap_or_default()
}

// Public convenience functions
pub fn get_process_name(pid: u32) -> Result<String> {
    unsafe { get_process_name_impl(pid) }
}

pub fn get_window_title(pid: u32) -> String {
    unsafe { get_window_title_impl(pid) }
}
