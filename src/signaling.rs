use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::connect_async;
use futures_util::{SinkExt, StreamExt};
use anyhow::Result;
use crate::utils::Error;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "message_type")]
pub enum SignalingMessage {
    Join {
        room_id: String,
        peer_id: String,
    },
    Disconnect {
        room_id: String,
        peer_id: String,
    },
    PeerList {
        peers: Vec<String>,
    },
    Offer {
        room_id: String,
        sdp: String,
        from_peer: String,
        to_peer: String,
    },
    Answer {
        room_id: String,
        sdp: String,
        from_peer: String,
        to_peer: String,
    },
    IceCandidate {
        room_id: String,
        candidate: String,
        from_peer: String,
        to_peer: String,
    },
    RequestPeerList,
    InitiateCall {
        peer_id: String,
        room_id: String,
    },
    MediaError {
        error_type: String,
        description: String,
        peer_id: String,
    },
    EndCall {
        room_id: String,
        peer_id: String,
    },
    CallRequest {
        room_id: String,
        from_peer: String,
        to_peers: Vec<String>,
    },
    CallResponse {
        room_id: String,
        from_peer: String,
        to_peer: String,
        accepted: bool,
    },
    Error {
        message: String,
    },
    ConnectionLost {
        peer_id: String,
    },
}

pub struct SignalingClient {
    tx: mpsc::Sender<SignalingMessage>,
    rx: mpsc::Receiver<SignalingMessage>,
}

impl SignalingClient {
    pub async fn connect(url: &str) -> Result<Self> {
        let (ws_stream, _) = connect_async(url).await?;
        let (write, read) = ws_stream.split();
        
        let (tx, rx) = mpsc::channel(100);
        let (outgoing_tx, mut outgoing_rx) = mpsc::channel(100);

        // Handle outgoing messages
        tokio::spawn(async move {
            while let Some(msg) = outgoing_rx.recv().await {
                if let Ok(json) = serde_json::to_string(&msg) {
                    if write.send(json.into()).await.is_err() {
                        break;
                    }
                }
            }
        });

        // Handle incoming messages
        tokio::spawn(async move {
            let mut read = read;
            while let Some(msg) = read.next().await {
                if let Ok(msg) = msg {
                    if let Ok(signal) = serde_json::from_str::<SignalingMessage>(msg.to_string().as_str()) {
                        if tx.send(signal).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });

        Ok(Self {
            tx: outgoing_tx,
            rx,
        })
    }

    pub async fn send(&mut self, msg: SignalingMessage) -> Result<()> {
        let json = serde_json::to_string(&msg)?;
        self.tx.send(msg).await.map_err(|e| Error::Signaling(format!("Failed to send message: {}", e)))?;
        Ok(())
    }

    pub async fn receive(&mut self) -> Result<Option<SignalingMessage>> {
        if let Some(msg) = self.rx.recv().await {
            Ok(Some(msg))
        } else {
            Ok(None)
        }
    }
} 