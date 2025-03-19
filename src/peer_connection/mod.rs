#[macro_use]
mod items;
pub mod error;
mod handlers;
mod verifier;
mod watcher;
use crate::bep_state_reference::BepStateRef;
use crate::models::Peer;
use crate::sync_directory::{SyncDirectory, SyncFile};
use error::{PeerCommandError, PeerConnectionError};
use handlers::{drain_requests, handle_connection};
use rand::distr::StandardUniform;
use tokio_util::sync::CancellationToken;

use core::time;
use futures::channel::oneshot::{self, Canceled};
use items::EncodableItem;
use log;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ring::digest;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::sync::{Arc, OnceLock, RwLock};
use std::thread;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{channel, Sender};
use tokio_util::task::TaskTracker;

#[derive(Copy, Clone, PartialEq)]
pub enum PeerRequestResponseType {
    None,
    WhenReady,
    WhenSent,
    WhenResponse,
    WhenClosed,
}
pub enum PeerRequestResponse {
    Response(items::Response),
    Error(PeerCommandError),
    Sent,
    Closed,
}

pub struct PeerRequestListener {
    pub id: i32,
    pub response_type: PeerRequestResponseType,
    pub peer_connection: oneshot::Sender<PeerRequestResponse>,
}

/// When a connection is established to a peer, this class
/// should take over the socket. It creates its own thread
/// to handle the connection, and you can send it commands
/// Please appreciate the restraint it took to not name it
/// BeerConnection.
#[derive(Clone)]
pub struct PeerConnection {
    state: BepStateRef,
    // Handles for pending requests that need to be answered
    // We just keep it around, to empty it when the channel closes
    requests: Arc<RwLock<HashMap<i32, PeerRequestListener>>>,
    // A channel for sending messages that should be sent to the peer
    // Each message may optionally include a oneshot sender, if we need
    // to know when the message has been sent.
    tx: Sender<(Vec<u8>, Option<oneshot::Sender<PeerRequestResponse>>)>,
    // A channel to activate, to shut down the connection
    cancellation_token: CancellationToken,
    task_tracker: TaskTracker,

    // Any fatal error should be stored here
    error: Arc<OnceLock<Option<PeerConnectionError>>>,

    // Set when the peer connects
    peer_id: Arc<OnceLock<Vec<u8>>>,
    name: Arc<OnceLock<String>>,
}

