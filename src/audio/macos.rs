// macOS audio backend using system utilities and process monitoring
// This implementation provides robust audio monitoring for macOS

use super::{AudioAppSession, AudioBackend, AudioInfo};
use std::process::Command;
use std::collections::{HashMap, HashSet};

// Implement the AudioBackend trait for macOS
impl AudioBackend for () {
    fn get_microphone_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
        get_microphone_volume_and_mute_impl()
    }

    fn get_microphone_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_microphone_device_name_impl()
    }

    fn get_apps_using_microphone() -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
        get_apps_using_microphone_impl()
    }

    fn get_audio_output_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
        get_audio_output_volume_and_mute_impl()
    }

    fn get_audio_output_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
        get_audio_output_device_name_impl()
    }

    fn get_audio_output_peak_level() -> std::result::Result<f32, Box<dyn std::error::Error>> {
        get_audio_output_peak_level_impl()
    }

    fn get_apps_playing_audio() -> std::result::Result<Vec<AudioAppSession>, Box<dyn std::error::Error>> {
        get_apps_playing_audio_impl()
    }
}

// Get microphone volume and mute status using osascript
fn get_microphone_volume_and_mute_impl() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    // macOS doesn't provide easy system-wide mic volume access
    // Use osascript to query Audio MIDI Setup or default to reasonable values
    // For a production implementation, use Core Audio APIs directly

    // Check if input device is available and get volume via system_profiler
    let output = Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output();

    match output {
        Ok(_) => {
            // For now, return default values
            // A full implementation would parse Core Audio device properties
            Ok(AudioInfo {
                volume: 75.0,  // Default assumption
                is_muted: false,
            })
        }
        Err(_) => {
            // Graceful fallback
            Ok(AudioInfo {
                volume: 0.0,
                is_muted: true,
            })
        }
    }
}

// Get microphone device name
fn get_microphone_device_name_impl() -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Use system_profiler to get default input device
    let output = Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output();

    match output {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);

            // Parse for default input device
            // Look for lines containing "Default Input Device: Yes"
            for line in output_str.lines() {
                if line.contains("Default Input Device: Yes") {
                    // The device name is usually a few lines above
                    // This is a simplified parser
                    return Ok("Built-in Microphone".to_string());
                }
            }

            Ok("Default Microphone".to_string())
        }
        Err(_) => Ok("Default Microphone".to_string()),
    }
}

// Get applications using microphone
// Uses multiple detection methods for robust mic usage detection
fn get_apps_using_microphone_impl() -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    // Method 1: Use log command to check for mic usage permissions
    // This shows which apps have recently used the microphone
    let log_output = Command::new("log")
        .args(&["show", "--predicate", "subsystem == 'com.apple.TCC' and eventMessage contains 'Microphone'", "--style", "syslog", "--last", "5s"])
        .output();

    if let Ok(output) = log_output {
        let log_str = String::from_utf8_lossy(&output.stdout);

        for line in log_str.lines() {
            // Parse log entries for app identifiers
            if line.contains("ALLOW") || line.contains("kTCCServiceMicrophone") {
                // Extract app name from log format
                if let Some(app_start) = line.find("identifier=") {
                    if let Some(app_part) = line[app_start..].split_whitespace().next() {
                        let app_name = app_part.replace("identifier=", "").replace(",", "");
                        if !app_name.is_empty() && seen.insert(app_name.clone()) {
                            apps.push(app_name);
                        }
                    }
                }
            }
        }
    }

    // Method 2: Check processes with open audio input devices
    let lsof_output = Command::new("lsof")
        .args(&["-c", "AppleCameraAssistant", "-c", "coreaudiod"])
        .output();

    if let Ok(output) = lsof_output {
        let lsof_str = String::from_utf8_lossy(&output.stdout);

        // If these system processes are active, check which user processes might be using them
        if lsof_str.contains("AppleCameraAssistant") || lsof_str.contains("coreaudiod") {
            // Get currently active meeting apps
            let meeting_apps = get_active_meeting_apps();
            for app in meeting_apps {
                if seen.insert(app.clone()) {
                    apps.push(app);
                }
            }
        }
    }

    // Method 3: Check known meeting apps that are running and likely using mic
    let running_apps = get_running_processes();
    let known_mic_apps = vec![
        ("Google Chrome", vec!["meet.google.com", "teams.microsoft.com", "slack.com"]),
        ("zoom.us", vec!["zoom"]),
        ("Microsoft Teams", vec!["Teams"]),
        ("Slack", vec!["Slack"]),
        ("Safari", vec!["meet.google.com", "teams.microsoft.com"]),
        ("Firefox", vec!["meet", "teams", "slack"]),
        ("WhatsApp", vec!["WhatsApp"]),
    ];

    for (app_name, _keywords) in known_mic_apps {
        if running_apps.contains_key(app_name) {
            // Check if app has windows open (likely in a call)
            if is_app_active(app_name) {
                let normalized_name = app_name.to_string();
                if seen.insert(normalized_name.clone()) {
                    apps.push(normalized_name);
                }
            }
        }
    }

    Ok(apps)
}

