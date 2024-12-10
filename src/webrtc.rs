use anyhow::Result;
use std::sync::Arc;
use tokio::sync::Mutex;
use webrtc::api::APIBuilder;
use webrtc::api::media_engine::MediaEngine;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::RTCPeerConnection;
use webrtc::peer_connection::signaling_state::RTCSignalingState;
use webrtc::ice_transport::ice_connection_state::RTCIceConnectionState;
use webrtc::track::track_local::track_local_static_sample::TrackLocalStaticSample;
use webrtc::track::track_local::{TrackLocal, TrackLocalWriter};
use webrtc::track::track_remote::TrackRemote;
use webrtc::rtp_transceiver::rtp_codec::{RTCRtpCodecCapability, RTPCodecType};
use webrtc::rtp_transceiver::rtp_receiver::RTCRtpReceiver;
use webrtc::media::media_stream::MediaStream;
use crate::audio::AudioPlayback;
use crate::connection::{ConnectionMonitor, ConnectionState};
use crate::metrics::QualityMonitor;

pub struct WebRTCClient {
    pub peer_connection: Arc<RTCPeerConnection>,
    pub audio_track: Arc<TrackLocalStaticSample>,
    pub audio_playback: Arc<Mutex<Option<AudioPlayback>>>,
    pub connection_monitor: ConnectionMonitor,
    pub quality_monitor: QualityMonitor,
}

impl WebRTCClient {
    pub async fn new() -> Result<Self> {
        let connection_monitor = ConnectionMonitor::new();
        let monitor = connection_monitor.clone();

        // Create a MediaEngine object to configure the supported codec
        let mut media_engine = webrtc::media_engine::MediaEngine::default();
        
        // Register default codecs
        media_engine.register_default_codecs()?;

        // Create an API object
        let api = APIBuilder::new()
            .with_media_engine(media_engine)
            .build();

        // Create configuration
        let config = RTCConfiguration {
            ice_servers: vec![RTCIceServer {
                urls: vec!["stun:stun.l.google.com:19302".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        // Create a new RTCPeerConnection
        let peer_connection = Arc::new(api.new_peer_connection(config).await?);

        // Create an audio track
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            RTCRtpCodecCapability {
                mime_type: "audio/opus".to_owned(),
                ..Default::default()
            },
            "audio".to_owned(),
            "webrtc-rs".to_owned(),
        ));

        // Add the audio track to the peer connection
        peer_connection
            .add_track(Arc::clone(&audio_track) as Arc<dyn TrackLocal + Send + Sync>)
            .await?;

        let audio_playback = Arc::new(Mutex::new(None));
        let audio_playback_clone = audio_playback.clone();

        // Set up track handling
        peer_connection.on_track(Box::new(move |track: Option<Arc<TrackRemote>>, _: Option<Arc<MediaStream>>, _: Option<Arc<RTCRtpReceiver>>| {
            if let Some(track) = track {
                if track.kind() == RTPCodecType::Audio {
                    let audio_playback = audio_playback_clone.clone();
                    Box::pin(async move {
                        if let Ok(playback) = AudioPlayback::new(track) {
                            let mut guard = audio_playback.lock().await;
                            *guard = Some(playback);
                        }
                    })
                } else {
                    Box::pin(async {})
                }
            } else {
                Box::pin(async {})
            }
        }));

        // Set up connection state monitoring
        peer_connection.on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            let monitor = monitor.clone();
            Box::pin(async move {
                monitor.update_peer_state(s);
                println!("Peer Connection State has changed: {}", s);
            })
        }));

        let monitor = connection_monitor.clone();
        peer_connection.on_signaling_state_change(Box::new(move |s: RTCSignalingState| {
            let monitor = monitor.clone();
            Box::pin(async move {
                monitor.update_signaling_state(s);
                println!("Signaling State has changed: {}", s);
            })
        }));

        let monitor = connection_monitor.clone();
        peer_connection.on_ice_connection_state_change(Box::new(move |s: RTCIceConnectionState| {
            let monitor = monitor.clone();
            Box::pin(async move {
                monitor.update_ice_state(s);
                println!("ICE Connection State has changed: {}", s);
            })
        }));

        let quality_monitor = QualityMonitor::new(peer_connection.clone());
        
        Ok(Self {
            peer_connection,
            audio_track,
            audio_playback,
            connection_monitor,
            quality_monitor,
        })
    }

    pub async fn create_offer(&self) -> Result<String> {
        let offer = self.peer_connection.create_offer(None).await?;
        self.peer_connection
            .set_local_description(offer.clone())
            .await?;
        Ok(serde_json::to_string(&offer)?)
    }

    pub async fn handle_answer(&self, sdp: String) -> Result<()> {
        let answer = serde_json::from_str(&sdp)?;
        self.peer_connection.set_remote_description(answer).await?;
        Ok(())
    }

    pub async fn handle_offer(&self, sdp: String) -> Result<String> {
        let offer = serde_json::from_str(&sdp)?;
        self.peer_connection.set_remote_description(offer).await?;
        
        let answer = self.peer_connection.create_answer(None).await?;
        self.peer_connection
            .set_local_description(answer.clone())
            .await?;
        
        Ok(serde_json::to_string(&answer)?)
    }

    pub async fn start_monitoring(&mut self) -> Result<()> {
        self.quality_monitor.start_monitoring().await;
        Ok(())
    }
} 