use super::items::{self, EncodableItem};
use crate::bep_state::BepState;
use crate::models::Peer;
use futures::channel::oneshot;
use log;
use prost::Message;
use ring::digest;
use std::collections::HashMap;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::time::SystemTime;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc::{channel, Receiver, Sender, UnboundedReceiver, UnboundedSender};

#[derive(Copy, Clone, PartialEq)]
pub enum PeerRequestResponseType {
    None,
    WhenClosed,
    WhenSent,
    WhenResponse,
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
    requests: Arc<Mutex<HashMap<i32, PeerRequestListener>>>,
    hello: Arc<Mutex<Option<items::Hello>>>,
    tx: Sender<(Option<oneshot::Sender<PeerRequestResponse>>, Vec<u8>)>,
    // Receiver for messages that should be sent
    // Should only be accessed by handle_writing
    rx: Arc<tokio::sync::Mutex<Receiver<(Option<oneshot::Sender<PeerRequestResponse>>, Vec<u8>)>>>,
    shutdown_send: UnboundedSender<()>,
    shutdown_recv: Arc<tokio::sync::Mutex<UnboundedReceiver<()>>>,
}

impl PeerConnectionInner {
    pub fn new(state: Arc<Mutex<BepState>>) -> Self {
        let (tx, rx) = channel(100);
        let (shutdown_send, shutdown_recv) = tokio::sync::mpsc::unbounded_channel::<()>();
        PeerConnectionInner {
            state,
            requests: Arc::new(Mutex::new(HashMap::new())),
            hello: Arc::new(Mutex::new(None)),
            tx,
            rx: Arc::new(tokio::sync::Mutex::new(rx)),
            shutdown_send,
            shutdown_recv: Arc::new(tokio::sync::Mutex::new(shutdown_recv)),
        }
    }

    pub fn get_name(&self) -> String {
        self.state.lock().unwrap().get_name()
    }

    pub fn get_peer_name(&self) -> Option<String> {
        self.hello
            .lock()
            .unwrap()
            .as_ref()
            .map(|x| x.device_name.clone())
    }

    pub async fn submit_message(&mut self, msg: Vec<u8>) {
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
        &mut self,
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
            self.requests.lock().unwrap().insert(id, request);
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
            Err(e) => Err(io::Error::new(
                io::ErrorKind::Other,
                "Got an error while trying to close connection",
            )),
        }
    }

    /// Get the peer this connection is to
    pub fn get_peer(&self) -> Option<Peer> {
        // TODO: Authenticate peers
        let peers = self.state.lock().unwrap().get_peers();
        let name = self
            .hello
            .as_ref()
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .device_name
            .clone();
        for peer in peers {
            if name == peer.name {
                return Some(peer);
            }
        }
        None
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
    stream: &mut ReadHalf<impl AsyncRead>,
    mut inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let request = receive_message!(items::Request, stream)?;
    log::info!("{}: Received request {:?}", inner.get_name(), request);

    let dir = inner
        .state
        .lock()
        .unwrap()
        .get_sync_directory(request.folder);

    let peer = inner.get_peer().unwrap();

    if dir.is_none()
        || !inner
            .state
            .lock()
            .unwrap()
            .is_directory_synced(dir.as_ref().unwrap(), &peer)
    {
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
    if let Some(peer_request) = inner.requests.lock().unwrap().remove(&response.id) {
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

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_reading(
    stream: &mut ReadHalf<impl AsyncRead>,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    loop {
        let header_len = tokio::time::timeout(Duration::from_millis(1000 * 3), stream.read_u16())
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
            handle_request(stream, innerclone).await?;
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
            return Err(io::Error::new(
                io::ErrorKind::ConnectionReset,
                "Connection was closed",
            ));
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown message type",
            ));
        }
    }
}

async fn handle_writing(
    mut wr: WriteHalf<impl AsyncWriteExt>,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let mut rx = inner.rx.lock().await;
    while let Some((tx, msg)) = rx.recv().await {
        log::info!("{}: Wrote message", inner.get_name());
        wr.write_all(&msg).await?;
        if let Some(tx) = tx {
            let r = tx.send(PeerRequestResponse::Sent);
            if let Err(_e) = r {
                log::error!("{}: Got error when sending response:", inner.get_name());
            }
        }
    }
    rx.close();
    Ok(())
}

/// Starts two tasks, one that sends data over TCP, and one that receives
/// If as a result of receiving a message, the server wants to transmit something,
/// it simply adds is to the tx queue
pub async fn handle_connection(
    mut stream: (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let hello = items::exchange_hellos(&mut stream, inner.get_name().to_string()).await?;
    log::info!(
        "{}: Received hello from {}",
        inner.get_name(),
        hello.client_name
    );
    *inner.hello.lock().unwrap() = Some(hello);
    let (mut rd, wr) = tokio::io::split(stream);

    // TODO: More graceful shutdown that abort
    let name = inner.get_name();
    let sendclone = inner.shutdown_send.clone();
    let innerclone = inner.clone();
    let s1 = tokio::spawn(async move {
        let r = handle_reading(&mut rd, innerclone).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from reader: {}", name, e);
        }
        let _r = sendclone.send(());
        r
    });

    let sendclone = inner.shutdown_send.clone();
    let innerclone = inner.clone();
    let name = inner.get_name();
    let s2 = tokio::spawn(async move {
        let r = handle_writing(wr, innerclone).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from writer: {}", name, e);
        }
        let _r = sendclone.send(());
        r
    });
    let mut shutdown_recv = inner.shutdown_recv.lock().await;
    shutdown_recv.recv().await;
    shutdown_recv.close();
    s1.abort();
    s2.abort();

    let mut rx = inner.rx.lock().await;
    rx.close();
    while let Some((txo, _msg)) = rx.recv().await {
        if let Some(tx) = txo {
            tx.send(PeerRequestResponse::Closed);
        }
    }

    log::info!("{}: Shutting down server", inner.get_name());

    {
        let mut requests = inner.requests.lock().unwrap();
        for (_key, channel) in requests.drain() {
            let s = channel.inner.send(PeerRequestResponse::Closed);
            assert!(s.is_ok());
        }
    }
    assert!(inner.requests.lock().unwrap().is_empty());
    log::info!("Responded to all unhandled requests");

    Ok(())
}
