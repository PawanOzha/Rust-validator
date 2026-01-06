use serde::{Deserialize, Serialize};
use std::error::Error;

/// Complete audio output status report
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioOutputReport {
    pub timestamp: String,
    pub output: AudioOutputInfo,
    pub active_apps: Vec<AudioAppInfo>,
    pub errors: Vec<String>,
}

/// Audio output device information
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioOutputInfo {
    pub default_device: String,
    pub is_muted: bool,
    pub volume_level: f32,
    pub peak_level: f32,
    pub is_active: bool,
}

/// Information about an app playing audio
#[derive(Debug, Serialize, Deserialize)]
pub struct AudioAppInfo {
    pub name: String,
    pub volume: f32,
    pub is_playing: bool,
    pub peak_level: f32,
    pub process_id: u32,
    pub window_title: String,
}

/// Main audio output monitor struct
pub struct AudioOutputMonitor {
    errors: Vec<String>,
}

impl AudioOutputMonitor {
    /// Create a new audio output monitor instance
    pub fn new() -> std::result::Result<Self, Box<dyn Error>> {
        Ok(AudioOutputMonitor {
            errors: Vec::new(),
        })
    }

    /// Build complete JSON status report
    pub fn build_status_report(&mut self) -> std::result::Result<AudioOutputReport, Box<dyn Error>> {
        #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
        {
            let output_info = self.get_output_info();
            let active_apps = self.get_active_apps();

            Ok(AudioOutputReport {
                timestamp: chrono::Utc::now().to_rfc3339(),
                output: output_info,
                active_apps,
                errors: self.errors.clone(),
            })
        }

        #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
        {
            Err("Audio output monitoring is only supported on Windows, Linux, and macOS".into())
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    fn get_output_info(&mut self) -> AudioOutputInfo {
        use crate::audio::platform;

        // Get default audio output device info
        let (device_name, volume_level, is_muted) = match platform::get_audio_output_volume_and_mute() {
            Ok(audio_info) => {
                let name = platform::get_audio_output_device_name()
                    .unwrap_or_else(|_| "Default Speakers".to_string());
                (name, audio_info.volume, audio_info.is_muted)
            }
            Err(e) => {
                self.errors.push(format!("Audio output error: {}", e));
                ("Default Speakers".to_string(), 50.0, false)
            }
        };

        // Get peak level (current audio level)
        let peak_level = match platform::get_audio_output_peak_level() {
            Ok(level) => level,
            Err(e) => {
                self.errors.push(format!("Failed to get peak level: {}", e));
                0.0
            }
        };

        let is_active = peak_level > 0.01; // Audio is playing if peak > 1%

        AudioOutputInfo {
            default_device: device_name,
            is_muted,
            volume_level,
            peak_level,
            is_active,
        }
    }

    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    fn get_active_apps(&mut self) -> Vec<AudioAppInfo> {
        use crate::audio::platform;

        match platform::get_apps_playing_audio() {
            Ok(apps) => apps.into_iter().map(|app| {
                AudioAppInfo {
                    name: app.name,
                    volume: app.volume,
                    is_playing: app.is_active,
                    peak_level: app.peak_level,
                    process_id: app.process_id,
                    window_title: app.window_title,
                }
            }).collect(),
            Err(e) => {
                self.errors.push(format!("Failed to get playing apps: {}", e));
                Vec::new()
            }
        }
    }
}
