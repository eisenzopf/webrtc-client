use std::fmt;
use webrtc::Error as WebRTCError;
use tokio_tungstenite::tungstenite::Error as WsError;
use anyhow::Error as AnyhowError;

#[derive(Debug)]
pub enum AppError {
    WebRTC(WebRTCError),
    Ws(WsError),
    Other(AnyhowError),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::WebRTC(e) => write!(f, "WebRTC error: {}", e),
            AppError::Ws(e) => write!(f, "WebSocket error: {}", e),
            AppError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl std::error::Error for AppError {}

impl From<WebRTCError> for AppError {
    fn from(err: WebRTCError) -> Self {
        AppError::WebRTC(err)
    }
}

impl From<WsError> for AppError {
    fn from(err: WsError) -> Self {
        AppError::Ws(err)
    }
}

impl From<AnyhowError> for AppError {
    fn from(err: AnyhowError) -> Self {
        AppError::Other(err)
    }
}

pub type Result<T> = std::result::Result<T, AppError>; 