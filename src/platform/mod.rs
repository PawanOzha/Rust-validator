// Platform-specific utilities for process and window information
// Each platform provides process name and window title extraction

// Platform-specific modules
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

// Common trait for platform utilities
pub trait PlatformUtils {
    /// Get process name from process ID
    fn get_process_name(pid: u32) -> Result<String, Box<dyn std::error::Error>>;

    /// Get window title from process ID
    fn get_window_title(pid: u32) -> Result<String, Box<dyn std::error::Error>>;
}
