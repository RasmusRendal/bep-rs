use super::bep_state;
use super::items;
use futures::channel::oneshot;
use log;
use prost::Message;
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};

// TODO: When this rust PR lands
// https://github.com/rust-lang/rust/issues/63063
//type Stream = (impl AsyncWriteExt+AsyncReadExt+Unpin+Send+'static);

/// When a connection is established to a peer, this class
/// should take over the socket. It creates its own thread
/// to handle the connection, and you can send it commands
/// Please appreciate the restraint it took to not name it
/// BeerConnection.
pub struct PeerConnection {
    is_alive: bool,
    /// Pushing things into this channel causes them to be
    /// sent over the tcp connection. See get_file for an
    /// example
    tx: Sender<PeerRequest>,
}

#[derive(Clone, PartialEq)]
enum PeerRequestResponseType {
    None,
    WhenSent,
    WhenClosed,
    WhenResponse,
}

/// Encapsulates something that the peer connection should
/// transmit over TCP. The optional `inner` lets you
/// encapsulate a channel, over which you would like to be
/// notified when a response is sent.
struct PeerRequest {
    id: i32,
    response_type: PeerRequestResponseType,
    msg: Vec<u8>,
    inner: Option<oneshot::Sender<items::Response>>,
}

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
async fn handle_request(
    stream: &mut ReadHalf<impl AsyncReadExt>,
    _requests: Arc<Mutex<HashMap<i32, PeerRequest>>>,
    tx: Sender<PeerRequest>,
) -> tokio::io::Result<()> {
    let request = receive_message!(items::Request, stream)?;
    log::info!("Received request {:?}", request);
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
    log::info!("Sending response {:?}", response);
    let header = items::Header {
        r#type: 4,
        compression: 0,
    };
    let mut message_buffer: Vec<u8> = Vec::new();
    message_buffer.extend_from_slice(&u16::to_be_bytes(header.encoded_len() as u16));
    message_buffer.append(&mut header.encode_to_vec());
    message_buffer.extend_from_slice(&u32::to_be_bytes(response.encoded_len() as u32));
    message_buffer.append(&mut response.encode_to_vec());

    // Garbage channel for now
    let pr = PeerRequest {
        id: -1,
        response_type: PeerRequestResponseType::None,
        msg: message_buffer,
        inner: None,
    };
    let r = tx.send(pr);
    assert!(r.is_ok());

    log::info!("Finished handling request");
    Ok(())
}

