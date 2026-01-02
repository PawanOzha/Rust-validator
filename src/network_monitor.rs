use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, Duration};

/// Network signal indicating WebRTC activity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCSignal {
    pub process_id: u32,
    pub process_name: String,
    pub remote_ips: Vec<String>,
    pub has_stun_traffic: bool,
    pub has_media_traffic: bool,
    pub connection_count: usize,
    pub last_seen: SystemTime,
    pub started_at: SystemTime,
}

/// Network monitor for WebRTC detection
pub struct NetworkMonitor {
    active_connections: HashMap<u32, WebRTCSignal>,
    #[allow(dead_code)]
    known_stun_servers: HashSet<String>,
}

impl NetworkMonitor {
    pub fn new() -> Self {
        let mut known_stun_servers = HashSet::new();

        // Common STUN servers used by meeting apps
        known_stun_servers.insert("stun.l.google.com".to_string());
        known_stun_servers.insert("stun1.l.google.com".to_string());
        known_stun_servers.insert("stun2.l.google.com".to_string());
        known_stun_servers.insert("stun3.l.google.com".to_string());
        known_stun_servers.insert("stun4.l.google.com".to_string());
        known_stun_servers.insert("stun.teams.microsoft.com".to_string());
        known_stun_servers.insert("stun.zoom.us".to_string());
        known_stun_servers.insert("stun.slack.com".to_string());
        known_stun_servers.insert("turn.whatsapp.com".to_string());

        NetworkMonitor {
            active_connections: HashMap::new(),
            known_stun_servers,
        }
    }

    /// Get WebRTC signals for active connections
    /// This is a simplified implementation that uses Windows netstat
    /// For production, you'd use pcap, but this works without driver installation
    pub fn get_webrtc_signals(&mut self) -> Vec<WebRTCSignal> {
        #[cfg(target_os = "windows")]
        {
            self.scan_network_connections();
        }

        // Clean up stale connections (no activity for 10 seconds)
        let now = SystemTime::now();
        self.active_connections.retain(|_, signal| {
            now.duration_since(signal.last_seen).unwrap_or(Duration::from_secs(0)).as_secs() < 10
        });

        self.active_connections.values().cloned().collect()
    }

    #[cfg(target_os = "windows")]
    fn scan_network_connections(&mut self) {
        use std::process::Command;

        // Use netstat to get active connections with process IDs
        // netstat -ano gives us: Proto, Local Address, Foreign Address, State, PID
        let output = match Command::new("netstat")
            .args(&["-ano", "-p", "UDP"])
            .output()
        {
            Ok(output) => output,
            Err(_) => return,
        };

        let output_str = String::from_utf8_lossy(&output.stdout);

        for line in output_str.lines().skip(4) {
            self.parse_netstat_line(line);
        }
    }

    #[cfg(target_os = "windows")]
    fn parse_netstat_line(&mut self, line: &str) {
        let parts: Vec<&str> = line.split_whitespace().collect();

        // UDP format: UDP  0.0.0.0:PORT  *:*  PID
        if parts.len() >= 4 && parts[0] == "UDP" {
            if let Some(pid_str) = parts.last() {
                if let Ok(pid) = pid_str.parse::<u32>() {
                    if pid == 0 {
                        return; // Skip system process
                    }

                    // Check if this is a WebRTC-related port
                    let local_addr = parts[1];

                    // WebRTC typically uses high UDP ports (>10000)
                    // STUN uses port 3478, 19302
                    if self.is_webrtc_port(local_addr) {
                        self.update_or_create_signal(pid);
                    }
                }
            }
        }
    }

    fn is_webrtc_port(&self, addr: &str) -> bool {
        if let Some(port_str) = addr.split(':').last() {
            if let Ok(port) = port_str.parse::<u16>() {
                // STUN/TURN standard ports
                if port == 3478 || port == 19302 || port == 5349 {
                    return true;
                }

                // WebRTC media ports (typically >10000)
                if port >= 10000 && port <= 65535 {
                    return true;
                }
            }
        }
        false
    }

    fn update_or_create_signal(&mut self, pid: u32) {
        let now = SystemTime::now();

        self.active_connections.entry(pid)
            .and_modify(|signal| {
                signal.last_seen = now;
                signal.connection_count += 1;
            })
            .or_insert_with(|| {
                let process_name = get_process_name_from_pid(pid);
                WebRTCSignal {
                    process_id: pid,
                    process_name,
                    remote_ips: Vec::new(),
                    has_stun_traffic: true,
                    has_media_traffic: true,
                    connection_count: 1,
                    last_seen: now,
                    started_at: now,
                }
            });
    }

    /// Check if a specific process has WebRTC activity
    pub fn has_webrtc_activity(&self, process_id: u32) -> bool {
        self.active_connections.contains_key(&process_id)
    }

    /// Get WebRTC signal for specific process
    pub fn get_signal_for_process(&self, process_id: u32) -> Option<&WebRTCSignal> {
        self.active_connections.get(&process_id)
    }
}

#[cfg(target_os = "windows")]
fn get_process_name_from_pid(pid: u32) -> String {
    use std::process::Command;

    // Use tasklist to get process name
    let output = Command::new("tasklist")
        .args(&["/FI", &format!("PID eq {}", pid), "/FO", "CSV", "/NH"])
        .output();

    if let Ok(output) = output {
        let output_str = String::from_utf8_lossy(&output.stdout);
        if let Some(first_line) = output_str.lines().next() {
            // CSV format: "processname.exe","PID","Session","Memory"
            let parts: Vec<&str> = first_line.split(',').collect();
            if let Some(name) = parts.first() {
                return name.trim_matches('"').to_string();
            }
        }
    }

    format!("Process_{}", pid)
}

#[cfg(not(target_os = "windows"))]
fn get_process_name_from_pid(_pid: u32) -> String {
    String::from("Unknown")
}
