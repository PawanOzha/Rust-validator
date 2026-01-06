// macOS platform utilities for process and window information

use super::PlatformUtils;
use std::process::Command;

// Implement PlatformUtils trait for macOS
impl PlatformUtils for () {
    fn get_process_name(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_process_name_impl(pid)
    }

    fn get_window_title(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_window_title_impl(pid)
    }
}

/// Get process name from process ID using ps command
fn get_process_name_impl(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    let output = Command::new("ps")
        .args(&["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map_err(|e| format!("Failed to execute ps: {}", e))?;

    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        if !name.is_empty() {
            // Extract just the filename if it's a full path
            if let Some(filename) = name.split('/').last() {
                return Ok(filename.to_string());
            }
            return Ok(name);
        }
    }

    Err(format!("Process {} not found", pid).into())
}

/// Get window title for a process using AppleScript
/// This requires Accessibility permissions on macOS
fn get_window_title_impl(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Method 1: Try to get window title via AppleScript
    // This requires Accessibility permissions

    // First, get the process name to identify the app
    let process_name = get_process_name_impl(pid)?;

    // Try using osascript to get window title
    // Note: This may fail if Accessibility permissions are not granted
    let script = format!(
        r#"
        tell application "System Events"
            try
                set appName to name of first process whose unix id is {}
                tell process appName
                    try
                        set windowTitle to name of front window
                        return windowTitle
                    on error
                        return appName
                    end try
                end tell
            on error
                return "{}"
            end try
        end tell
        "#,
        pid, process_name
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let title = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_string();

            if !title.is_empty() && title != "missing value" {
                return Ok(title);
            }
        }
        _ => {}
    }

    // Fallback: Return process name if window title cannot be obtained
    Ok(process_name)
}

// Public convenience functions
pub fn get_process_name(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_process_name_impl(pid)
}

pub fn get_window_title(pid: u32) -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_window_title_impl(pid)
}
