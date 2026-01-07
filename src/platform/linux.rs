// Linux platform utilities for process and window information

use super::PlatformUtils;
use procfs::process::Process;
use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_long, c_ulong};
use std::process::Command;

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

/// Get window title for a process using X11, Wayland, or fallbacks
/// Tries multiple methods to ensure window titles are found
fn get_window_title_impl(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Method 1: Try X11 window title first
    if let Ok(title) = get_window_title_x11(pid) {
        if !title.is_empty() && title != "Window not found for PID" {
            return Ok(title);
        }
    }

    // Method 2: Try Wayland via /proc/pid/environ and desktop files
    if let Ok(title) = get_window_title_wayland(pid) {
        if !title.is_empty() {
            return Ok(title);
        }
    }

    // Method 3: Try wmctrl (works on both X11 and some Wayland compositors)
    if let Ok(title) = get_window_title_wmctrl(pid) {
        if !title.is_empty() {
            return Ok(title);
        }
    }

    // Method 4: Try reading from /proc/pid/cmdline for app identification
    if let Ok(title) = get_title_from_cmdline(pid) {
        if !title.is_empty() {
            return Ok(title);
        }
    }

    // Fallback to process name
    get_process_name_impl(pid)
}

/// Get window title on Wayland using proc and environment
fn get_window_title_wayland(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    use std::fs;

    // Read environment variables to detect the display protocol
    let environ_path = format!("/proc/{}/environ", pid);
    if let Ok(environ_data) = fs::read(&environ_path) {
        let environ_str = String::from_utf8_lossy(&environ_data);

        // Check if running under Wayland
        if environ_str.contains("WAYLAND_DISPLAY") {
            // Try to extract GDK_BACKEND or other app identifiers
            for entry in environ_str.split('\0') {
                if entry.starts_with("GDK_BACKEND") || entry.starts_with("WAYLAND_DISPLAY") {
                    // Detected Wayland, now try to get app name via cmdline
                    return get_title_from_cmdline(pid);
                }
            }
        }
    }

    Err("Not running under Wayland or info unavailable".into())
}

/// Get window title using wmctrl command
fn get_window_title_wmctrl(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("wmctrl")
        .args(&["-l", "-p"])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            let wmctrl_str = String::from_utf8_lossy(&output.stdout);

            for line in wmctrl_str.lines() {
                let parts: Vec<&str> = line.split_whitespace().collect();
                // wmctrl format: window_id desktop pid machine window_title
                if parts.len() >= 5 {
                    if let Ok(window_pid) = parts[2].parse::<u32>() {
                        if window_pid == pid {
                            // Join remaining parts as window title
                            let title = parts[4..].join(" ");
                            return Ok(title);
                        }
                    }
                }
            }
        }
    }

    Err("wmctrl not available or window not found".into())
}

/// Extract meaningful title from command line arguments
fn get_title_from_cmdline(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    use std::fs;

    let cmdline_path = format!("/proc/{}/cmdline", pid);
    if let Ok(cmdline_data) = fs::read(&cmdline_path) {
        let cmdline = String::from_utf8_lossy(&cmdline_data);
        let args: Vec<&str> = cmdline.split('\0').filter(|s| !s.is_empty()).collect();

        if !args.is_empty() {
            // Look for recognizable patterns
            for arg in &args {
                // Check for URLs (meeting links)
                if arg.contains("meet.google.com") || arg.contains("teams.microsoft.com") || arg.contains("zoom.us") {
                    return Ok(format!("Meeting: {}", extract_domain(arg)));
                }

                // Check for app names
                if arg.contains("--app=") {
                    if let Some(app_name) = arg.split("--app=").nth(1) {
                        return Ok(app_name.to_string());
                    }
                }

                // Check for titles
                if arg.contains("--title=") || arg.contains("--name=") {
                    if let Some(title) = arg.split('=').nth(1) {
                        return Ok(title.to_string());
                    }
                }
            }

            // Return the executable name if nothing else found
            if let Some(exe) = args.first() {
                if let Some(basename) = exe.split('/').last() {
                    return Ok(basename.to_string());
                }
            }
        }
    }

    Err("Could not extract title from cmdline".into())
}

/// Extract domain from URL
fn extract_domain(url: &str) -> String {
    if let Some(domain_start) = url.find("://") {
        let after_protocol = &url[domain_start + 3..];
        if let Some(slash_pos) = after_protocol.find('/') {
            return after_protocol[..slash_pos].to_string();
        }
        return after_protocol.to_string();
    }

    url.to_string()
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
