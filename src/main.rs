mod audio;
mod connection;
mod error;
mod metrics;
mod signaling;
mod webrtc;

use crate::audio::{AudioCapture, AudioPlayback};
use crate::connection::{ConnectionMonitor, ConnectionState, ConnectionStatus};
use crate::error::{Error, Result};
use crate::metrics::{ConnectionQuality, QualityMonitor};
use crate::signaling::{SignalingClient, SignalingMessage};
use crate::webrtc::WebRTCClient;

use dioxus::prelude::*;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use rand::random;
use std::time::Duration;
use tokio::time::sleep;
use webrtc::peer_connection::signaling_state::RTCSignalingState;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::ice_transport::ice_candidate::RTCIceCandidateInit;
use webrtc::api::media_engine::MediaEngine;
use anyhow::Error as AnyhowError;

const MAX_RECONNECT_ATTEMPTS: u32 = 5;
const RECONNECT_DELAY_MS: u64 = 1000;

struct AppState {
    signaling: Option<Arc<Mutex<SignalingClient>>>,
    webrtc: Option<Arc<WebRTCClient>>,
    audio_capture: Option<AudioCapture>,
    peer_id: String,
    room_id: String,
    reconnect_attempts: u32,
}

impl AppState {
    async fn reconnect(&mut self) -> Result<()> {
        if self.reconnect_attempts >= MAX_RECONNECT_ATTEMPTS {
            return Err(Error::Connection(
                "Max reconnection attempts reached".to_string(),
            ));
        }

        self.reconnect_attempts += 1;
        sleep(Duration::from_millis(RECONNECT_DELAY_MS)).await;

        // Try to reconnect WebSocket
        match SignalingClient::connect("ws://127.0.0.1:8080").await {
            Ok(client) => {
                let client = Arc::new(Mutex::new(client));
                
                // Re-join the room
                let join_msg = SignalingMessage::Join {
                    room_id: self.room_id.clone(),
                    peer_id: self.peer_id.clone(),
                };
                
                client.lock().await.send(join_msg).await?;
                self.signaling = Some(client);
                self.reconnect_attempts = 0;
                Ok(())
            }
            Err(e) => {
                Err(Error::Connection(format!("Reconnection failed: {}", e)))
            }
        }
    }

    async fn handle_connection_error(&mut self, error: Error) -> Result<()> {
        match error {
            Error::WebSocket(_) | Error::Connection(_) => {
                println!("Connection error, attempting to reconnect...");
                self.reconnect().await
            }
            Error::WebRTC(e) => {
                // If it's a fatal WebRTC error, clean up and restart the call
                println!("WebRTC error: {}, cleaning up...", e);
                self.cleanup_call().await;
                Err(Error::WebRTC(e))
            }
            Error::Audio(e) => {
                // Log audio error but try to continue
                println!("Audio error: {}, continuing...", e);
                Ok(())
            }
            _ => Err(error),
        }
    }

    async fn cleanup_call(&mut self) {
        self.webrtc = None;
        self.audio_capture = None;
        
        if let Some(ref signaling) = self.signaling {
            let _ = signaling.lock().await.send(SignalingMessage::EndCall {
                room_id: self.room_id.clone(),
                peer_id: self.peer_id.clone(),
            }).await;
        }
    }
}

#[derive(Props)]
struct PeerItemProps<'a> {
    peer_id: String,
    selected: bool,
    on_select: EventHandler<'a, String>,
}

fn PeerItem<'a>(cx: Scope<'a, PeerItemProps<'a>>) -> Element {
    cx.render(rsx! {
        div { class: "peer-item",
            input {
                r#type: "checkbox",
                checked: "{cx.props.selected}",
                onclick: move |_| cx.props.on_select.call(cx.props.peer_id.clone())
            }
            span { "{cx.props.peer_id}" }
        }
    })
}

fn main() {
    dioxus_desktop::launch(App);
}

