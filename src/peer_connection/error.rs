use std::io;

use thiserror::Error;
use tokio::time::error::Elapsed;

#[derive(Error, Debug, Clone)]
pub enum PeerConnectionError {
    #[error("Unknown Peer")]
    UnknownPeer,
    #[error("Received invalid magic bytes")]
    InvalidMagicBytes,
    #[error("Decode error: {0}")]
    DecodeError(prost::DecodeError),
    #[error("Connection timeout")]
    TimeoutError,
    #[error("Unknown error: {0}")]
    Other(String),
}

impl From<Elapsed> for PeerConnectionError {
    fn from(_err: Elapsed) -> Self {
        PeerConnectionError::TimeoutError
    }
}

impl From<io::Error> for PeerConnectionError {
    fn from(err: io::Error) -> Self {
        PeerConnectionError::Other(err.to_string())
    }
}

impl From<prost::DecodeError> for PeerConnectionError {
    fn from(error: prost::DecodeError) -> Self {
        PeerConnectionError::DecodeError(error)
    }
}

#[derive(Error, Debug)]
pub enum PeerCommandError {
    #[error("Connection closed unexpectedly.")]
    ConnectionClosed,
    #[error("Error establishing connection: {0}")]
    ConnectionError(PeerConnectionError),
}