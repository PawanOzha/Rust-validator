mod mic_monitor;
mod audio_output_monitor;
mod network_monitor;
mod correlation_engine;
#[cfg(target_os = "windows")]
mod wasapi_audio;

use mic_monitor::MicMonitor;
use audio_output_monitor::AudioOutputMonitor;
use network_monitor::NetworkMonitor;
use correlation_engine::{CorrelationEngine, MultiSignal};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::thread;
use std::time::{Duration, SystemTime};
use chrono::Timelike;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AudioSource {
    name: String,
    process_id: u32,
    window_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detected_app: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MonitorState {
    active_call: Option<CallInfo>,
    other_audio_sources: Vec<AudioSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallInfo {
    app: String,
    process_id: u32,
    window_title: String,
    has_mic: bool,
    has_audio: bool,
    has_webrtc: bool,
    confidence: f32,
    started_at: String,
    #[serde(skip, default = "default_system_time")]
    last_seen: SystemTime,
    #[serde(skip, default = "default_system_time")]
    call_started_system_time: SystemTime,
}

fn default_system_time() -> SystemTime {
    SystemTime::now()
}

#[derive(Debug, Serialize, Deserialize)]
struct JsonLogEntry {
    timestamp: String,
    active_call: Option<CallInfo>,
    other_audio: Vec<AudioSource>,
}

// Communication apps we care about
const CALL_APPS: &[&str] = &[
    "meet.google.com",
    "meet",
    "slack",
    "zoom",
    "teams",
    "whatsapp",
];

// Grace period before ending call (seconds)
// Reduced to 2s for faster detection while still preventing false endings
const CALL_END_GRACE_PERIOD: u64 = 2;

/// OS information structure
#[derive(Debug)]
struct OSInfo {
    os_name: String,
    arch: String,
    family: String,
    platform_details: String,
}

/// Get detailed OS information in a robust way
fn get_os_info() -> OSInfo {
    use std::env::consts;

    // Get basic OS name
    let os_name = match consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        "dragonfly" => "DragonFly BSD",
        "android" => "Android",
        "ios" => "iOS",
        "solaris" => "Solaris",
        "illumos" => "illumos",
        "redox" => "Redox",
        "fuchsia" => "Fuchsia",
        "haiku" => "Haiku",
        unknown => unknown,
    };

    // Get architecture
    let arch = match consts::ARCH {
        "x86_64" => "64-bit (x86_64)",
        "x86" => "32-bit (x86)",
        "aarch64" => "64-bit ARM (aarch64)",
        "arm" => "32-bit ARM",
        "powerpc64" => "PowerPC 64-bit",
        "powerpc" => "PowerPC 32-bit",
        "mips64" => "MIPS 64-bit",
        "mips" => "MIPS 32-bit",
        "riscv64" => "RISC-V 64-bit",
        "s390x" => "IBM S390x",
        "sparc64" => "SPARC 64-bit",
        unknown => unknown,
    };

    // Get OS family
    let family = match consts::FAMILY {
        "unix" => "Unix-like",
        "windows" => "Windows",
        unknown => unknown,
    };

    // Get platform-specific details
    let platform_details = get_platform_specific_info(consts::OS);

    OSInfo {
        os_name: os_name.to_string(),
        arch: arch.to_string(),
        family: family.to_string(),
        platform_details,
    }
}

/// Get platform-specific version information
fn get_platform_specific_info(os: &str) -> String {
    match os {
        "windows" => get_windows_version(),
        "linux" => get_linux_version(),
        "macos" => get_macos_version(),
        _ => format!("{} (version detection not implemented)", os),
    }
}

#[cfg(target_os = "windows")]
fn get_windows_version() -> String {
    use std::process::Command;

    // Try to get Windows version using 'ver' command
    if let Ok(output) = Command::new("cmd").args(&["/c", "ver"]).output() {
        if let Ok(version_str) = String::from_utf8(output.stdout) {
            let version_str = version_str.trim();
            if !version_str.is_empty() {
                return version_str.to_string();
            }
        }
    }

    // Fallback: Try systeminfo (slower but more detailed)
    if let Ok(output) = Command::new("wmic")
        .args(&["os", "get", "Caption,Version", "/value"])
        .output()
    {
        if let Ok(info) = String::from_utf8(output.stdout) {
            let mut caption = String::new();
            let mut version = String::new();

            for line in info.lines() {
                if line.starts_with("Caption=") {
                    caption = line.replace("Caption=", "").trim().to_string();
                } else if line.starts_with("Version=") {
                    version = line.replace("Version=", "").trim().to_string();
                }
            }

            if !caption.is_empty() || !version.is_empty() {
                return format!("{} (Build {})", caption, version);
            }
        }
    }

    "Windows (version unknown)".to_string()
}

#[cfg(not(target_os = "windows"))]
fn get_windows_version() -> String {
    "Not running on Windows".to_string()
}

#[cfg(target_os = "linux")]
fn get_linux_version() -> String {
    use std::fs;

    // Try reading /etc/os-release (standard on modern Linux)
    if let Ok(content) = fs::read_to_string("/etc/os-release") {
        let mut name = String::new();
        let mut version = String::new();

        for line in content.lines() {
            if line.starts_with("PRETTY_NAME=") {
                name = line
                    .replace("PRETTY_NAME=", "")
                    .trim_matches('"')
                    .to_string();
            } else if line.starts_with("VERSION=") {
                version = line.replace("VERSION=", "").trim_matches('"').to_string();
            }
        }

        if !name.is_empty() {
            return name;
        }
        if !version.is_empty() {
            return format!("Linux {}", version);
        }
    }

    // Fallback: Try uname
    if let Ok(output) = std::process::Command::new("uname").arg("-a").output() {
        if let Ok(uname_str) = String::from_utf8(output.stdout) {
            return uname_str.trim().to_string();
        }
    }

    "Linux (version unknown)".to_string()
}

#[cfg(not(target_os = "linux"))]
fn get_linux_version() -> String {
    "Not running on Linux".to_string()
}

#[cfg(target_os = "macos")]
fn get_macos_version() -> String {
    use std::process::Command;

    // Try to get macOS version using sw_vers
    if let Ok(output) = Command::new("sw_vers").output() {
        if let Ok(version_str) = String::from_utf8(output.stdout) {
            let mut product_name = String::new();
            let mut product_version = String::new();

            for line in version_str.lines() {
                if line.starts_with("ProductName:") {
                    product_name = line.replace("ProductName:", "").trim().to_string();
                } else if line.starts_with("ProductVersion:") {
                    product_version = line.replace("ProductVersion:", "").trim().to_string();
                }
            }

            if !product_name.is_empty() && !product_version.is_empty() {
                return format!("{} {}", product_name, product_version);
            }
        }
    }

    "macOS (version unknown)".to_string()
}

#[cfg(not(target_os = "macos"))]
fn get_macos_version() -> String {
    "Not running on macOS".to_string()
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let is_stream = args.contains(&"--stream".to_string());
    
    let log_dir = args.iter()
        .position(|r| r == "--log-dir")
        .and_then(|i| args.get(i + 1))
        .map(|s| PathBuf::from(s));

    if !is_stream {
        // Only print headers if NOT streaming JSON to stdout
        println!("\n=== Recordio Call Validator (Enhanced) ===");
        println!("Tracking: Meet, Slack, Zoom, Teams, WhatsApp");
        // println!("Features: WebRTC Detection, Voice Note Filtering, YouTube Filtering");
        // println!("Console: Call start/end only");
        // println!("Full logs: audio_monitor_rust.json");

        // Display OS information
        let os_info = get_os_info();
        println!("\n=== Worker Installed (System Information) ===");
        println!("Operating System: {}", os_info.os_name);
        println!("Architecture: {}", os_info.arch);
        // println!("OS Family: {}", os_info.family);
        // println!("Platform: {}", os_info.platform_details);
        // println!();
    }

    let mut previous_state = MonitorState {
        active_call: None,
        other_audio_sources: Vec::new(),
    };

    // Initialize network monitor and correlation engine
    let mut network_monitor = NetworkMonitor::new();
    let correlation_engine = CorrelationEngine::new();

    loop {
        let mut current_state = MonitorState {
            active_call: None,
            other_audio_sources: Vec::new(),
        };

        let mut mic_sources: Vec<AudioSource> = Vec::new();
        let mut audio_sources: Vec<AudioSource> = Vec::new();

        // Get microphone sources
        if let Ok(mut monitor) = MicMonitor::new() {
            if let Ok(report) = monitor.build_status_report() {
                for app_name in &report.conflicts.apps_using_mic {
                    mic_sources.push(AudioSource {
                        name: app_name.clone(),
                        process_id: 0,
                        window_title: String::new(),
                        detected_app: detect_call_app(app_name, ""),
                    });
                }
            }
        }

        // Get audio output sources
        if let Ok(mut monitor) = AudioOutputMonitor::new() {
            if let Ok(report) = monitor.build_status_report() {
                for app in report.active_apps {
                    if app.is_playing || app.peak_level > 0.001 {
                        audio_sources.push(AudioSource {
                            name: app.name.clone(),
                            process_id: app.process_id,
                            window_title: app.window_title.clone(),
                            detected_app: detect_call_app(&app.name, &app.window_title),
                        });
                    }
                }
            }
        }

        // Get WebRTC signals from network monitor (updates internal state)
        let _webrtc_signals = network_monitor.get_webrtc_signals();

        // Check if previous call is still active
        if let Some(prev_call) = &previous_state.active_call {
            // Build signal for existing call
            let audio_src = audio_sources.iter().find(|src| src.process_id == prev_call.process_id);
            let has_mic = mic_sources.iter().any(|src| {
                if let Some(detected) = &src.detected_app {
                    detected == &prev_call.app
                } else {
                    false
                }
            });
            let has_audio = audio_src.is_some();
            let has_webrtc = network_monitor.has_webrtc_activity(prev_call.process_id);

            let audio_peak_level = audio_src.map(|_src| 0.1).unwrap_or(0.0); // Simplified
            let window_title = audio_src
                .map(|src| src.window_title.clone())
                .unwrap_or_else(|| prev_call.window_title.clone());

            // Calculate call duration
            let call_duration = SystemTime::now()
                .duration_since(prev_call.call_started_system_time)
                .unwrap_or(Duration::from_secs(0));

            let signal = MultiSignal {
                process_id: prev_call.process_id,
                process_name: prev_call.app.clone(),
                window_title: window_title.clone(),
                has_mic_active: has_mic,
                has_audio_output: has_audio,
                audio_peak_level,
                has_webrtc_connection: has_webrtc,
                webrtc_started_at: None,
                detected_app: Some(prev_call.app.clone()),
                duration: call_duration,
            };

            // Enhanced: Use correlation engine to determine if call should continue
            // This handles mic/camera off scenarios
            let should_continue = correlation_engine.should_maintain_call(&signal, true);

            if should_continue {
                // Call is still active - update it
                let detection = correlation_engine.detect_call(&signal);

                current_state.active_call = Some(CallInfo {
                    app: prev_call.app.clone(),
                    process_id: prev_call.process_id,
                    window_title,
                    has_mic,
                    has_audio,
                    has_webrtc,
                    confidence: detection.confidence,
                    started_at: prev_call.started_at.clone(),
                    last_seen: SystemTime::now(),
                    call_started_system_time: prev_call.call_started_system_time,
                });
            } else {
                // Call signals lost - check grace period
                let elapsed = SystemTime::now()
                    .duration_since(prev_call.last_seen)
                    .unwrap_or(Duration::from_secs(0));

                if elapsed.as_secs() < CALL_END_GRACE_PERIOD {
                    // Still within grace period - keep the call active
                    current_state.active_call = Some(prev_call.clone());
                }
                // else: grace period expired, call will end
            }
        } else {
            // No previous call - detect new calls using enhanced correlation engine
            for audio_src in &audio_sources {
                if let Some(detected) = &audio_src.detected_app {
                    let is_browser = is_browser_process(&audio_src.name);

                    // Check if this app has mic active
                    let has_mic = if is_browser {
                        // For browsers, check if ANY browser is using the mic
                        // (can't correlate specific tabs without browser extension)
                        mic_sources.iter().any(|mic_src| is_browser_process(&mic_src.name))
                    } else {
                        // For native apps, require exact app match
                        mic_sources.iter().any(|mic_src| {
                            if let Some(mic_detected) = &mic_src.detected_app {
                                mic_detected == detected
                            } else {
                                false
                            }
                        })
                    };

                    // Check for WebRTC connection
                    let has_webrtc = network_monitor.has_webrtc_activity(audio_src.process_id);

                    // Build multi-signal for correlation engine
                    let signal = MultiSignal {
                        process_id: audio_src.process_id,
                        process_name: audio_src.name.clone(),
                        window_title: audio_src.window_title.clone(),
                        has_mic_active: has_mic,
                        has_audio_output: true,
                        audio_peak_level: 0.1, // Simplified
                        has_webrtc_connection: has_webrtc,
                        webrtc_started_at: None,
                        detected_app: Some(detected.clone()),
                        duration: Duration::from_secs(0), // New call
                    };

                    // ENHANCED: Use correlation engine to detect call
                    // This filters out voice notes, YouTube, and other false positives
                    let detection = correlation_engine.detect_call(&signal);

                    // DEBUG: Show what's being detected
                    if !is_stream && (detection.confidence > 0.3 || has_mic || has_webrtc) {
                        eprintln!("[DEBUG] App: {} | Mic: {} | Audio: {} | WebRTC: {} | Confidence: {:.0}% | Call: {}",
                            detected, has_mic, true, has_webrtc, detection.confidence * 100.0, detection.is_call);
                        if !detection.reasons.is_empty() {
                            eprintln!("[DEBUG] Reasons: {:?}", detection.reasons);
                        }
                    }

                    if detection.is_call {
                        // High-confidence call detected!
                        let now = SystemTime::now();
                        current_state.active_call = Some(CallInfo {
                            app: detected.clone(),
                            process_id: audio_src.process_id,
                            window_title: audio_src.window_title.clone(),
                            has_mic,
                            has_audio: true,
                            has_webrtc,
                            confidence: detection.confidence,
                            started_at: chrono::Local::now().format("%H:%M:%S").to_string(),
                            last_seen: now,
                            call_started_system_time: now,
                        });
                        break;
                    }
                    // else: Not a call (voice note, YouTube, etc.) - skip
                }
            }
        }

        // Collect other audio sources (not the active call)
        for audio_src in &audio_sources {
            let is_active_call = if let Some(call) = &current_state.active_call {
                audio_src.process_id == call.process_id
            } else {
                false
            };

            if !is_active_call {
                current_state.other_audio_sources.push(audio_src.clone());
            }
        }

        // Stream to stdout if requested
        if is_stream {
            if let Ok(json) = serde_json::to_string(&current_state) {
                println!("{}", json);
            }
        }

        // Log to JSON if log_dir is provided
        if let Some(ref path) = log_dir {
            log_to_custom_file(&current_state, path);
        }

        // Log state changes to console (only if not streaming)
        if !is_stream {
            log_state_changes(&previous_state, &current_state);
        }

        // Update previous state
        previous_state = current_state;

        // Sleep before next check
        thread::sleep(Duration::from_millis(500));
    }
}

/// Log current state to specific file
fn log_to_custom_file(state: &MonitorState, dir: &PathBuf) {
    // Ensure directory exists
    if !dir.exists() {
        if let Err(e) = std::fs::create_dir_all(dir) {
            eprintln!("[rust] Failed to create log directory {:?}: {}", dir, e);
            return;
        }
    }

    let entry = JsonLogEntry {
        timestamp: chrono::Local::now().to_rfc3339(),
        active_call: state.active_call.clone(),
        other_audio: state.other_audio_sources.clone(),
    };

    let log_path = dir.join("rust_monitor.log");

    match OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(mut file) => {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{}", json);
            }
        }
        Err(e) => {
            eprintln!("[rust] Failed to open log file {:?}: {}", log_path, e);
        }
    }
}

