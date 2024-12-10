use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio::sync::Mutex;
use tokio::time::interval;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::rtp_transceiver::rtp_transceiver_direction::RTCRtpTransceiverDirection;
use webrtc::stats::stats_report::StatsReport;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionQuality {
    pub round_trip_time: f64,        // milliseconds
    pub jitter: f64,                 // milliseconds
    pub packet_loss_rate: f64,       // percentage (0-100)
    pub audio_level: f64,            // dB (-127 to 0)
    pub bitrate: f64,                // kbps
    pub quality_score: u8,           // 0-100
}

impl Default for ConnectionQuality {
    fn default() -> Self {
        Self {
            round_trip_time: 0.0,
            jitter: 0.0,
            packet_loss_rate: 0.0,
            audio_level: -127.0,
            bitrate: 0.0,
            quality_score: 100,
        }
    }
}

impl ConnectionQuality {
    fn calculate_quality_score(&mut self) {
        let rtt_score = if self.round_trip_time < 150.0 { 40 } 
                       else if self.round_trip_time < 300.0 { 30 }
                       else { 20 };

        let jitter_score = if self.jitter < 30.0 { 20 }
                          else if self.jitter < 50.0 { 15 }
                          else { 10 };

        let loss_score = if self.packet_loss_rate < 1.0 { 40 }
                        else if self.packet_loss_rate < 3.0 { 30 }
                        else if self.packet_loss_rate < 5.0 { 20 }
                        else { 10 };

        self.quality_score = (rtt_score + jitter_score + loss_score) as u8;
    }
}

pub struct QualityMonitor {
    peer_connection: Arc<RTCPeerConnection>,
    stats: Arc<Mutex<Option<StatsReport>>>,
}

impl QualityMonitor {
    pub fn new(peer_connection: Arc<RTCPeerConnection>) -> Self {
        Self {
            peer_connection,
            stats: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start_monitoring(&self) {
        let pc = self.peer_connection.clone();
        let stats = self.stats.clone();
        
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));
            
            loop {
                interval.tick().await;
                if let Ok(report) = pc.get_stats().await {
                    let mut stats_guard = stats.lock().await;
                    *stats_guard = Some(report);
                }
            }
        });
    }

    pub async fn get_current_stats(&self) -> Option<StatsReport> {
        let stats = self.stats.lock().await;
        stats.clone()
    }
}

fn extract_rtt(stats: &StatsReport) -> Option<f64> {
    // Implementation for extracting RTT from stats
    None // Placeholder
}

// Similar helper functions for other metrics... 