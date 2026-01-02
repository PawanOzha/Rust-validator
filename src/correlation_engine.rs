use std::time::{Duration, SystemTime};
use serde::{Deserialize, Serialize};

/// All signals collected from different sources
#[derive(Debug, Clone)]
pub struct MultiSignal {
    pub process_id: u32,
    pub process_name: String,
    pub window_title: String,

    // WASAPI signals
    pub has_mic_active: bool,
    pub has_audio_output: bool,
    pub audio_peak_level: f32,

    // Network signals
    pub has_webrtc_connection: bool,
    pub webrtc_started_at: Option<SystemTime>,

    // Metadata
    pub detected_app: Option<String>,
    pub duration: Duration,
}

/// Detection result with confidence scoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionResult {
    pub is_call: bool,
    pub confidence: f32,
    pub signal_type: SignalType,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalType {
    MeetingCall,      // High-confidence bidirectional call
    VoiceNote,        // One-way voice message
    MediaPlayback,    // YouTube, Spotify, etc.
    Unknown,
}

/// Correlation engine for multi-signal fusion
pub struct CorrelationEngine {
    // Known media sites to filter out
    media_sites: Vec<String>,

    // Call apps we care about
    call_apps: Vec<String>,
}

impl CorrelationEngine {
    pub fn new() -> Self {
        CorrelationEngine {
            media_sites: vec![
                "youtube".to_string(),
                "netflix".to_string(),
                "spotify".to_string(),
                "twitch".to_string(),
                "soundcloud".to_string(),
                "apple music".to_string(),
                "prime video".to_string(),
            ],
            call_apps: vec![
                "meet".to_string(),
                "google meet".to_string(),
                "slack".to_string(),
                "zoom".to_string(),
                "teams".to_string(),
                "microsoft teams".to_string(),
                "whatsapp".to_string(),
            ],
        }
    }

    /// Main detection logic with confidence scoring
    pub fn detect_call(&self, signal: &MultiSignal) -> DetectionResult {
        let mut confidence = 0.0;
        let mut reasons = Vec::new();

        // RULE 1: Must be a known call app
        if !self.is_call_app(&signal.process_name, &signal.window_title, &signal.detected_app) {
            return DetectionResult {
                is_call: false,
                confidence: 0.0,
                signal_type: SignalType::Unknown,
                reasons: vec!["Not a known call app".to_string()],
            };
        }

        // RULE 2: Filter out media playback (YouTube, Netflix, etc.)
        if self.is_media_site(&signal.window_title) {
            return DetectionResult {
                is_call: false,
                confidence: 0.0,
                signal_type: SignalType::MediaPlayback,
                reasons: vec!["Media playback site detected".to_string()],
            };
        }

        // RULE 3: Check for voice notes (mic only, no incoming audio, short duration)
        if self.is_voice_note(signal) {
            return DetectionResult {
                is_call: false,
                confidence: 0.3,
                signal_type: SignalType::VoiceNote,
                reasons: vec!["Voice note pattern detected".to_string()],
            };
        }

        // SIGNAL SCORING: Multi-source confidence fusion

        // Core signal: Audio output (someone speaking to you)
        if signal.has_audio_output && signal.audio_peak_level > 0.001 {
            confidence += 0.40;
            reasons.push("Audio output active".to_string());
        }

        // Strong signal: WebRTC connection (definitive proof of call)
        if signal.has_webrtc_connection {
            confidence += 0.35;
            reasons.push("WebRTC connection detected".to_string());
        }

        // Supporting signal: Microphone active
        if signal.has_mic_active {
            confidence += 0.15;
            reasons.push("Microphone active".to_string());
        } else {
            // Even without mic, can still be a call if user muted
            // But we need stronger signals
            reasons.push("Microphone muted/off".to_string());
        }

        // Metadata signal: Window title confirms call
        if self.window_title_confirms_call(&signal.window_title) {
            confidence += 0.10;
            reasons.push("Window title confirms meeting".to_string());
        }

        // Time-based validation (only for ongoing calls, not new ones)
        // Don't penalize new calls (duration = 0)
        if signal.duration > Duration::from_secs(1) && signal.duration < Duration::from_secs(5) {
            // Very short events are likely false positives (but not brand new calls)
            confidence *= 0.7;
            reasons.push("Short duration - reduced confidence".to_string());
        }

        // Determine if this is a call
        // Use relaxed threshold to match old logic behavior
        // Old logic: if (has_mic && has_audio && is_call_app) = detect
        // New scoring: Audio(40%) + Mic(15%) = 55%, so use 45% threshold
        let is_call = confidence >= 0.45; // 45% threshold (matches old logic)

        DetectionResult {
            is_call,
            confidence,
            signal_type: if is_call { SignalType::MeetingCall } else { SignalType::Unknown },
            reasons,
        }
    }

