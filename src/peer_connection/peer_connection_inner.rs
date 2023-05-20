use super::items::{self, EncodableItem};
use super::verifier::verify_connection;
use crate::bep_state::BepState;
use crate::models::Peer;
use crate::sync_directory::{SyncDirectory, SyncFile};
use futures::channel::oneshot;
use log;
use prost::Message;
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ring::digest;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc::{channel, Receiver, Sender, UnboundedReceiver, UnboundedSender};

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
    pub inner: oneshot::Sender<PeerRequestResponse>,
}

/// Encapsulates the "inner" state of the PeerConnection
#[derive(Clone)]
pub struct PeerConnectionInner {
    state: Arc<Mutex<BepState>>,
    // Handles for pending requests that need to be answered
    requests: Arc<RwLock<HashMap<i32, PeerRequestListener>>>,
    tx: Sender<(Option<oneshot::Sender<PeerRequestResponse>>, Vec<u8>)>,
    shutdown_send: UnboundedSender<()>,
    peer_id: Arc<RwLock<Vec<u8>>>,
    name: Arc<RwLock<String>>,
}

impl PeerConnectionInner {
    pub fn new(
        state: Arc<Mutex<BepState>>,
        socket: (impl AsyncWrite + AsyncRead + Unpin + Send + 'static),
        connector: bool,
    ) -> Self {
        let (tx, rx) = channel(100);
        let (shutdown_send, shutdown_recv) = tokio::sync::mpsc::unbounded_channel::<()>();
        let inner = PeerConnectionInner {
            state,
            requests: Arc::new(RwLock::new(HashMap::new())),
            tx,
            shutdown_send,
            peer_id: Arc::new(RwLock::new(vec![])),
            name: Arc::new(RwLock::new("".to_string())),
        };
        let innerc = inner.clone();
        tokio::spawn(async move {
            if let Err(e) =
                handle_connection(socket, inner.clone(), rx, shutdown_recv, connector).await
            {
                log::error!("{}: Error occured in client {}", inner.get_name(), e);
            }
        });
        innerc
    }

    pub async fn directory_updated(&self, directory: String) {
        let mut synced_index_updated = false;
        if let Some(peer) = self.get_peer() {
            let mut state = self.state.lock().unwrap();
            if let Some(directory) = state.get_sync_directory(&directory) {
                if state.is_directory_synced(&directory, &peer) {
                    synced_index_updated = true;
                }
            }
        }

        if synced_index_updated {
            self.send_index().await;
        }
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
        let r = self.tx.send((None, msg)).await;
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.get_name(),
                e
            );
        }
    }

    pub async fn submit_request(
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
                inner: tx,
            };
            self.requests.write().unwrap().insert(id, request);
        };
        let r = self.tx.send((first_part, msg)).await;
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
                inner: tx,
            },
        );
        rx.await;
        Ok(())
    }

    pub async fn close(&mut self) -> io::Result<PeerRequestResponse> {
        log::info!("submitted close");
        if self.shutdown_send.is_closed() || self.tx.is_closed() {
            log::info!("already shut");
            return Ok(PeerRequestResponse::Closed);
        }
        let message = items::Close {
            reason: "Exit by user".to_string(),
        }
        .encode_for_bep();
        log::info!("submitted close");
        self.submit_request(-1, PeerRequestResponseType::WhenSent, message)
            .await?;
        log::info!("done waiting");
        self.shutdown_send.send(());
        Ok(PeerRequestResponse::Closed)
    }
}

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
pub async fn handle_request(
    request: items::Request,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    log::info!("{}: Received request {:?}", inner.get_name(), request);

    let dir = inner
        .state
        .lock()
        .unwrap()
        .get_sync_directory(&request.folder);

    let peer = inner.get_peer().unwrap();

    if dir.is_none()
        || !inner
            .state
            .lock()
            .unwrap()
            .is_directory_synced(dir.as_ref().unwrap(), &peer)
    {
        log::error!("Peer requested file, but it does not exist");
        let response = items::Response {
            id: request.id,
            data: vec![],
            code: items::ErrorCode::InvalidFile as i32,
        };
        inner.submit_message(response.encode_for_bep()).await;
        return Ok(());
    }

    let dir = dir.unwrap();
    let mut path = dir.path.clone();
    path.push(request.name.clone());
    let file = File::open(path).unwrap();
    let mut buf_reader = BufReader::new(file);
    let mut data = Vec::new();
    buf_reader.read_to_end(&mut data)?;

    let hash = digest::digest(&digest::SHA256, &data);
    let response = if hash.as_ref() != request.hash {
        log::error!("Request hash: {:?}", request.hash);
        log::error!("Real hash {:?}", hash.as_ref());
        log::error!("Peer requested file, but with invalid hash");
        items::Response {
            id: request.id,
            data,
            code: items::ErrorCode::InvalidFile as i32,
        }
    } else {
        items::Response {
            id: request.id,
            data,
            code: items::ErrorCode::NoError as i32,
        }
    };
    log::info!("{}: Sending response {:?}", inner.get_name(), response);
    inner.submit_message(response.encode_for_bep()).await;

    log::info!("{}: Finished handling request", inner.get_name());
    Ok(())
}

