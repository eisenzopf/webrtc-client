use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use tokio::sync::watch;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::signaling_state::RTCSignalingState;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Failed,
    Reconnecting,
}

impl fmt::Display for ConnectionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectionState::Disconnected => write!(f, "Disconnected"),
            ConnectionState::Connecting => write!(f, "Connecting"),
            ConnectionState::Connected => write!(f, "Connected"),
            ConnectionState::Failed => write!(f, "Failed"),
            ConnectionState::Reconnecting => write!(f, "Reconnecting"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionStatus {
    pub state: ConnectionState,
    pub signaling_state: RTCSignalingState,
    pub ice_state: RTCIceConnectionState,
    pub peer_state: RTCPeerConnectionState,
    pub last_error: Option<String>,
}

impl Default for ConnectionStatus {
    fn default() -> Self {
        Self {
            state: ConnectionState::Disconnected,
            signaling_state: RTCSignalingState::Stable,
            ice_state: RTCIceConnectionState::New,
            peer_state: RTCPeerConnectionState::New,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct ConnectionMonitor {
    status: Arc<watch::Sender<ConnectionStatus>>,
    receiver: watch::Receiver<ConnectionStatus>,
}

impl ConnectionMonitor {
    pub fn new() -> Self {
        let (status, receiver) = watch::channel(ConnectionStatus::default());
        Self {
            status: Arc::new(status),
            receiver,
        }
    }

    pub fn update_state(&self, state: ConnectionState) {
        let _ = self.status.send_modify(|status| {
            status.state = state;
        });
    }

    pub fn update_signaling_state(&self, state: RTCSignalingState) {
        let _ = self.status.send_modify(|status| {
            status.signaling_state = state;
        });
    }

    pub fn update_ice_state(&self, state: RTCIceConnectionState) {
        let _ = self.status.send_modify(|status| {
            status.ice_state = state;
            status.state = match state {
                RTCIceConnectionState::Connected => ConnectionState::Connected,
                RTCIceConnectionState::Failed => ConnectionState::Failed,
                RTCIceConnectionState::Disconnected => ConnectionState::Disconnected,
                RTCIceConnectionState::Checking => ConnectionState::Connecting,
                _ => status.state.clone(),
            };
        });
    }

    pub fn update_peer_state(&self, state: RTCPeerConnectionState) {
        if let Ok(mut status) = self.status.send_modify() {
            status.peer_state = state;
        }
    }

    pub fn set_error(&self, error: String) {
        if let Ok(mut status) = self.status.send_modify() {
            status.last_error = Some(error);
            status.state = ConnectionState::Failed;
        }
    }

    pub fn subscribe(&self) -> watch::Receiver<ConnectionStatus> {
        self.receiver.clone()
    }
} 