async fn handle_response(
    stream: &mut ReadHalf<impl AsyncReadExt>,
    requests: Arc<Mutex<HashMap<i32, PeerRequest>>>,
) -> tokio::io::Result<()> {
    let response = receive_message!(items::Response, stream)?;
    log::info!("Received response for request {}", response.id);
    if let Some(peer_request) = requests.lock().unwrap().remove(&response.id) {
        assert!(peer_request.inner.is_some());
        let r = peer_request.inner.unwrap().send(response);
        assert!(r.is_ok());
    }
    Ok(())
}

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_messages(
    stream: &mut ReadHalf<impl AsyncReadExt>,
    requests: Arc<Mutex<HashMap<i32, PeerRequest>>>,
    tx: Sender<PeerRequest>,
) -> tokio::io::Result<()> {
    let r2 = requests.clone();
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
            log::info!("Handling request");
            let r2 = r2.clone();
            let txc = tx.clone();
            handle_request(stream, r2, txc).await?;
        } else if header.r#type == items::MessageType::Response as i32 {
            handle_response(stream, r2.clone()).await?;
        } else if header.r#type == items::MessageType::Close as i32 {
            let close = receive_message!(items::Close, stream)?;
            log::info!("Peer requested close. Reason: {}", close.reason);
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
    requests: Arc<Mutex<HashMap<i32, PeerRequest>>>,
    receiver: Arc<Mutex<Receiver<PeerRequest>>>,
) -> tokio::io::Result<()> {
    loop {
        let mut content: Option<Vec<u8>> = None;
        let mut close = false;
        {
            if let Ok(msg) = receiver.try_lock().map(|l| l.try_recv()) {
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
                        if let Ok(mut l) = requests.lock() {
                            l.insert(msg.id, msg);
                        }
                    }
                }
            }
        }
        if let Some(msg) = content {
            wr.write_all(&*msg).await?;
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
async fn handle_connection(
    mut stream: (impl AsyncWriteExt + AsyncReadExt + Unpin + std::marker::Send + 'static),
    receiver: Arc<Mutex<Receiver<PeerRequest>>>,
    tx: Sender<PeerRequest>,
    name: &'static str,
) -> tokio::io::Result<()> {
    let hello = items::exchange_hellos(&mut stream).await?;
    log::info!("Received hello from {}", hello.client_name);
    let requests: Arc<Mutex<HashMap<i32, PeerRequest>>> = Arc::new(Mutex::new(HashMap::new()));
    let (mut rd, wr) = tokio::io::split(stream);

    // TODO: More graceful shutdown that abort
    let (shutdown_send, mut shutdown_recv) = tokio::sync::mpsc::unbounded_channel::<()>();

    let r = requests.clone();
    let r2 = requests.clone();
    let sendclone = shutdown_send.clone();
    let s1 = tokio::spawn(async move {
        handle_messages(&mut rd, r, tx).await;
        sendclone.send(());
    });
    let sendclone = shutdown_send.clone();
    let s2 = tokio::spawn(async move {
        handle_writing(wr, requests, receiver).await;
        sendclone.send(());
    });
    shutdown_recv.recv().await;
    s1.abort();
    s2.abort();
    log::info!("Shutting down server {}", name);

    let keys: Vec<i32> = r2
        .try_lock()
        .map(|x| x.keys().map(|y| y.clone()).collect())
        .unwrap_or(Vec::new());
    let mut l = r2.lock().unwrap();
    log::info!("locked");
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
    log::info!("returned");
    return Ok(());
}

impl PeerConnection {
    pub fn is_alive(&self) -> bool {
        self.is_alive
    }

    pub fn new(
        socket: (impl AsyncWriteExt + AsyncReadExt + Unpin + Send + 'static),
        name: &'static str,
    ) -> Self {
        let (tx, rx) = channel();
        let txc = tx.clone();
        let me = PeerConnection { is_alive: true, tx };
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, Arc::new(Mutex::new(rx)), txc, name).await {
                log::error!("Error occured in client {}", e);
            }
        });
        me
    }

    /// Requests a file from the peer, writing to the path on the filesystem
    pub async fn get_file(
        &mut self,
        directory: &bep_state::Directory,
        sync_file: &bep_state::File,
    ) -> tokio::io::Result<()> {
        log::info!("Requesting file {}", sync_file.name);

        let header = items::Header {
            r#type: items::MessageType::Request as i32,
            compression: items::MessageCompression::r#None as i32,
        };
        let message_id = StdRng::from_entropy().sample(Standard);

        // TODO: Support bigger files
        let message = items::Request {
            id: message_id,
            folder: directory.id.clone(),
            name: sync_file.name.clone(),
            offset: 0,
            size: 8,
            hash: vec![0],
            from_temporary: false,
        };

        let mut message_buffer: Vec<u8> = Vec::new();
        message_buffer.extend_from_slice(&u16::to_be_bytes(header.encoded_len() as u16));
        message_buffer.append(&mut header.encode_to_vec());
        message_buffer.extend_from_slice(&u32::to_be_bytes(message.encoded_len() as u32));
        message_buffer.append(&mut message.encode_to_vec());
        let (tx, rx) = oneshot::channel();
        let request = PeerRequest {
            id: message_id,
            response_type: PeerRequestResponseType::WhenResponse,
            msg: message_buffer.clone(),
            inner: Some(tx),
        };
        if let Err(e) = self.tx.send(request) {
            log::error!("Received error from buffer: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Error in buffer"));
        }
        let message = rx.await.unwrap();

        if message.id != message_id {
            log::error!("Expected message id {}, got {}", message_id, message.id);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Got out of order Response",
            ));
        }
        if message.code != items::ErrorCode::NoError as i32 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Got error on file request, and I don't know how to handle errors.",
            ));
        }

        let mut file = directory.path.clone();
        file.push(sync_file.name.clone());
        log::info!("Writing to path {:?}", file);
        let mut o = File::create(file)?;
        o.write_all(message.data.as_slice())?;
        Ok(())
    }

    pub async fn close(&mut self) -> tokio::io::Result<()> {
        log::info!("Connection close requested");
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
        let (tx, rx) = oneshot::channel();
        let request = PeerRequest {
            id: -1,
            response_type: PeerRequestResponseType::WhenClosed,
            msg: message_buffer.clone(),
            inner: Some(tx),
        };
        if let Err(e) = self.tx.send(request) {
            log::error!("Received error from buffer: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Error in buffer"));
        }
        let response = rx.await;
        if let Err(e) = response {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Connection was aborted",
            ));
        }
        //assert!(response.is_ok());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_open_close() {
        let _ = env_logger::builder().is_test(true).try_init();
        let (client, server) = tokio::io::duplex(64);
        let mut connection1 = PeerConnection::new(client, "con1");
        let mut connection2 = PeerConnection::new(server, "con2");
        connection1.close().await;
        log::info!("done waiting");
        connection2.close().await;
    }
}