/// Detect which call app this is
fn detect_call_app(process_name: &str, window_title: &str) -> Option<String> {
    let combined = format!("{} {}", process_name.to_lowercase(), window_title.to_lowercase());

    for app in CALL_APPS {
        if combined.contains(app) {
            return Some(match *app {
                "meet.google.com" | "meet" => "Google Meet".to_string(),
                "slack" => "Slack".to_string(),
                "zoom" => "Zoom".to_string(),
                "teams" => "Microsoft Teams".to_string(),
                "whatsapp" => "WhatsApp".to_string(),
                _ => app.to_string(),
            });
        }
    }

    None
}

/// Log only call start/end to console (minimal)
fn log_state_changes(previous: &MonitorState, current: &MonitorState) {
    let timestamp = chrono::Local::now().format("%H:%M:%S").to_string();

    // Call started
    if previous.active_call.is_none() && current.active_call.is_some() {
        if let Some(call) = &current.active_call {
            println!("[{}] ======> CALL STARTED - {}", timestamp, call.app);
        }
    }
    // Call ended
    else if previous.active_call.is_some() && current.active_call.is_none() {
        if let Some(prev_call) = &previous.active_call {
            let duration = calculate_duration(&prev_call.started_at);
            println!("[{}] ======> CALL ENDED - {} (Duration: {})", timestamp, prev_call.app, duration);
        }
    }
}

