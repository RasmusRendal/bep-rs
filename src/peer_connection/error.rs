use std::io;

use thiserror::Error;
use tokio::time::error::Elapsed;

/// Fatal connection errors
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

/// An error that may occur while asking something from a PeerConnection
#[derive(Error, Debug)]
pub enum PeerCommandError {
    #[error("Connection closed unexpectedly.")]
    ConnectionClosed,
    #[error("Invalid file requested")]
    InvalidFile,
    #[error("Inexistent File Requested")]
    NoSuchFile,
    #[error("Error establishing connection: {0}")]
    ConnectionError(PeerConnectionError),
    #[error("I/O Error: {0}")]
    IOError(io::Error),
    #[error("Connection not yet initialized")]
    UninitializedError,
    #[error("Unknown error type: {0}")]
    Other(String),
}

impl From<io::Error> for PeerCommandError {
    fn from(err: io::Error) -> Self {
        PeerCommandError::IOError(err)
    }
}

impl From<PeerConnectionError> for PeerCommandError {
    fn from(err: PeerConnectionError) -> Self {
        PeerCommandError::ConnectionError(err)
    }
}
