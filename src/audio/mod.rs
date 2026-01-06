// Platform-specific audio backends
// Each platform implements the AudioBackend trait to provide audio monitoring

// Platform-specific modules
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "macos")]
pub mod macos;

// Re-export platform-specific implementation as 'platform'
#[cfg(target_os = "windows")]
pub use windows as platform;

#[cfg(target_os = "linux")]
pub use linux as platform;

#[cfg(target_os = "macos")]
pub use macos as platform;

// Shared data structures (platform-agnostic)

/// Audio device information (volume and mute status)
#[derive(Debug, Clone)]
pub struct AudioInfo {
    pub volume: f32,      // 0.0 - 100.0 percentage
    pub is_muted: bool,
}

/// Information about an application's audio session
#[derive(Debug, Clone)]
pub struct AudioAppSession {
    pub name: String,         // Process name (e.g., "chrome.exe")
    pub volume: f32,          // Per-app volume 0.0-100.0
    pub is_active: bool,      // Whether session is currently active
    pub peak_level: f32,      // Current audio level 0.0-1.0
    pub process_id: u32,      // Process ID
    pub window_title: String, // Window title of the application
}

// Platform audio backend trait
// All platforms must implement these functions
pub trait AudioBackend {
    /// Get microphone volume and mute status
    fn get_microphone_volume_and_mute() -> Result<AudioInfo, Box<dyn std::error::Error>>;

    /// Get name of default microphone device
    fn get_microphone_device_name() -> Result<String, Box<dyn std::error::Error>>;

    /// Get list of applications currently using the microphone
    fn get_apps_using_microphone() -> Result<Vec<String>, Box<dyn std::error::Error>>;

    /// Get audio output (speakers/headphones) volume and mute status
    fn get_audio_output_volume_and_mute() -> Result<AudioInfo, Box<dyn std::error::Error>>;

    /// Get name of default audio output device
    fn get_audio_output_device_name() -> Result<String, Box<dyn std::error::Error>>;

    /// Get current audio output peak level (0.0 to 1.0)
    fn get_audio_output_peak_level() -> Result<f32, Box<dyn std::error::Error>>;

    /// Get list of applications currently playing audio
    fn get_apps_playing_audio() -> Result<Vec<AudioAppSession>, Box<dyn std::error::Error>>;
}
