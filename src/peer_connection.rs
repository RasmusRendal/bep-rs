use super::bep_state;
use super::items;
use log;
use prost::Message;
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::fs::File;
use std::io::{self, Write};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::net::TcpStream;

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

/// Encapsulates something that the peer connection should
/// transmit over TCP. The optional `inner` lets you
/// encapsulate a channel, over which you would like to be
/// notified when a response is sent.
#[derive(Clone)]
struct PeerRequest {
    id: i32,
    msg: Vec<u8>,
    inner: Option<Arc<Mutex<Sender<items::Response>>>>,
}

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
async fn handle_request(
    stream: &mut ReadHalf<TcpStream>,
    _requests: Arc<Mutex<Vec<PeerRequest>>>,
    tx: Sender<PeerRequest>,
) -> io::Result<()> {
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
        msg: message_buffer,
        inner: None,
    };
    tx.send(pr);

    log::info!("Finished handling request");
    Ok(())
}

async fn handle_response(
    stream: &mut ReadHalf<TcpStream>,
    requests: Arc<Mutex<Vec<PeerRequest>>>,
) -> io::Result<()> {
    let response = receive_message!(items::Response, stream)?;
    log::info!("Received response for request {}", response.id);
    if let Ok(reqlist) = requests.lock() {
        for req in reqlist.iter() {
            if req.id == response.id {
                // This unwrap should be safe
                // If inner is None, it should not be added to the vector
                if let Ok(tx) = req.inner.as_ref().unwrap().try_lock() {
                    if let Err(e) = tx.send(response) {
                        return Err(io::Error::new(io::ErrorKind::Other, "Got a sending error"));
                    }
                    break;
                } else {
                    return Err(io::Error::new(io::ErrorKind::Other, "Could not lock"));
                }
            }
        }
    }
    Ok(())
}

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_messages(
    stream: &mut ReadHalf<TcpStream>,
    requests: Arc<Mutex<Vec<PeerRequest>>>,
    tx: Sender<PeerRequest>,
) -> io::Result<()> {
    log::info!("Handle messages function called");
    let r2 = requests.clone();
    loop {
        let header_len = stream.read_u16().await? as usize;
        let mut header = vec![0u8; header_len];
        stream.read_exact(&mut header).await?;
        let header = items::Header::decode(&*header)?;
        if header.compression != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Peer is trying to use compression, but we do not support it",
            ));
        }
        if header.r#type == 3 {
            log::info!("Handling request");
            let r2 = r2.clone();
            let txc = tx.clone();
            handle_request(stream, r2, txc).await?;
        } else if header.r#type == items::MessageType::Response as i32 {
            handle_response(stream, r2.clone()).await?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown message type",
            ));
        }
    }
}

async fn handle_writing(
    mut wr: WriteHalf<TcpStream>,
    requests: Arc<Mutex<Vec<PeerRequest>>>,
    receiver: Arc<Mutex<Receiver<PeerRequest>>>,
) -> io::Result<()> {
    log::info!("Spawn 2 success");
    loop {
        let mut msg: Option<PeerRequest> = None;
        if let Ok(Ok(message)) = receiver.try_lock().map(|l| l.try_recv()) {
            msg = Some(message.clone());
        }
        if let Some(m) = msg {
            log::info!("Sending message with id {}", m.id);
            wr.write_all(&*m.msg).await?;
            if m.inner.is_some() {
                if let Ok(mut l) = requests.lock() {
                    l.push(m);
                }
            }
        }
    }
}

/// Starts two tasks, one that sends data over TCP, and one that receives
/// If as a result of receiving a message, the server wants to transmit something,
/// it simply adds is to the tx queue
async fn handle_connection(
    mut stream: TcpStream,
    receiver: Arc<Mutex<Receiver<PeerRequest>>>,
    tx: Sender<PeerRequest>,
) -> io::Result<()> {
    items::exchange_hellos(&mut stream).await?;
    let requests: Arc<Mutex<Vec<PeerRequest>>> = Arc::new(Mutex::new(vec![]));
    let (mut rd, wr) = tokio::io::split(stream);

    let r = requests.clone();
    let s1 = tokio::spawn(async move { handle_messages(&mut rd, r, tx).await });

    let s2 = tokio::spawn(async move { handle_writing(wr, requests, receiver).await });

    let result = tokio::try_join!(s1, s2)?;
    result.0?;
    result.1?;

    return Ok(());
}

impl PeerConnection {
    pub fn is_alive(&self) -> bool {
        self.is_alive
    }

    pub fn new(socket: TcpStream) -> Self {
        log::info!("New connection");
        let (tx, rx) = channel();
        let txc = tx.clone();
        let me = PeerConnection { is_alive: true, tx };
        log::info!("The connection handler thread is a-runnin");
        tokio::spawn(async move {
            log::info!("The connection handler thread is a-runnin");
            if let Err(e) = handle_connection(socket, Arc::new(Mutex::new(rx)), txc).await {
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
    ) -> io::Result<()> {
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
        let (tx, rx) = channel();
        let request = PeerRequest {
            id: message_id,
            msg: message_buffer.clone(),
            inner: Some(Arc::new(Mutex::new(tx))),
        };
        if let Err(e) = self.tx.send(request) {
            log::error!("Received error from buffer: {}", e);
            return Err(io::Error::new(io::ErrorKind::Other, "Error in buffer"));
        }
        // TODO: Do something a bit more clever, maybe using a future
        let message = rx.recv().unwrap();

        if message.id != message_id {
            log::error!("Expected message id {}, got {}", message_id, message.id);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Got out of order Response",
            ));
        }
        if message.code != 0 {
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
}