async fn handle_response(
    stream: &mut ReadHalf<impl AsyncRead>,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let response = receive_message!(items::Response, stream)?;
    log::info!(
        "{}: Received response for request {}",
        inner.get_name(),
        response.id
    );
    if let Some(peer_request) = inner.requests.write().unwrap().remove(&response.id) {
        assert!(peer_request.response_type == PeerRequestResponseType::WhenResponse);
        let r = peer_request
            .inner
            .send(PeerRequestResponse::Response(response));
        assert!(r.is_ok());
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            "Received unsolicited response",
        ))
    }
}

async fn handle_index(index: items::Index, inner: PeerConnectionInner) -> tokio::io::Result<()> {
    log::info!("{}: Handling index", inner.get_name());
    let syncdir = inner
        .state
        .lock()
        .unwrap()
        .get_sync_directory(&index.folder)
        .unwrap();
    let mut localindex = syncdir.generate_index(&mut inner.state.as_ref().lock().unwrap());
    let mut updated = false;
    for file in index.files {
        log::info!("{}: Handling index file {}", inner.get_name(), file.name);
        let localfile = localindex
            .iter_mut()
            .find(|x| x.get_name(&syncdir) == file.name);
        if localfile.is_some() {
            let localfile: &mut SyncFile = localfile.unwrap();
            if localfile.versions.len() >= file.version.as_ref().unwrap().counters.len() {
                continue;
            }
            log::info!("{}: Updating file {}", inner.get_name(), file.name);
            localfile.versions = file
                .version
                .unwrap()
                .counters
                .into_iter()
                .map(|x| (x.id, x.value))
                .collect();
            localfile.hash = file.blocks[0].hash.clone();
            localfile.modified_by = file.modified_by;
            inner
                .state
                .lock()
                .unwrap()
                .update_sync_file(&syncdir, &localfile);
            updated = true;
        } else {
            log::info!("{}: New file {}", inner.get_name(), file.name);
            let mut path = syncdir.path.clone();
            path.push(file.name.clone());
            let syncfile = SyncFile {
                id: None,
                path,
                hash: file.blocks[0].hash.clone(),
                modified_by: file.modified_by,
                synced_version: 0,
                versions: file
                    .version
                    .unwrap()
                    .counters
                    .into_iter()
                    .map(|x| (x.id, x.value))
                    .collect(),
            };
            inner
                .state
                .lock()
                .unwrap()
                .update_sync_file(&syncdir, &syncfile);
            updated = true;
        }
    }
    log::info!("{}: Index merged", inner.get_name());
    inner.get_directory(&syncdir).await?;

    Ok(())
}

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_reading(
    stream: &mut ReadHalf<impl AsyncRead>,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    loop {
        let header_len = tokio::time::timeout(Duration::from_millis(1000 * 30), stream.read_u16())
            .await?? as usize;
        let mut header = vec![0u8; header_len];
        stream.read_exact(&mut header).await?;
        let header = items::Header::decode(&*header)?;
        if header.compression != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Peer is trying to use compression, but we do not support it",
            ));
        }
        if header.r#type == items::MessageType::Request as i32 {
            log::info!("{}: Handling request", inner.get_name());
            let innerclone = inner.clone();
            let request = receive_message!(items::Request, stream)?;
            tokio::spawn(async move {
                if let Err(e) = handle_request(request, innerclone).await {
                    log::error!("Received an error when handling request: {}", e);
                }
            });
        } else if header.r#type == items::MessageType::Index as i32 {
            let index = receive_message!(items::Index, stream)?;
            let innerc = inner.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_index(index, innerc).await {
                    log::error!("Received an error when handling index: {}", e);
                }
            });
        } else if header.r#type == items::MessageType::Response as i32 {
            let innerclone = inner.clone();
            handle_response(stream, innerclone).await?;
        } else if header.r#type == items::MessageType::Close as i32 {
            let close = receive_message!(items::Close, stream)?;
            log::info!(
                "{}: Peer requested close. Reason: {}",
                inner.get_name(),
                close.reason
            );
            return Ok(());
        } else {
            log::error!(
                "{}: Got unknown message type {}",
                inner.get_name(),
                header.r#type
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown message type",
            ));
        }
    }
}