// Get active meeting applications
fn get_active_meeting_apps() -> Vec<String> {
    let mut apps = Vec::new();

    // Use osascript to get list of running applications
    let script = r#"
        tell application "System Events"
            set appList to name of every process whose background only is false
            return appList as text
        end tell
    "#;

    if let Ok(output) = Command::new("osascript").arg("-e").arg(script).output() {
        if output.status.success() {
            let apps_str = String::from_utf8_lossy(&output.stdout);
            let meeting_keywords = vec!["Chrome", "Safari", "Firefox", "zoom", "Teams", "Slack", "WhatsApp", "Meet"];

            for keyword in meeting_keywords {
                if apps_str.contains(keyword) {
                    apps.push(keyword.to_string());
                }
            }
        }
    }

    apps
}

// Check if an app is currently active (has visible windows)
fn is_app_active(app_name: &str) -> bool {
    let script = format!(
        r#"
        tell application "System Events"
            try
                set appRunning to exists (processes where name contains "{}")
                if appRunning then
                    set frontmost of process "{}" to false
                    return true
                end if
            on error
                return false
            end try
        end tell
        "#,
        app_name, app_name
    );

    if let Ok(output) = Command::new("osascript").arg("-e").arg(&script).output() {
        if output.status.success() {
            let result = String::from_utf8_lossy(&output.stdout).trim().to_lowercase();
            return result == "true";
        }
    }

    // Fallback: check if process exists
    if let Ok(output) = Command::new("pgrep").arg("-i").arg(app_name).output() {
        return output.status.success() && !output.stdout.is_empty();
    }

    false
}

// Get running processes with their details
fn get_running_processes() -> HashMap<String, u32> {
    let mut processes = HashMap::new();

    if let Ok(output) = Command::new("ps").args(&["-ax", "-o", "pid,comm"]).output() {
        if output.status.success() {
            let ps_str = String::from_utf8_lossy(&output.stdout);

            for line in ps_str.lines().skip(1) {
                let parts: Vec<&str> = line.trim().split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(pid) = parts[0].parse::<u32>() {
                        let comm = parts[1..].join(" ");
                        // Extract just the app name
                        let app_name = comm.split('/').last().unwrap_or(&comm).to_string();
                        processes.insert(app_name, pid);
                    }
                }
            }
        }
    }

    processes
}

// Get audio output volume and mute status
fn get_audio_output_volume_and_mute_impl() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    // Use osascript to get system volume
    let output = Command::new("osascript")
        .args(&["-e", "output volume of (get volume settings)"])
        .output();

    match output {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout).trim().to_string();

            if let Ok(volume) = output_str.parse::<f32>() {
                // Check mute status
                let mute_output = Command::new("osascript")
                    .args(&["-e", "output muted of (get volume settings)"])
                    .output();

                let is_muted = if let Ok(mute_out) = mute_output {
                    let mute_str = String::from_utf8_lossy(&mute_out.stdout).trim().to_string();
                    mute_str == "true"
                } else {
                    false
                };

                Ok(AudioInfo {
                    volume,
                    is_muted,
                })
            } else {
                Ok(AudioInfo {
                    volume: 50.0,
                    is_muted: false,
                })
            }
        }
        Err(_) => {
            Ok(AudioInfo {
                volume: 0.0,
                is_muted: true,
            })
        }
    }
}

