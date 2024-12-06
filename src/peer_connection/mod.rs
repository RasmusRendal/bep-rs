#[macro_use]
mod items;
mod handlers;
mod verifier;
use crate::bep_state::BepState;
use crate::models::Peer;
use crate::sync_directory::{SyncDirectory, SyncFile};
use handlers::handle_connection;

use futures::channel::oneshot;
use items::EncodableItem;
use log;
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ring::digest;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Error, ErrorKind, Write};
use std::sync::{Arc, Mutex, RwLock};
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{channel, Sender, UnboundedSender};

#[derive(Copy, Clone, PartialEq)]
pub enum PeerRequestResponseType {
    None,
    WhenSent,
    WhenResponse,
    WhenClosed,
}

pub enum PeerRequestResponse {
    Response(items::Response),
    Error(String),
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
    state: Arc<Mutex<BepState>>,
    // Handles for pending requests that need to be answered
    requests: Arc<RwLock<HashMap<i32, PeerRequestListener>>>,
    // A channel for sending messages that should be sent to the peer
    // Each message may optionally include a oneshot sender, if we need
    // to know when the message has been sent.
    tx: Sender<(Vec<u8>, Option<oneshot::Sender<PeerRequestResponse>>)>,
    // A channel to activate, to shut down the connection
    shutdown_send: UnboundedSender<()>,

    // Hack for semi-constants. But these are sometimes acquired
    // from the peer, and the connection object is initialized synchronously.
    peer_id: Arc<RwLock<Vec<u8>>>,
    name: Arc<RwLock<String>>,
}

impl PeerConnection {
    pub fn new(
        socket: (impl AsyncWrite + AsyncRead + Unpin + Send + 'static),
        state: Arc<Mutex<BepState>>,
        connector: bool,
    ) -> Self {
        let (tx, rx) = channel(100);
        let (shutdown_send, shutdown_recv) = tokio::sync::mpsc::unbounded_channel::<()>();
        let peer_connection = PeerConnection {
            state: state.clone(),
            requests: Arc::new(RwLock::new(HashMap::new())),
            tx,
            shutdown_send,
            peer_id: Arc::new(RwLock::new(vec![])),
            name: Arc::new(RwLock::new("".to_string())),
        };
        let peer_connectionc = peer_connection.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_connection(
                socket,
                peer_connection.clone(),
                rx,
                shutdown_recv,
                connector,
            )
            .await
            {
                log::error!(
                    "{}: Error occured in client {}",
                    peer_connection.get_name(),
                    e
                );
            }
        });

