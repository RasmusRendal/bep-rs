#[macro_use]
mod items;
mod peer_connection_inner;
mod verifier;
use super::bep_state::BepState;
use super::sync_directory::{SyncDirectory, SyncFile};
use log;
use peer_connection_inner::{
    handle_connection, PeerConnectionInner, PeerRequestResponse, PeerRequestResponseType,
};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite};

// TODO: When this rust PR lands
// https://github.com/rust-lang/rust/issues/63063
//type Stream = (impl AsyncWrite+AsyncRead+Unpin+Send+'static);

/// When a connection is established to a peer, this class
/// should take over the socket. It creates its own thread
/// to handle the connection, and you can send it commands
/// Please appreciate the restraint it took to not name it
/// BeerConnection.
pub struct PeerConnection {
    /// Pushing things into this channel causes them to be
    /// sent over the tcp connection. See get_file for an
    /// example
    inner: PeerConnectionInner,
}

impl PeerConnection {
    pub fn new(
        socket: (impl AsyncWrite + AsyncRead + Unpin + Send + 'static),
        state: Arc<Mutex<BepState>>,
        connector: bool,
    ) -> Self {
        let inner = PeerConnectionInner::new(state, socket, connector);
        PeerConnection { inner }
    }

    async fn submit_request(
        &mut self,
        id: i32,
        response_type: PeerRequestResponseType,
        msg: Vec<u8>,
    ) -> io::Result<PeerRequestResponse> {
        self.inner.submit_request(id, response_type, msg).await
    }

    /// Sync an entire directory from the peer,
    /// overwriting the local copy
    pub async fn get_directory(&mut self, directory: &SyncDirectory) -> tokio::io::Result<()> {
        self.inner.get_directory(directory).await
    }

    pub async fn get_file(
        &mut self,
        directory: &SyncDirectory,
        sync_file: &SyncFile,
    ) -> tokio::io::Result<()> {
        self.inner.get_file(directory, sync_file).await
    }

    pub async fn close(&mut self) -> tokio::io::Result<()> {
        log::info!("{}: Connection close requested", self.inner.get_name());
        let response = self.inner.close().await?;

        match response {
            PeerRequestResponse::Closed => Ok(()),
            PeerRequestResponse::Error(e) => {
                log::error!(
                    "{}: Got error while trying to close request: {}",
                    self.inner.get_name(),
                    e
                );
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Got an error while trying to close connection",
                ))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid response to close request. This should not happen.",
            )),
        }
    }

    pub fn get_peer_name(&self) -> Option<String> {
        self.inner.get_peer().map(|x| x.name)
    }
}