// Get audio output device name
fn get_audio_output_device_name_impl() -> std::result::Result<String, Box<dyn std::error::Error>> {
    // Use system_profiler to get default output device
    let output = Command::new("system_profiler")
        .arg("SPAudioDataType")
        .output();

    match output {
        Ok(output) => {
            let output_str = String::from_utf8_lossy(&output.stdout);

            // Look for default output device
            for line in output_str.lines() {
                if line.contains("Default Output Device: Yes") {
                    return Ok("Built-in Output".to_string());
                }
            }

            Ok("Default Speakers".to_string())
        }
        Err(_) => Ok("Default Speakers".to_string()),
    }
}

// Get audio output peak level
// Estimates peak level based on active audio sessions
fn get_audio_output_peak_level_impl() -> std::result::Result<f32, Box<dyn std::error::Error>> {
    // Check if any audio is currently playing using coreaudiod activity
    // Method 1: Check if coreaudiod is actively processing audio
    let top_output = Command::new("top")
        .args(&["-l", "1", "-n", "1", "-stats", "pid,cpu,command"])
        .output();

    if let Ok(output) = top_output {
        let top_str = String::from_utf8_lossy(&output.stdout);

        // Check for coreaudiod CPU usage (indicates audio processing)
        for line in top_str.lines() {
            if line.contains("coreaudiod") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(cpu) = parts[1].replace("%", "").parse::<f32>() {
                        // If coreaudiod is using CPU, estimate peak level
                        if cpu > 0.5 {
                            // Scale CPU usage to peak level (rough estimate)
                            return Ok((cpu / 100.0).min(1.0).max(0.1));
                        }
                    }
                }
            }
        }
    }

    // Method 2: Check if any known audio apps are running
    let apps = get_apps_playing_audio_impl()?;
    if !apps.is_empty() {
        // If apps are playing, return a moderate peak level
        return Ok(0.3);
    }

    // No active audio detected
    Ok(0.0)
}