        state
            .as_ref()
            .lock()
            .unwrap()
            .listeners
            .push(peer_connectionc.clone());
        peer_connectionc
    }

    pub async fn send_index(&self) -> io::Result<()> {
        if let Some(peer) = self.get_peer() {
            let directories = self.state.lock().unwrap().get_sync_directories();
            for dir in directories {
                if !self.state.lock().unwrap().is_directory_synced(&dir, &peer) {
                    continue;
                }
                let index = items::Index {
                    folder: dir.id.clone(),
                    files: dir
                        .generate_index(&mut self.state.as_ref().lock().unwrap())
                        .iter()
                        .map(|x| items::FileInfo {
                            name: x.get_name(&dir),
                            r#type: items::FileInfoType::File as i32,
                            size: x.get_size() as i64,
                            permissions: 0,
                            modified_s: x
                                .path
                                .metadata()
                                .unwrap()
                                .modified()
                                .unwrap()
                                .duration_since(SystemTime::UNIX_EPOCH)
                                .unwrap()
                                .as_secs() as i64,
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
                                .get_blocks()
                                .iter()
                                .map(|y| items::BlockInfo {
                                    offset: 0,
                                    size: y.size as i32,
                                    hash: y.hash.clone(),
                                    weak_hash: 0,
                                })
                                .collect(),
                            symlink_target: "".to_string(),
                        })
                        .collect(),
                };
                log::info!("{}: Sending index: {:?}", self.get_name(), index);
                self.submit_message(index.encode_for_bep()).await;
            }
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Peer not yet received",
        ))
    }

    pub async fn get_file(
        &self,
        directory: &SyncDirectory,
        sync_file: &SyncFile,
    ) -> tokio::io::Result<()> {
        let message_id = StdRng::from_entropy().sample(Standard);

        let name = sync_file.get_name(directory);

        // TODO: Support bigger files
        let message = items::Request {
            id: message_id,
            folder: directory.id.clone(),
            name,
            offset: 0,
            size: 8,
            hash: sync_file.hash.clone(),
            from_temporary: false,
        };

        let message = self
            .submit_request(
                message_id,
                PeerRequestResponseType::WhenResponse,
                message.encode_for_bep(),
            )
            .await?;

        match message {
            PeerRequestResponse::Response(response) => {
                if response.code != items::ErrorCode::NoError as i32 {
                    log::error!("Error code when requesting data: {}", response.code);
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        "Got an error when requesting data",
                    ));
                }
                let hash = digest::digest(&digest::SHA256, &response.data);
                if hash.as_ref() != sync_file.hash.clone() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Received file does not correspond to requested hash",
                    ));
                }

                let mut file = directory.path.clone();
                file.push(sync_file.get_name(directory));
                log::info!("Writing to path {:?}", file);
                let mut o = File::create(file)?;
                o.write_all(response.data.as_slice())?;
                let mut sync_file = sync_file.to_owned();
                sync_file.synced_version = sync_file.get_index_version();
                self.state
                    .lock()
                    .unwrap()
                    .update_sync_file(directory, &sync_file);
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Got error on file request, and I don't know how to handle errors.",
                ));
            }
        }
        Ok(())
    }

    pub async fn get_directory(&self, directory: &SyncDirectory) -> io::Result<()> {
        log::info!("{}: Syncing directory {}", self.get_name(), directory.label);
        let index = directory.generate_index(&mut self.state.as_ref().lock().unwrap());

        for file in &index {
            if file.synced_version < file.get_index_version() {
                self.get_file(directory, file).await?;
            }
        }
        directory.generate_index(&mut self.state.as_ref().lock().unwrap());
        log::info!("{}: Syncing complete", self.get_name());
        Ok(())
    }

    pub fn get_name(&self) -> String {
        if self.name.read().unwrap().is_empty() {
            *self.name.write().unwrap() = self.state.lock().unwrap().get_name();
        }
        self.name.read().unwrap().clone()
    }

    pub async fn submit_message(&self, msg: Vec<u8>) {
        let r = self.tx.send((msg, None)).await;
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.get_name(),
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
                self.get_name(),
                e
            );
        }
        match rx.await {
            Ok(r) => Ok(r),
            Err(e) => {
                log::error!(
                    "{}: Got error while closing connection {}",
                    self.get_name(),
                    e
                );
                Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Got an error while trying to close connection",
                ))
            }
        }
    }

    fn set_peer(&self, id: Vec<u8>) {
        *self.peer_id.write().unwrap() = id;
    }

    /// Get the peer this connection is to
    pub fn get_peer(&self) -> Option<Peer> {
        let peers = self.state.lock().unwrap().get_peers();
        let peer_id = self.peer_id.as_ref().read().unwrap().clone();
        // TODO: Check properly
        peers
            .into_iter()
            .find(|peer| &peer_id == peer.device_id.as_ref().unwrap())
    }

    pub async fn wait_for_close(&mut self) -> io::Result<()> {
        if self.shutdown_send.is_closed() || self.tx.is_closed() {
            log::info!("already shut");
            return Ok(());
        }
        let (tx, rx) = oneshot::channel();
        let id = StdRng::from_entropy().sample(Standard);
        self.requests.write().unwrap().insert(
            id,
            PeerRequestListener {
                id,
                response_type: PeerRequestResponseType::WhenClosed,
                peer_connection: tx,
            },
        );
        if let Err(e) = rx.await {
            return Err(Error::new(ErrorKind::Other, e));
        }
        Ok(())
    }

    pub async fn directory_updated(&mut self, directory: &SyncDirectory) {
        log::info!("sending");
        let mut synced_index_updated = false;
        if let Some(peer) = self.get_peer() {
            let mut state = self.state.lock().unwrap();
            if let Some(directory) = state.get_sync_directory(&directory.id.clone()) {
                if state.is_directory_synced(&directory, &peer) {
                    synced_index_updated = true;
                }
            }
        }
        if synced_index_updated {
            self.send_index().await.unwrap();
        }
    }

    pub async fn close(&mut self) -> tokio::io::Result<()> {
        log::info!("{}: Connection close requested", self.get_name());
        if self.shutdown_send.is_closed() || self.tx.is_closed() {
            log::info!("already shut");
            return Ok(());
        }
        let message = items::Close {
            reason: "Exit by user".to_string(),
        }
        .encode_for_bep();
        log::info!("submitted close");
        self.submit_request(-1, PeerRequestResponseType::WhenSent, message)
            .await?;
        log::info!("done waiting");

        // We ignore errors here.
        // If the shutdown sender has an error, it's because it's because our connection
        // is already (in the process) of being closed
        let _ = self.shutdown_send.send(());

        Ok(())
    }

    pub fn get_peer_name(&self) -> Option<String> {
        self.get_peer().map(|x| x.name)
    }
}
