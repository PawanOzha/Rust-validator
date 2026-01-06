// Linux platform utilities for process and window information

use super::PlatformUtils;
use procfs::process::Process;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_long, c_ulong};

// Implement PlatformUtils trait for Linux
impl PlatformUtils for () {
    fn get_process_name(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_process_name_impl(pid)
    }

    fn get_window_title(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_window_title_impl(pid)
    }
}

/// Get process name from /proc filesystem
fn get_process_name_impl(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let process = Process::new(pid as i32)
        .map_err(|e| format!("Failed to read process {}: {}", pid, e))?;

    let stat = process.stat()
        .map_err(|e| format!("Failed to read process stat: {}", e))?;

    Ok(stat.comm)
}

/// Get window title for a process using X11
/// Falls back to process name if X11 is not available or window not found
fn get_window_title_impl(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Try X11 window title first
    if let Ok(title) = get_window_title_x11(pid) {
        return Ok(title);
    }

    // Fallback to process name
    get_process_name_impl(pid)
}

/// Get window title using X11
#[cfg(feature = "x11")]
fn get_window_title_x11(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    use x11::xlib::*;
    use std::ptr;

    unsafe {
        // Open display
        let display = XOpenDisplay(ptr::null());
        if display.is_null() {
            return Err("Failed to open X11 display".into());
        }

        let root = XDefaultRootWindow(display);

        // Get the _NET_CLIENT_LIST atom
        let net_client_list = XInternAtom(
            display,
            b"_NET_CLIENT_LIST\0".as_ptr() as *const c_char,
            0,
        );

        let net_wm_pid = XInternAtom(
            display,
            b"_NET_WM_PID\0".as_ptr() as *const c_char,
            0,
        );

        let net_wm_name = XInternAtom(
            display,
            b"_NET_WM_NAME\0".as_ptr() as *const c_char,
            0,
        );

        let utf8_string = XInternAtom(
            display,
            b"UTF8_STRING\0".as_ptr() as *const c_char,
            0,
        );

        // Get window list
        let mut actual_type_return = 0;
        let mut actual_format_return = 0;
        let mut nitems_return = 0;
        let mut bytes_after_return = 0;
        let mut prop_return: *mut u8 = ptr::null_mut();

        let status = XGetWindowProperty(
            display,
            root,
            net_client_list,
            0,
            1024,
            0,
            XA_WINDOW,
            &mut actual_type_return,
            &mut actual_format_return,
            &mut nitems_return,
            &mut bytes_after_return,
            &mut prop_return,
        );

        if status != 0 || prop_return.is_null() {
            XCloseDisplay(display);
            return Err("Failed to get window list".into());
        }

        let windows = std::slice::from_raw_parts(
            prop_return as *const c_ulong,
            nitems_return as usize,
        );

        // Search for window with matching PID
        for &window in windows {
            // Get window PID
            let mut window_pid_type = 0;
            let mut window_pid_format = 0;
            let mut window_pid_nitems = 0;
            let mut window_pid_bytes_after = 0;
            let mut window_pid_prop: *mut u8 = ptr::null_mut();

            let pid_status = XGetWindowProperty(
                display,
                window,
                net_wm_pid,
                0,
                1,
                0,
                XA_CARDINAL,
                &mut window_pid_type,
                &mut window_pid_format,
                &mut window_pid_nitems,
                &mut window_pid_bytes_after,
                &mut window_pid_prop,
            );

            if pid_status == 0 && !window_pid_prop.is_null() {
                let window_pid = *(window_pid_prop as *const c_ulong) as u32;

                if window_pid == pid {
                    // Found matching window, get title
                    let mut title_type = 0;
                    let mut title_format = 0;
                    let mut title_nitems = 0;
                    let mut title_bytes_after = 0;
                    let mut title_prop: *mut u8 = ptr::null_mut();

                    let title_status = XGetWindowProperty(
                        display,
                        window,
                        net_wm_name,
                        0,
                        1024,
                        0,
                        utf8_string,
                        &mut title_type,
                        &mut title_format,
                        &mut title_nitems,
                        &mut title_bytes_after,
                        &mut title_prop,
                    );

                    if title_status == 0 && !title_prop.is_null() {
                        let title_cstr = CStr::from_ptr(title_prop as *const c_char);
                        let title = title_cstr.to_string_lossy().to_string();

                        XFree(title_prop as *mut _);
                        XFree(window_pid_prop as *mut _);
                        XFree(prop_return as *mut _);
                        XCloseDisplay(display);

                        return Ok(title);
                    }

                    if !title_prop.is_null() {
                        XFree(title_prop as *mut _);
                    }
                }

                XFree(window_pid_prop as *mut _);
            }
        }

        XFree(prop_return as *mut _);
        XCloseDisplay(display);

        Err("Window not found for PID".into())
    }
}

/// Fallback when X11 is not available (Wayland or headless)
#[cfg(not(feature = "x11"))]
fn get_window_title_x11(_pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    Err("X11 support not compiled".into())
}

// Public convenience functions
pub fn get_process_name(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_process_name_impl(pid)
}

pub fn get_window_title(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_window_title_impl(pid)
}
