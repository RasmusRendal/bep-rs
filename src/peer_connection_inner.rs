use super::items;
use futures::channel::oneshot;
use log;
use prost::Message;
use std::collections::HashMap;
use std::io;
use std::sync::mpsc::channel;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};

#[derive(Clone, PartialEq)]
pub enum PeerRequestResponseType {
    None,
    WhenSent,
    WhenClosed,
    WhenResponse,
}

/// Encapsulates something that the peer connection should
/// transmit over TCP. The optional `inner` lets you
/// encapsulate a channel, over which you would like to be
/// notified when a response is sent.
pub struct PeerRequest {
    pub id: i32,
    pub response_type: PeerRequestResponseType,
    pub msg: Vec<u8>,
    pub inner: Option<oneshot::Sender<items::Response>>,
}

/// Encapsulates the "inner" state of the PeerConnection
#[derive(Clone)]
pub struct PeerConnectionInner {
    name: Arc<String>,
    requests: Arc<Mutex<HashMap<i32, PeerRequest>>>,
    tx: Sender<PeerRequest>,
    rx: Arc<Mutex<Receiver<PeerRequest>>>,
}

impl PeerConnectionInner {
    pub fn new(name: String) -> Self {
        let (tx, rx) = channel();
        PeerConnectionInner {
            name: Arc::new(name),
            requests: Arc::new(Mutex::new(HashMap::new())),
            tx,
            rx: Arc::new(Mutex::new(rx)),
        }
    }

    pub fn submit_message(&mut self, msg: Vec<u8>) {
        let request = PeerRequest {
            id: -1,
            response_type: PeerRequestResponseType::None,
            msg,
            inner: None,
        };
        let r = self.tx.send(request);
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.name,
                e
            );
        }
    }

    pub fn submit_request(
        &mut self,
        id: i32,
        response_type: PeerRequestResponseType,
        msg: Vec<u8>,
    ) -> oneshot::Receiver<items::Response> {
        let (tx, rx) = oneshot::channel();
        let request = PeerRequest {
            id,
            response_type,
            msg,
            inner: Some(tx),
        };
        let r = self.tx.send(request);
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.name,
                e
            );
        }
        rx
    }
}

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
pub async fn handle_request(
    stream: &mut ReadHalf<impl AsyncReadExt>,
    mut inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let request = receive_message!(items::Request, stream)?;
    log::info!("{}: Received request {:?}", inner.name, request);
    let response = if request.name == "testfile" {
        items::Response {
            id: request.id,
            data: "hello world".to_string().into_bytes(),
            code: 0,
        }
    } else {
        items::Response {
            id: request.id,
            data: vec![],
            code: 2,
        }
    };
    log::info!("{}: Sending response {:?}", inner.name, response);
    let header = items::Header {
        r#type: 4,
        compression: 0,
    };
    let mut message_buffer: Vec<u8> = Vec::new();
    message_buffer.extend_from_slice(&u16::to_be_bytes(header.encoded_len() as u16));
    message_buffer.append(&mut header.encode_to_vec());
    message_buffer.extend_from_slice(&u32::to_be_bytes(response.encoded_len() as u32));
    message_buffer.append(&mut response.encode_to_vec());
    inner.submit_message(message_buffer);

    log::info!("{}: Finished handling request", inner.name);
    Ok(())
}

async fn handle_response(
    stream: &mut ReadHalf<impl AsyncReadExt>,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let response = receive_message!(items::Response, stream)?;
    log::info!(
        "{}: Received response for request {}",
        inner.name,
        response.id
    );
    if let Some(peer_request) = inner.requests.lock().unwrap().remove(&response.id) {
        assert!(peer_request.inner.is_some());
        let r = peer_request.inner.unwrap().send(response);
        assert!(r.is_ok());
    }
    Ok(())
}

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_reading(
    stream: &mut ReadHalf<impl AsyncReadExt>,
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
            log::info!("{}: Handling request", inner.name);
            let innerclone = inner.clone();
            handle_request(stream, innerclone).await?;
        } else if header.r#type == items::MessageType::Response as i32 {
            let innerclone = inner.clone();
            handle_response(stream, innerclone).await?;
        } else if header.r#type == items::MessageType::Close as i32 {
            let close = receive_message!(items::Close, stream)?;
            log::info!(
                "{}: Peer requested close. Reason: {}",
                inner.name,
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
    loop {
        let mut content: Option<Vec<u8>> = None;
        let mut close = false;
        {
            if let Ok(msg) = inner.rx.try_lock().map(|l| l.try_recv()) {
                if msg.is_err() {
                    continue;
                }
                let msg = msg.unwrap();

                content = Some(msg.msg.clone());
                if msg.response_type == PeerRequestResponseType::WhenSent {
                    let r = items::Response {
                        id: -1,
                        data: "hello world".to_string().into_bytes(),
                        code: 0,
                    };
                    let m = msg.inner.unwrap().send(r);
                    assert!(m.is_ok());
                } else if msg.response_type == PeerRequestResponseType::WhenClosed {
                    close = true;
                } else if msg.response_type == PeerRequestResponseType::WhenResponse {
                    if msg.inner.is_some() {
                        if let Ok(mut l) = inner.requests.lock() {
                            l.insert(msg.id, msg);
                        }
                    }
                }
            }
        }
        if let Some(msg) = content {
            wr.write_all(&msg).await?;
        }
        if close {
            wr.shutdown().await?;
            return Ok(());
        }
    }
}

/// Starts two tasks, one that sends data over TCP, and one that receives
/// If as a result of receiving a message, the server wants to transmit something,
/// it simply adds is to the tx queue
pub async fn handle_connection(
    mut stream: (impl AsyncWriteExt + AsyncReadExt + Unpin + std::marker::Send + 'static),
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let hello = items::exchange_hellos(&mut stream).await?;
    log::info!("{}: Received hello from {}", inner.name, hello.client_name);
    let (mut rd, wr) = tokio::io::split(stream);

    // TODO: More graceful shutdown that abort
    let (shutdown_send, mut shutdown_recv) = tokio::sync::mpsc::unbounded_channel::<()>();

    let sendclone = shutdown_send.clone();
    let innerclone = inner.clone();
    let s1 = tokio::spawn(async move {
        handle_reading(&mut rd, innerclone).await;
        sendclone.send(());
    });

    let sendclone = shutdown_send.clone();
    let innerclone = inner.clone();
    let s2 = tokio::spawn(async move {
        handle_writing(wr, innerclone).await;
        sendclone.send(());
    });
    shutdown_recv.recv().await;
    s1.abort();
    s2.abort();
    log::info!("{}: Shutting down server", inner.name);

    let keys: Vec<i32> = inner
        .requests
        .lock()
        .map(|x| x.keys().copied().collect())
        .unwrap();

    let mut l = inner.requests.lock().unwrap();
    for k in keys {
        let r = l.remove(&k).unwrap();
        let response = items::Response {
            id: r.id,
            data: "hello world".to_string().into_bytes(),
            code: 0,
        };
        let s = r.inner.unwrap().send(response);
        assert!(s.is_ok());
    }
    Ok(())
}