impl PeerConnection {
    pub fn new(
        socket: (impl AsyncWrite + AsyncRead + Unpin + Send + 'static),
        state: BepStateRef,
        connector: bool,
    ) -> Self {
        let (tx, rx) = channel(100);
        let cancellation_token = CancellationToken::new();
        let peer_connection = PeerConnection {
            state: state.clone(),
            requests: Arc::new(RwLock::new(HashMap::new())),
            tx,
            cancellation_token: cancellation_token.clone(),
            error: Arc::new(OnceLock::new()),
            peer_id: Arc::new(OnceLock::new()),
            name: Arc::new(OnceLock::new()),
            task_tracker: TaskTracker::new(),
        };
        let peer_connectionc = peer_connection.clone();
        let mut sc = state.clone();
        tokio::spawn(async move {
            sc.add_peer_connection(peer_connection.clone()).await;
            if let Err(e) = handle_connection(
                socket,
                peer_connection.clone(),
                rx,
                cancellation_token,
                connector,
            )
            .await
            {
                log::error!(
                    "{}: Error occured in client {:?}",
                    peer_connection.get_name().await,
                    e
                );
                if let Err(err) = peer_connection.error.set(Some(e)) {
                    log::error!(
                        "{}: Error storing error in client: {:?}",
                        peer_connection.get_name().await,
                        err
                    );
                }
            }
            drain_requests(&peer_connection);
        });

        peer_connectionc
    }

    pub async fn send_index(&self) -> io::Result<()> {
        if let Ok(peer) = self.get_peer().await {
            let directories = self.state.get_sync_directories().await;
            for dir in directories {
                if !self
                    .state
                    .is_directory_synced(dir.id.clone(), peer.id.unwrap())
                    .await
                {
                    continue;
                }

                let files = dir
                    .get_index(self.state.clone())
                    .await
                    .iter()
                    .map(|x| -> Result<items::FileInfo, std::io::Error> {
                        Ok(items::FileInfo {
                            name: x.get_name(),
                            r#type: items::FileInfoType::File as i32,
                            size: x.get_size() as i64,
                            permissions: 0,
                            modified_s: x.modified_s(),
                            modified_ns: 0,
                            modified_by: 0,
                            deleted: false,
                            invalid: false,
                            no_permissions: true,
                            version: Some(items::Vector {
                                counters: x
                                    .versions
                                    .iter()
                                    .map(|x| items::Counter {
                                        id: x.0,
                                        value: x.1,
                                    })
                                    .collect::<Vec<_>>(),
                            }),
                            sequence: 1,
                            block_size: x.get_size() as i32,
                            blocks: x
                                .gen_blocks(&dir)?
                                .iter()
                                .map(|y| items::BlockInfo {
                                    offset: 0,
                                    size: y.size,
                                    hash: y.hash.clone(),
                                    weak_hash: 0,
                                })
                                .collect(),
                            symlink_target: "".to_string(),
                            blocks_hash: Vec::new(),
                            encrypted: Vec::new(),
                            platform: None,
                            local_flags: 0,
                            version_hash: Vec::new(),
                            inode_change_ns: 0,
                            encryption_trailer_size: 0,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                let index = items::Index {
                    folder: dir.id.clone(),
                    files,
                    last_sequence: 0,
                };
                log::info!("{}: Sending index: {:?}", self.get_name().await, index);
                self.submit_message(index.encode_for_bep()).await;
            }
            return Ok(());
        }
        Err(io::Error::other("Peer not yet received"))
    }

    pub async fn get_file(
        &self,
        directory: &SyncDirectory,
        sync_file: &SyncFile,
    ) -> Result<(), PeerCommandError> {
        let message_id = StdRng::from_os_rng().sample(StandardUniform);

        if sync_file.blocks.is_empty() {
            return Err(PeerCommandError::Other(
                "File object has no blocks".to_string(),
            ));
        } else if sync_file.blocks.len() != 1 {
            return Err(PeerCommandError::Other(
                "Multi-block files are unsupported :/".to_string(),
            ));
        }
        let block = &sync_file.blocks[0];
        let name = sync_file.get_name();

        // TODO: Support bigger files
        let message = items::Request {
            id: message_id,
            folder: directory.id.clone(),
            name,
            offset: 0,
            size: block.size,
            hash: block.hash.clone(),
            from_temporary: false,
            weak_hash: 0,
            block_no: 0,
        };

        log::info!("Submitting file request");

        let message = self
            .submit_request(
                message_id,
                PeerRequestResponseType::WhenResponse,
                message.encode_for_bep(),
            )
            .await?;

        log::info!("Got response");

        match message {
            PeerRequestResponse::Response(response) => {
                match items::ErrorCode::from_i32(response.code) {
                    Some(items::ErrorCode::NoError) => {
                        let hash = digest::digest(&digest::SHA256, &response.data);
                        if hash.as_ref() != sync_file.hash.clone() {
                            return Err(PeerCommandError::InvalidFile);
                        }

                        let file = directory.path.clone();
                        if file.is_none() {
                            return Err(PeerCommandError::Other(
                                "Requesting file from non-synced directory".to_string(),
                            ));
                        }
                        let mut file = file.unwrap();

                        file.push(sync_file.get_name());
                        log::info!("Writing to path {:?}", file);
                        let mut o = File::create(file)?;
                        o.write_all(response.data.as_slice())?;
                        let mut sync_file = sync_file.to_owned();
                        sync_file.synced_version = sync_file.get_index_version();
                        self.state.update_sync_file(directory, &sync_file).await;
                    }
                    Some(items::ErrorCode::NoSuchFile) => {
                        return Err(PeerCommandError::NoSuchFile);
                    }
                    Some(items::ErrorCode::InvalidFile) => {
                        return Err(PeerCommandError::InvalidFile);
                    }
                    Some(items::ErrorCode::Generic) => {
                        return Err(PeerCommandError::Other("Generic Error".to_string()));
                    }
                    None => {
                        return Err(PeerCommandError::Other(format!(
                            "Invalid error code received: {}",
                            response.code
                        )));
                    }
                }
            }
            PeerRequestResponse::Error(e) => {
                return Err(PeerCommandError::Other(format!(
                    "Received an error while getting file: {}",
                    e
                )));
            }
            PeerRequestResponse::Closed => {
                return Err(PeerCommandError::ConnectionClosed);
            }
            _ => {
                return Err(PeerCommandError::Other(
                    "Got error on file request, and I don't know how to handle errors.".to_string(),
                ));
            }
        }
        Ok(())
    }

    pub async fn get_directory(&self, directory: &SyncDirectory) -> Result<(), PeerCommandError> {
        log::info!(
            "{}: Syncing directory {}",
            self.get_name().await,
            directory.label
        );
        let index = directory.get_index(self.state.clone()).await;

        for file in &index {
            let mut path = directory.path.clone().ok_or(PeerCommandError::Other(
                "Requested get of non-synced directory".to_string(),
            ))?;
            path.push(&file.path);
            if !path.exists() || file.synced_version < file.get_index_version() {
                log::info!("{}: we want a file {:?}", self.get_name().await, file.path);
                self.get_file(directory, file).await?;
            }
        }
        self.state.clone().directory_changed(directory).await;
        log::info!("{}: Syncing complete", self.get_name().await);
        Ok(())
    }

    pub async fn get_name(&self) -> String {
        // TODO: Terrible
        let state_name = self.state.get_name().await;
        self.name.get_or_init(|| state_name).clone()
    }

    pub async fn submit_message(&self, msg: Vec<u8>) {
        let r = self.tx.send((msg, None)).await;
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.get_name().await,
                e
            );
        }
    }

    async fn submit_request(
        &self,
        id: i32,
        response_type: PeerRequestResponseType,
        msg: Vec<u8>,
    ) -> io::Result<PeerRequestResponse> {
        assert!(response_type != PeerRequestResponseType::None);
        let (tx, rx) = oneshot::channel();

        let mut first_part: Option<oneshot::Sender<PeerRequestResponse>> = None;

        if response_type == PeerRequestResponseType::WhenSent {
            first_part = Some(tx);
        } else {
            let request = PeerRequestListener {
                id,
                response_type,
                peer_connection: tx,
            };
            self.requests.write().unwrap().insert(id, request);
        };
        let r = self.tx.send((msg, first_part)).await;
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.get_name().await,
                e
            );
        }
        match rx.await {
            Ok(r) => Ok(r),
            Err(e) => {
                log::error!(
                    "{}: Got error while closing connection {}",
                    self.get_name().await,
                    e
                );
                Err(io::Error::other(
                    "Got an error while trying to close connection",
                ))
            }
        }
    }

    fn set_peer(&self, id: Vec<u8>) {
        self.peer_id.set(id).unwrap();
    }

    /// Get the peer this connection is to
    pub async fn get_peer(&self) -> Result<Peer, PeerCommandError> {
        let peers = self.state.get_peers().await;
        let peer_id = self
            .peer_id
            .as_ref()
            .get()
            .ok_or(PeerCommandError::UninitializedError)?
            .clone();
        // TODO: Check properly
        let peer = peers
            .into_iter()
            .find(|peer| &peer_id == peer.device_id.as_ref().unwrap_or(&vec![]));
        if peer.is_none() {
            let _ = self.close_reason("Unknown peer".to_string()).await;
            Err(PeerCommandError::ConnectionError(
                PeerConnectionError::UnknownPeer,
            ))
        } else {
            Ok(peer.unwrap())
        }
    }

    /// Returns Ok(()), except in the case that an error has occured
    fn has_error(&self) -> Result<(), PeerConnectionError> {
        match self.error.get() {
            Some(Some(e)) => Err(e.clone()),
            Some(None) => Ok(()),
            None => Ok(()),
        }
    }

    /// This future completes when the initial messages have been sent, or
    /// the connection has been closed due to an error.
    pub async fn wait_for_ready(&self) -> Result<(), PeerConnectionError> {
        let (tx, rx) = oneshot::channel();
        let send = self.tx.send(([].to_vec(), Some(tx))).await;
        if send.is_err() {
            self.has_error()?;
            return Err(PeerConnectionError::TimeoutError);
        }
        match rx.await {
            Ok(_) => Ok(()),
            Err(Canceled) => {
                // TODO: Horrible hack to ensure we get our error
                thread::sleep(time::Duration::from_millis(100));
                self.has_error()?;
                Err(PeerConnectionError::Other("Unknown error".to_string()))
            }
        }
    }

    /// Waits for the connection to close. Does not actually ask to close.
    pub async fn wait_for_close(&self) -> Result<(), PeerConnectionError> {
        self.task_tracker.wait().await;
        self.has_error()?;
        Ok(())
    }

    pub async fn directory_updated(&self, directory: &SyncDirectory) {
        let mut synced_index_updated = false;
        if let Ok(peer) = self.get_peer().await {
            if let Some(directory) = self.state.get_sync_directory(&directory.id.clone()).await {
                if self
                    .state
                    .is_directory_synced(directory.id, peer.id.unwrap())
                    .await
                {
                    synced_index_updated = true;
                }
            }
        }
        if synced_index_updated {
            self.send_index().await.unwrap();
        }
    }

    /// Sends a request to close. Does not wait for the connection to be closed.
    pub async fn close_reason(&self, reason: String) -> tokio::io::Result<()> {
        log::info!("{}: Connection close requested", self.get_name().await);
        let message = items::Close { reason }.encode_for_bep();
        self.submit_request(-1, PeerRequestResponseType::WhenSent, message)
            .await?;
        self.cancellation_token.cancel();
        Ok(())
    }

    /// Close the connection regularly
    pub async fn close(&self) -> tokio::io::Result<()> {
        self.close_reason("Exit by user".to_string()).await
    }

    pub async fn close_err(&self, err: &PeerConnectionError) -> tokio::io::Result<()> {
        match err {
            _ => self.close_reason("Unknown error".to_string()),
        }
        .await
    }

    pub async fn get_peer_name(&self) -> Result<String, PeerCommandError> {
        self.get_peer().await.map(|x| x.name)
    }

    pub fn watch(&mut self) {
        let pc_clone = self.clone();
        self.task_tracker.spawn(async move {
            if let Err(e) = watcher::watch(pc_clone.clone()).await {
                log::error!("{}: Watcher error: {}", pc_clone.get_name().await, e);
            }
            log::info!("{}: Shut down watcher", pc_clone.get_name().await);
        });
    }
}