    /// Check if this matches voice note pattern
    fn is_voice_note(&self, signal: &MultiSignal) -> bool {
        // Voice note characteristics:
        // 1. Mic is active (recording)
        // 2. NO incoming audio (not listening to others)
        // 3. No WebRTC connection (not a peer-to-peer call)
        // 4. Usually short duration (<2 minutes)

        let has_outgoing_only = signal.has_mic_active && !signal.has_audio_output;
        let no_webrtc = !signal.has_webrtc_connection;
        let is_short = signal.duration < Duration::from_secs(120);

        // Voice note pattern
        if has_outgoing_only && no_webrtc {
            return true;
        }

        // Also check for very short mic-only sessions in WhatsApp/Slack
        if is_short && signal.has_mic_active && !signal.has_webrtc_connection {
            if let Some(app) = &signal.detected_app {
                if app.to_lowercase().contains("whatsapp") || app.to_lowercase().contains("slack") {
                    return true;
                }
            }
        }

        false
    }

    /// Check if this is a media playback site
    fn is_media_site(&self, window_title: &str) -> bool {
        let lower_title = window_title.to_lowercase();

        for media_site in &self.media_sites {
            if lower_title.contains(media_site) {
                return true;
            }
        }

        false
    }

    /// Check if this is a known call app
    fn is_call_app(&self, process_name: &str, window_title: &str, detected_app: &Option<String>) -> bool {
        let combined = format!(
            "{} {} {}",
            process_name.to_lowercase(),
            window_title.to_lowercase(),
            detected_app.as_ref().map(|s| s.to_lowercase()).unwrap_or_default()
        );

        for app in &self.call_apps {
            if combined.contains(app) {
                return true;
            }
        }

        false
    }

    /// Check if window title confirms a meeting is happening
    fn window_title_confirms_call(&self, window_title: &str) -> bool {
        let lower_title = window_title.to_lowercase();

        // Meeting-specific keywords in window titles
        let meeting_keywords = [
            "meeting",
            "call with",
            "video call",
            "zoom meeting",
            "teams meeting",
            " meet ",
            "conference",
        ];

        for keyword in &meeting_keywords {
            if lower_title.contains(keyword) {
                return true;
            }
        }

        false
    }

    /// Enhanced call detection that handles mic/camera off scenarios
    pub fn should_maintain_call(&self, signal: &MultiSignal, was_previously_call: bool) -> bool {
        if !was_previously_call {
            return false;
        }

        // RULE: Maintain call state if it's still detected as a valid call app AND:
        // 1. WebRTC connection still active (strongest signal), OR
        // 2. Audio output still active (hearing others even if muted), OR
        // 3. Microphone still active

        // First check: Must still be a known call app
        if !self.is_call_app(&signal.process_name, &signal.window_title, &signal.detected_app) {
            return false;
        }

        // Strong signal: WebRTC still connected AND (audio or mic active)
        if signal.has_webrtc_connection && (signal.has_audio_output || signal.has_mic_active) {
            return true;
        }

        // Medium signal: Still hearing others (even if mic/camera off)
        if signal.has_audio_output {
            return true;
        }

        // Mic-only active (edge case: audio temporarily cut out)
        if signal.has_mic_active {
            return true;
        }

        // No active signals - let grace period in main.rs handle it
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_voice_note_detection() {
        let engine = CorrelationEngine::new();

        let voice_note_signal = MultiSignal {
            process_id: 1234,
            process_name: "WhatsApp.exe".to_string(),
            window_title: "WhatsApp".to_string(),
            has_mic_active: true,
            has_audio_output: false,
            audio_peak_level: 0.0,
            has_webrtc_connection: false,
            webrtc_started_at: None,
            detected_app: Some("WhatsApp".to_string()),
            duration: Duration::from_secs(30),
        };

        assert!(engine.is_voice_note(&voice_note_signal));
    }

    #[test]
    fn test_youtube_filtering() {
        let engine = CorrelationEngine::new();

        assert!(engine.is_media_site("YouTube - Broadcast Yourself"));
        assert!(engine.is_media_site("Netflix - Watch TV Shows"));
        assert!(!engine.is_media_site("Google Meet - Meeting"));
    }
}