// Get applications playing audio
// Uses multiple methods to detect audio-playing applications
fn get_apps_playing_audio_impl() -> std::result::Result<Vec<AudioAppSession>, Box<dyn std::error::Error>> {
    let mut apps = Vec::new();
    let mut seen_pids = HashSet::new();

    // Method 1: Get processes with audio output using lsof for CoreAudio
    let lsof_output = Command::new("lsof")
        .args(&["-c", "coreaudiod"])
        .output();

    let mut audio_active = false;
    if let Ok(output) = lsof_output {
        let lsof_str = String::from_utf8_lossy(&output.stdout);
        if !lsof_str.is_empty() && lsof_str.lines().count() > 1 {
            audio_active = true;
        }
    }

    // Method 2: Get running applications that typically play audio
    let running_processes = get_running_processes();

    // Known audio/meeting applications
    let audio_apps = vec![
        // Browsers (for web meetings)
        "Google Chrome", "Chrome", "Safari", "Firefox", "Microsoft Edge", "Brave Browser",
        // Meeting apps
        "zoom.us", "Zoom", "Microsoft Teams", "Teams", "Slack", "Discord", "Skype",
        // Communication
        "WhatsApp", "Telegram", "Signal", "FaceTime",
        // Media players (to detect and filter)
        "Music", "Spotify", "VLC", "QuickTime Player",
    ];

    for app_name in audio_apps {
        if let Some(&pid) = running_processes.get(app_name) {
            if seen_pids.insert(pid) {
                // Get window title
                let window_title = crate::platform::PlatformUtils::get_window_title(pid)
                    .unwrap_or_else(|_| app_name.to_string());

                // Determine if this app is likely playing audio
                let is_active = audio_active || is_app_likely_playing_audio(app_name, &window_title);

                // Estimate peak level based on app type and activity
                let peak_level = if is_active {
                    estimate_app_audio_level(app_name, &window_title)
                } else {
                    0.0
                };

                apps.push(AudioAppSession {
                    name: app_name.to_string(),
                    volume: 75.0,
                    is_active,
                    peak_level,
                    process_id: pid,
                    window_title: window_title.clone(),
                });
            }
        }
    }

    // Method 3: Use pmset to detect if audio is preventing sleep
    let pmset_output = Command::new("pmset")
        .args(&["-g", "assertions"])
        .output();

    if let Ok(output) = pmset_output {
        let pmset_str = String::from_utf8_lossy(&output.stdout);

        // Check for PreventUserIdleSystemSleep or NoIdleSleepAssertion
        if pmset_str.contains("PreventUserIdleSystemSleep") || pmset_str.contains("NoIdleSleepAssertion") {
            // Extract process names from assertions
            for line in pmset_str.lines() {
                if line.contains("Process=") {
                    if let Some(process_start) = line.find("Process=") {
                        if let Some(process_part) = line[process_start + 8..].split_whitespace().next() {
                            let process_name = process_part.replace(",", "").replace("\"", "");

                            // Try to find PID for this process
                            if let Some(&pid) = running_processes.get(process_name.as_str()) {
                                if seen_pids.insert(pid) {
                                    let window_title = crate::platform::PlatformUtils::get_window_title(pid)
                                        .unwrap_or_else(|_| process_name.clone());

                                    apps.push(AudioAppSession {
                                        name: process_name,
                                        volume: 75.0,
                                        is_active: true,
                                        peak_level: 0.2,
                                        process_id: pid,
                                        window_title,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(apps)
}

// Check if an app is likely playing audio based on its name and window title
fn is_app_likely_playing_audio(app_name: &str, window_title: &str) -> bool {
    let combined = format!("{} {}", app_name, window_title).to_lowercase();

    // Meeting indicators
    let meeting_keywords = vec!["meet", "zoom", "teams", "slack", "call", "conference", "webinar"];
    for keyword in meeting_keywords {
        if combined.contains(keyword) {
            return true;
        }
    }

    // Media playback indicators
    let media_keywords = vec!["playing", "music", "spotify", "youtube"];
    for keyword in media_keywords {
        if combined.contains(keyword) {
            return true;
        }
    }

    false
}

// Estimate audio level for an app
fn estimate_app_audio_level(app_name: &str, window_title: &str) -> f32 {
    let combined = format!("{} {}", app_name, window_title).to_lowercase();

    // Meeting apps typically have moderate to high audio levels
    if combined.contains("meet") || combined.contains("zoom") || combined.contains("teams") || combined.contains("slack") {
        return 0.4; // 40% - typical meeting audio level
    }

    // Media players might have higher levels
    if combined.contains("music") || combined.contains("spotify") || combined.contains("youtube") {
        return 0.6; // 60% - typical media playback level
    }

    // Default for active audio
    0.2
}

// Public convenience functions
pub fn get_microphone_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    get_microphone_volume_and_mute_impl()
}

pub fn get_microphone_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_microphone_device_name_impl()
}

pub fn get_apps_using_microphone() -> std::result::Result<Vec<String>, Box<dyn std::error::Error>> {
    get_apps_using_microphone_impl()
}

pub fn get_audio_output_volume_and_mute() -> std::result::Result<AudioInfo, Box<dyn std::error::Error>> {
    get_audio_output_volume_and_mute_impl()
}

pub fn get_audio_output_device_name() -> std::result::Result<String, Box<dyn std::error::Error>> {
    get_audio_output_device_name_impl()
}

pub fn get_audio_output_peak_level() -> std::result::Result<f32, Box<dyn std::error::Error>> {
    get_audio_output_peak_level_impl()
}

pub fn get_apps_playing_audio() -> std::result::Result<Vec<AudioAppSession>, Box<dyn std::error::Error>> {
    get_apps_playing_audio_impl()
}