fn App(cx: Scope) -> Element {
    let state = use_ref(cx, || AppState {
        signaling: None,
        webrtc: None,
        audio_capture: None,
        peer_id: format!("user-{}", rand::random::<u32>()),
        room_id: "test-room".to_string(),
        reconnect_attempts: 0,
    });

    let connection_status = use_state(cx, || ConnectionStatus {
        state: ConnectionState::Disconnected,
        signaling_state: RTCSignalingState::Stable,
        ice_state: RTCIceConnectionState::New,
        peer_state: RTCPeerConnectionState::New,
        last_error: None,
    });
    let available_peers = use_state(cx, || Vec::<String>::new());
    let selected_peers = use_state(cx, || HashSet::<String>::new());
    let is_connected = use_state(cx, || false);
    let is_in_call = use_state(cx, || false);
    let is_muted = use_state(cx, || false);
    let error_message = use_state(cx, String::new);
    let quality_status = use_state(cx, || ConnectionQuality::default());

    let connect = move |_| {
        let state = state.clone();
        let connection_status = connection_status.clone();
        let is_connected = is_connected.clone();
        
        cx.spawn(async move {
            connection_status.set("Connecting...".to_string());
            
            if let Ok(client) = SignalingClient::connect("ws://127.0.0.1:8080").await {
                let client = Arc::new(Mutex::new(client));
                
                let join_msg = SignalingMessage::Join {
                    room_id: state.read().room_id.clone(),
                    peer_id: state.read().peer_id.clone(),
                };
                
                if let Ok(mut guard) = client.lock().await {
                    if guard.send(join_msg).await.is_ok() {
                        state.write().signaling = Some(client.clone());
                        connection_status.set("Connected to server".to_string());
                        is_connected.set(true);
                    }
                }
            } else {
                connection_status.set("Connection failed".to_string());
            }
        });
    };

    let start_call = move |_| {
        let state = state.clone();
        let selected = selected_peers.clone();
        let is_in_call = is_in_call.clone();
        
        cx.spawn(async move {
            let peers: Vec<String> = selected.get().iter().cloned().collect();
            if !peers.is_empty() {
                if let Ok(()) = start_call(state, peers).await {
                    is_in_call.set(true);
                }
            }
        });
    };

    let end_call = move |_| {
        let state = state.clone();
        let is_in_call = is_in_call.clone();
        
        cx.spawn(async move {
            if let Ok(mut state) = state.write() {
                // Clean up WebRTC and audio
                state.webrtc = None;
                state.audio_capture = None;
                
                // Send end call signal if needed
                if let Some(ref signaling) = state.signaling {
                    let mut sig = signaling.lock().await;
                    let _ = sig.send(SignalingMessage::EndCall {
                        room_id: state.room_id.clone(),
                        peer_id: state.peer_id.clone(),
                    }).await;
                }
                
                is_in_call.set(false);
            }
        });
    };

    let toggle_mute = move |_| {
        let state = state.clone();
        let is_muted = is_muted.clone();
        
        cx.spawn(async move {
            if let Ok(state) = state.read() {
                if let Some(ref webrtc_client) = state.webrtc {
                    let muted = !is_muted.get();
                    // Toggle audio track enabled state
                    if let Ok(senders) = webrtc_client.peer_connection.get_senders().await {
                        if let Some(sender) = senders.first() {
                            if let Some(track) = sender.track().await {
                                track.set_enabled(!muted);
                                is_muted.set(muted);
                            }
                        }
                    }
                }
            }
        });
    };

    let toggle_peer_selection = move |peer_id: String| {
        let selected = selected_peers.clone();
        let mut current = selected.get().clone();
        
        if current.contains(&peer_id) {
            current.remove(&peer_id);
        } else {
            current.insert(peer_id);
        }
        
        selected_peers.set(current);
    };

    let handle_error = move |error: Error| {
        let state = state.clone();
        let error_message = error_message.clone();
        
        cx.spawn(async move {
            let mut state = state.write();
            match state.handle_connection_error(error).await {
                Ok(_) => {
                    error_message.set("".to_string());
                }
                Err(e) => {
                    error_message.set(e.to_string());
                }
            }
        });
    };

    // Set up connection status monitoring when WebRTC client is created
    let monitor_connection = move |webrtc: Arc<WebRTCClient>| {
        let status = connection_status.clone();
        let mut receiver = webrtc.connection_monitor.subscribe();
        
        cx.spawn(async move {
            while receiver.changed().await.is_ok() {
                let new_status = receiver.borrow().clone();
                status.set(new_status);
            }
        });
    };

    // Set up quality monitoring when WebRTC client is created
    let monitor_quality = move |webrtc: Arc<WebRTCClient>| {
        let quality = quality_status.clone();
        let mut receiver = webrtc.quality_monitor.subscribe();
        
        cx.spawn(async move {
            while receiver.changed().await.is_ok() {
                let new_quality = receiver.borrow().clone();
                quality.set(new_quality);
            }
        });
    };

    cx.render(rsx! {
        style { include_str!("./style.css") }
        h1 { "WebRTC Voice Chat" }
        
        div { class: "control-panel",
            h3 { "Connection Settings" }
            div {
                label { r#for: "roomId", "Room ID:" }
                input {
                    id: "roomId",
                    value: "{state.read().room_id}",
                    disabled: "{*is_connected.get()}"
                }
                label { r#for: "peerId", "Peer ID:" }
                input {
                    id: "peerId",
                    value: "{state.read().peer_id}",
                    disabled: "{*is_connected.get()}"
                }
            }
            button {
                onclick: connect,
                disabled: "{*is_connected.get()}",
                "Connect to Server"
            }
        }

        div { class: "control-panel",
            h3 { "Available Peers" }
            div { class: "peer-list",
                available_peers.get().iter().map(|peer_id| {
                    rsx! {
                        PeerItem {
                            key: "{peer_id}",
                            peer_id: peer_id.clone(),
                            selected: selected_peers.get().contains(peer_id),
                            on_select: toggle_peer_selection
                        }
                    }
                })
            }
            button {
                onclick: start_call,
                disabled: "{!*is_connected.get() || *is_in_call.get() || selected_peers.get().is_empty()}",
                "Call Selected Peers"
            }
            button {
                onclick: end_call,
                disabled: "{!*is_in_call.get()}",
                "End Call"
            }
        }

        div { class: "control-panel",
            h3 { "Audio Controls" }
            button {
                onclick: toggle_mute,
                disabled: "{!*is_in_call.get()}",
                "{if *is_muted.get() { "Unmute" } else { "Mute" }}"
            }
        }

        div { class: "connection-status",
            div { class: "status-item",
                "Connection: ",
                span { 
                    class: "status-value {connection_status.get().state}",
                    "{connection_status.get().state}"
                }
            }
            div { class: "status-item",
                "ICE: ",
                span { 
                    class: "status-value",
                    "{connection_status.get().ice_state}"
                }
            }
            div { class: "status-item",
                "Signaling: ",
                span { 
                    class: "status-value",
                    "{connection_status.get().signaling_state}"
                }
            }
            {connection_status.get().last_error.as_ref().map(|error| rsx!(
                div { class: "status-error",
                    "Error: {error}"
                }
            ))}
        }

        {!error_message.get().is_empty().then(|| rsx!(
            div {
                class: "error-message",
                "{error_message.get()}"
            }
        ))}

        div { class: "quality-metrics",
            h3 { "Connection Quality" }
            div { class: "quality-item",
                "Quality Score: ",
                span { 
                    class: "quality-value {get_quality_class(quality_status.get().quality_score)}",
                    "{quality_status.get().quality_score}%"
                }
            }
            div { class: "quality-item",
                "Round Trip Time: ",
                span { class: "quality-value",
                    "{quality_status.get().round_trip_time:.1} ms"
                }
            }
            div { class: "quality-item",
                "Packet Loss: ",
                span { class: "quality-value",
                    "{quality_status.get().packet_loss_rate:.1}%"
                }
            }
            div { class: "quality-item",
                "Bitrate: ",
                span { class: "quality-value",
                    "{quality_status.get().bitrate:.1} kbps"
                }
            }
            div { class: "quality-item",
                "Audio Level: ",
                span { class: "quality-value",
                    "{quality_status.get().audio_level} dB"
                }
            }
        }
    })
}

fn get_quality_class(score: u8) -> &'static str {
    match score {
        90..=100 => "quality-excellent",
        70..=89 => "quality-good",
        50..=69 => "quality-fair",
        _ => "quality-poor"
    }
}

async fn handle_signaling_message(
    msg: SignalingMessage,
    state: Arc<Mutex<AppState>>,
) -> Result<()> {
    let mut state = state.lock().await;
    
    match msg {
        SignalingMessage::Error { message } => {
            Err(Error::Signaling(message))
        }
        SignalingMessage::ConnectionLost { peer_id } => {
            println!("Peer {} disconnected", peer_id);
            if state.webrtc.is_some() {
                state.cleanup_call().await;
            }
            Ok(())
        }
        SignalingMessage::CallRequest { from_peer, room_id, .. } => {
            // Create WebRTC client if it doesn't exist
            if state.webrtc.is_none() {
                state.webrtc = Some(Arc::new(WebRTCClient::new().await?));
            }

            // Send call response
            if let Some(ref signaling) = state.signaling {
                signaling.lock().await.send(SignalingMessage::CallResponse {
                    room_id,
                    from_peer: state.peer_id.clone(),
                    to_peer: from_peer,
                    accepted: true,
                }).await?;
            }
        }
        SignalingMessage::Offer { sdp, from_peer, room_id, .. } => {
            if let Some(ref webrtc) = state.webrtc {
                let answer = webrtc.handle_offer(sdp).await?;
                
                if let Some(ref signaling) = state.signaling {
                    signaling.lock().await.send(SignalingMessage::Answer {
                        room_id,
                        sdp: answer,
                        from_peer: state.peer_id.clone(),
                        to_peer: from_peer,
                    }).await?;
                }
            }
        }
        SignalingMessage::Answer { sdp, .. } => {
            if let Some(ref webrtc) = state.webrtc {
                webrtc.handle_answer(sdp).await?;
            }
        }
        SignalingMessage::IceCandidate { candidate, .. } => {
            let candidate_init = RTCIceCandidateInit {
                candidate: candidate,
                ..Default::default()
            };
            if let Some(ref webrtc) = state.webrtc {
                webrtc.peer_connection.add_ice_candidate(candidate_init).await?;
            }
        }
        _ => {}
    }
    Ok(())
}

async fn start_call(state: Arc<Mutex<AppState>>, selected_peers: Vec<String>) -> Result<()> {
    let mut state = state.lock().await;
    
    // Create WebRTC client if it doesn't exist
    if state.webrtc.is_none() {
        state.webrtc = Some(Arc::new(WebRTCClient::new().await?));
        
        // Set up audio capture
        if let Some(ref webrtc) = state.webrtc {
            state.audio_capture = Some(AudioCapture::new(webrtc.audio_track.clone())?);
        }
    }

    // Send call request
    if let Some(ref signaling) = state.signaling {
        signaling.lock().await.send(SignalingMessage::CallRequest {
            room_id: state.room_id.clone(),
            from_peer: state.peer_id.clone(),
            to_peers: selected_peers,
        }).await?;
    }

    Ok(())
}