async fn close_receiver(mut rx: Receiver<(Option<oneshot::Sender<PeerRequestResponse>>, Vec<u8>)>) {
    while let Some((txo, _msg)) = rx.recv().await {
        if let Some(tx) = txo {
            tx.send(PeerRequestResponse::Sent);
        }
    }
    rx.close();
}

async fn handle_writing(
    mut wr: WriteHalf<impl AsyncWriteExt>,
    inner: PeerConnectionInner,
    mut rx: Receiver<(Option<oneshot::Sender<PeerRequestResponse>>, Vec<u8>)>,
) -> tokio::io::Result<()> {
    while let Some((tx, msg)) = rx.recv().await {
        if let Err(e) = wr.write_all(&msg).await {
            close_receiver(rx).await;
            return Err(e);
        }
        log::info!("{}: Wrote message", inner.get_name());
        if let Some(tx) = tx {
            let r = tx.send(PeerRequestResponse::Sent);
            if let Err(_e) = r {
                log::error!("{}: Got error when sending response:", inner.get_name());
            }
        }
    }
    close_receiver(rx).await;
    Ok(())
}

pub async fn handle_hello(
    stream: &mut (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    inner: &PeerConnectionInner,
) -> io::Result<()> {
    let hello = items::exchange_hellos(stream, inner.get_name().to_string()).await?;
    log::info!(
        "{}: Received hello from {}",
        inner.get_name(),
        hello.client_name
    );
    Ok(())
}

/// Starts two tasks, one that sends data over TCP, and one that receives
/// If as a result of receiving a message, the server wants to transmit something,
/// it simply adds is to the tx queue
pub async fn handle_connection(
    mut stream: (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    inner: PeerConnectionInner,
    rx: Receiver<(Option<oneshot::Sender<PeerRequestResponse>>, Vec<u8>)>,
    mut shutdown_recv: UnboundedReceiver<()>,
    connector: bool,
) -> tokio::io::Result<()> {
    handle_hello(&mut stream, &inner).await?;

    let peerids = inner
        .state
        .lock()
        .unwrap()
        .get_peers()
        .into_iter()
        .map(|x| x.device_id.unwrap_or(vec![]).clone())
        .collect();
    let certificate =
        tokio_rustls::rustls::Certificate(inner.state.lock().unwrap().get_certificate());
    let key = tokio_rustls::rustls::PrivateKey(inner.state.lock().unwrap().get_key());

    let (tcpstream, peer_id) =
        verify_connection(stream, certificate, key, peerids, connector).await?;
    inner.set_peer(peer_id);
    let (mut rd, wr) = tokio::io::split(tcpstream);
    inner.send_index().await?;

    let name = inner.get_name();
    let sendclone = inner.shutdown_send.clone();
    let innerclone = inner.clone();
    tokio::spawn(async move {
        let r = handle_reading(&mut rd, innerclone.clone()).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from reader: {}", name, e);
        }
        let _r = sendclone.send(());
        r
    });

    let sendclone = inner.shutdown_send.clone();
    let innerclone = inner.clone();
    let name = inner.get_name();
    tokio::spawn(async move {
        let r = handle_writing(wr, innerclone, rx).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from writer: {}", name, e);
        }
        let _r = sendclone.send(());
        r
    });

    shutdown_recv.recv().await;
    shutdown_recv.close();

    // Remove from list of listeners
    inner
        .state
        .lock()
        .unwrap()
        .listeners
        .retain(|x| !x.inner.tx.same_channel(&inner.tx));

    log::info!("{}: Shutting down server", inner.get_name());

    {
        let mut requests = inner.requests.write().unwrap();
        for (_key, channel) in requests.drain() {
            let s = channel.inner.send(PeerRequestResponse::Closed);
            assert!(s.is_ok());
        }
    }
    assert!(inner.requests.read().unwrap().is_empty());
    log::info!("Responded to all unhandled requests");

    Ok(())
}
