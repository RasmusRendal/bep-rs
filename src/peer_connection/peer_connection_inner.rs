use super::items;
use futures::channel::oneshot;
use log;
use prost::Message;
use ring::digest;
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc::{
    channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender,
};

#[derive(Clone, PartialEq)]
pub enum PeerRequestResponseType {
    None,
    WhenClosed,
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
    pub name: Arc<String>,
    requests: Arc<Mutex<HashMap<i32, PeerRequestListener>>>,
    hello: Arc<Mutex<Option<items::Hello>>>,
    tx: Sender<Vec<u8>>,
    // Receiver for messages that should be sent
    // Should only be accessed by handle_writing
    rx: Arc<tokio::sync::Mutex<Receiver<Vec<u8>>>>,
    shutdown_send: UnboundedSender<()>,
    shutdown_recv: Arc<tokio::sync::Mutex<UnboundedReceiver<()>>>,
}

impl PeerConnectionInner {
    pub fn new(name: String) -> Self {
        let (tx, rx) = channel(100);
        let (shutdown_send, shutdown_recv) = tokio::sync::mpsc::unbounded_channel::<()>();
        PeerConnectionInner {
            name: Arc::new(name),
            requests: Arc::new(Mutex::new(HashMap::new())),
            hello: Arc::new(Mutex::new(None)),
            tx,
            rx: Arc::new(tokio::sync::Mutex::new(rx)),
            shutdown_send,
            shutdown_recv: Arc::new(tokio::sync::Mutex::new(shutdown_recv)),
        }
    }

    pub fn get_peer_name(&self) -> Option<String> {
        self.hello
            .lock()
            .unwrap()
            .as_ref()
            .map(|x| x.device_name.clone())
    }

    pub async fn submit_message(&mut self, msg: Vec<u8>) {
        let r = self.tx.send(msg).await;
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.name,
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
        let request = PeerRequestListener {
            id,
            response_type,
            inner: tx,
        };
        self.requests.lock().unwrap().insert(id, request);

        let r = self.tx.send(msg).await;
        if let Err(e) = r {
            log::error!(
                "{}: Tried to submit a request after server was closed: {}",
                self.name,
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

    pub async fn close(&mut self) -> io::Result<PeerRequestResponse> {
        log::info!("closing");
        let header = items::Header {
            r#type: items::MessageType::Close as i32,
            compression: items::MessageCompression::r#None as i32,
        };
        let message = items::Close {
            reason: "Exit by user".to_string(),
        };
        let mut message_buffer: Vec<u8> = Vec::new();
        message_buffer.extend_from_slice(&u16::to_be_bytes(header.encoded_len() as u16));
        message_buffer.append(&mut header.encode_to_vec());
        message_buffer.extend_from_slice(&u32::to_be_bytes(message.encoded_len() as u32));
        message_buffer.append(&mut message.encode_to_vec());
        self.shutdown_send.send(());
        self.submit_request(-1, PeerRequestResponseType::WhenClosed, message_buffer)
            .await
    }
}

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
pub async fn handle_request(
    stream: &mut ReadHalf<impl AsyncRead>,
    mut inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let request = receive_message!(items::Request, stream)?;
    log::info!("{}: Received request {:?}", inner.name, request);
    let data = "hello world".to_string().into_bytes();
    let hash = digest::digest(&digest::SHA256, &data);
    let response = if hash.as_ref() != request.hash {
        items::Response {
            id: request.id,
            data,
            code: items::ErrorCode::InvalidFile as i32,
        }
    } else if request.name == "testfile" {
        items::Response {
            id: request.id,
            data,
            code: items::ErrorCode::NoError as i32,
        }
    } else {
        items::Response {
            id: request.id,
            data: vec![],
            code: items::ErrorCode::NoSuchFile as i32,
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
    inner.submit_message(message_buffer).await;

    log::info!("{}: Finished handling request", inner.name);
    Ok(())
}

async fn handle_response(
    stream: &mut ReadHalf<impl AsyncRead>,
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let response = receive_message!(items::Response, stream)?;
    log::info!(
        "{}: Received response for request {}",
        inner.name,
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
    let mut rx = inner.rx.lock().await;
    while let Some(msg) = rx.recv().await {
        log::info!("{}: Wrote message", inner.name);
        wr.write_all(&msg).await?;
    }
    Ok(())
}

/// Starts two tasks, one that sends data over TCP, and one that receives
/// If as a result of receiving a message, the server wants to transmit something,
/// it simply adds is to the tx queue
pub async fn handle_connection(
    mut stream: (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    inner: PeerConnectionInner,
) -> tokio::io::Result<()> {
    let hello = items::exchange_hellos(&mut stream, inner.name.to_string()).await?;
    log::info!("{}: Received hello from {}", inner.name, hello.client_name);
    *inner.hello.lock().unwrap() = Some(hello);
    let (mut rd, wr) = tokio::io::split(stream);

    // TODO: More graceful shutdown that abort
    let name = inner.name.clone();
    let sendclone = inner.shutdown_send.clone();
    let innerclone = inner.clone();
    let s1 = tokio::spawn(async move {
        let r = handle_reading(&mut rd, innerclone).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from reader: {}", name, e);
        }
        sendclone.send(());
        r
    });

    let sendclone = inner.shutdown_send.clone();
    let innerclone = inner.clone();
    let name = inner.name.clone();
    let s2 = tokio::spawn(async move {
        let r = handle_writing(wr, innerclone).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from writer: {}", name, e);
        }

        sendclone.send(());
        r
    });
    inner.shutdown_recv.lock().await.recv().await;
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
        let s = r.inner.send(PeerRequestResponse::Closed);
        assert!(s.is_ok());
    }
    Ok(())
}