/// Calculate call duration
fn calculate_duration(started_at: &str) -> String {
    let now = chrono::Local::now();

    // Parse the start time (HH:MM:SS)
    if let Ok(start_time) = chrono::NaiveTime::parse_from_str(started_at, "%H:%M:%S") {
        let current_time = now.time();

        let duration_secs = if current_time >= start_time {
            (current_time - start_time).num_seconds()
        } else {
            // Handle day boundary
            let seconds_to_midnight = (chrono::NaiveTime::from_hms_opt(23, 59, 59).unwrap() - start_time).num_seconds();
            let seconds_from_midnight = current_time.num_seconds_from_midnight() as i64;
            seconds_to_midnight + seconds_from_midnight + 1
        };

        let hours = duration_secs / 3600;
        let minutes = (duration_secs % 3600) / 60;
        let seconds = duration_secs % 60;

        if hours > 0 {
            format!("{}h {}m {}s", hours, minutes, seconds)
        } else if minutes > 0 {
            format!("{}m {}s", minutes, seconds)
        } else {
            format!("{}s", seconds)
        }
    } else {
        format!("Started at {}", started_at)
    }
}

/// Check if process is a browser
fn is_browser_process(process_name: &str) -> bool {
    let lower = process_name.to_lowercase();
    lower.contains("chrome") ||
    lower.contains("firefox") ||
    lower.contains("edge") ||
    lower.contains("msedge") ||
    lower.contains("brave")
}

