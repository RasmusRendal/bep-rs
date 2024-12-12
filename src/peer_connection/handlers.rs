use super::items::{self, EncodableItem};
use super::verifier::verify_connection;
use super::{PeerConnection, PeerRequestResponse, PeerRequestResponseType};
use super::{PeerConnectionError, SyncFile};
use futures::channel::oneshot;
use log;
use prost::Message;
use ring::digest;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf};
use tokio::sync::mpsc::{Receiver, UnboundedReceiver};

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
async fn handle_request(
    request: items::Request,
    peer_connection: PeerConnection,
) -> tokio::io::Result<()> {
    log::info!(
        "{}: Received request {:?}",
        peer_connection.get_name(),
        request
    );

    let dir = peer_connection
        .state
        .get_sync_directory(&request.folder)
        .await;

    let peer = peer_connection.get_peer().unwrap();

    if dir.is_none()
        || !peer_connection
            .state
            .is_directory_synced(dir.as_ref().unwrap(), &peer)
            .await
    {
        log::error!("Peer requested file, but it does not exist");
        let response = items::Response {
            id: request.id,
            data: vec![],
            code: items::ErrorCode::InvalidFile as i32,
        };
        peer_connection
            .submit_message(response.encode_for_bep())
            .await;
        return Ok(());
    }

    let dir = dir.unwrap();
    let mut path = dir.path.clone();
    path.push(request.name.clone());
    let fh = File::open(path);
    if fh.is_err() {
        let response = items::Response {
            id: request.id,
            data: [].to_vec(),
            code: items::ErrorCode::NoSuchFile as i32,
        };
        peer_connection
            .submit_message(response.encode_for_bep())
            .await;
        return Ok(());
    }

    let mut buf_reader = BufReader::new(fh.unwrap());
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
    log::info!(
        "{}: Sending response {:?}",
        peer_connection.get_name(),
        response
    );
    peer_connection
        .submit_message(response.encode_for_bep())
        .await;

    log::info!("{}: Finished handling request", peer_connection.get_name());
    Ok(())
}

async fn handle_response(
    response: items::Response,
    peer_connection: PeerConnection,
) -> tokio::io::Result<()> {
    log::info!(
        "{}: Received response for request {}",
        peer_connection.get_name(),
        response.id
    );
    if let Some(peer_request) = peer_connection
        .requests
        .write()
        .unwrap()
        .remove(&response.id)
    {
        assert!(peer_request.response_type == PeerRequestResponseType::WhenResponse);
        let r = peer_request
            .peer_connection
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

async fn handle_index(
    index: items::Index,
    peer_connection: PeerConnection,
) -> tokio::io::Result<()> {
    log::info!("{}: Handling index", peer_connection.get_name());
    let syncdir = peer_connection
        .state
        .get_sync_directory(&index.folder)
        .await
        .unwrap();
    let mut localindex = syncdir.get_index(peer_connection.state.clone()).await;
    for file in index.files {
        log::info!(
            "{}: Handling index file {}",
            peer_connection.get_name(),
            file.name
        );
        let localfile = localindex
            .iter_mut()
            .find(|x| x.get_name(&syncdir) == file.name);
        if localfile.is_some() {
            let localfile: &mut SyncFile = localfile.unwrap();
            if localfile.versions.len() >= file.version.as_ref().unwrap().counters.len() {
                continue;
            }
            log::info!(
                "{}: Updating file {}",
                peer_connection.get_name(),
                file.name
            );
            localfile.versions = file
                .version
                .unwrap()
                .counters
                .into_iter()
                .map(|x| (x.id, x.value))
                .collect();
            localfile.hash = file.blocks[0].hash.clone();
            localfile.modified_by = file.modified_by;
            peer_connection
                .state
                .update_sync_file(syncdir.clone(), localfile.clone())
                .await;
        } else {
            log::info!("{}: New file {}", peer_connection.get_name(), file.name);
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
            peer_connection
                .state
                .update_sync_file(syncdir.clone(), syncfile)
                .await;
        }
    }
    log::info!("{}: Index merged", peer_connection.get_name());
    tokio::spawn(async move {
        if let Err(e) = peer_connection.get_directory(&syncdir).await {
            log::error!(
                "{}: Got error syncing directory {}",
                peer_connection.get_name(),
                e
            );
        }
    });

    Ok(())
}

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_reading(
    stream: &mut ReadHalf<impl AsyncRead>,
    peer_connection: PeerConnection,
) -> tokio::io::Result<()> {
    loop {
        log::info!("{}: Waiting for a new message", peer_connection.get_name());
        let header_len = tokio::time::timeout(Duration::from_millis(1000 * 30), stream.read_u16())
            .await?? as usize;
        log::info!(
            "{}: Got a header length {}",
            peer_connection.get_name(),
            header_len
        );
        let mut header = vec![0u8; header_len];
        stream.read_exact(&mut header).await?;
        let header = items::Header::decode(&*header)?;
        log::info!(
            "{}: Got a header of type {}",
            peer_connection.get_name(),
            header.r#type
        );
        if header.compression != 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Peer is trying to use compression, but we do not support it",
            ));
        }
        if header.r#type == items::MessageType::Request as i32 {
            log::info!("{}: Handling request", peer_connection.get_name());
            let peer_connectionclone = peer_connection.clone();
            let request = receive_message!(items::Request, stream)?;
            tokio::spawn(async move {
                if let Err(e) = handle_request(request, peer_connectionclone.clone()).await {
                    log::error!("Received an error when handling request: {}", e);
                    let _ = peer_connectionclone.close_err(&e).await;
                }
            });
        } else if header.r#type == items::MessageType::Index as i32 {
            let index = receive_message!(items::Index, stream)?;
            let peer_connectionc = peer_connection.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_index(index, peer_connectionc.clone()).await {
                    log::error!("Received an error when handling index: {}", e);
                    let _ = peer_connectionc.close_err(&e).await;
                }
            });
        } else if header.r#type == items::MessageType::Response as i32 {
            log::info!("{}: Got a response", peer_connection.get_name());
            let peer_connectionclone = peer_connection.clone();
            let response = receive_message!(items::Response, stream)?;
            tokio::spawn(async move {
                if let Err(e) = handle_response(response, peer_connectionclone.clone()).await {
                    log::error!("Received an error when handling response: {}", e);
                    let _ = peer_connectionclone.close_err(&e).await;
                }
            });
        } else if header.r#type == items::MessageType::Close as i32 {
            let close = receive_message!(items::Close, stream)?;
            log::info!(
                "{}: Peer requested close. Reason: {}",
                peer_connection.get_name(),
                close.reason
            );
            return Ok(());
        } else {
            log::error!(
                "{}: Got unknown message type {}",
                peer_connection.get_name(),
                header.r#type
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Unknown message type",
            ));
        }
    }
}

/// Closes the channel used for receiving messages for our peer
///
/// # Arguments
///
/// * `rx` - The receiver for the message queue
async fn close_receiver(mut rx: Receiver<(Vec<u8>, Option<oneshot::Sender<PeerRequestResponse>>)>) {
    // Empty the message queue
    while let Some((_msg, txo)) = rx.recv().await {
        if let Some(tx) = txo {
            // If we get an error, it's because the place we're reporting errors to has disappeared
            let _ = tx.send(PeerRequestResponse::Closed);
        }
    }
    rx.close();
}

async fn handle_writing(
    mut wr: WriteHalf<impl AsyncWriteExt>,
    peer_connection: PeerConnection,
    mut rx: Receiver<(Vec<u8>, Option<oneshot::Sender<PeerRequestResponse>>)>,
) -> tokio::io::Result<()> {
    while let Some((msg, tx)) = rx.recv().await {
        if !msg.is_empty() {
            if let Err(e) = wr.write_all(&msg).await {
                log::error!(
                    "{}: Error writing message, closing",
                    peer_connection.get_name()
                );
                close_receiver(rx).await;
                return Err(e);
            }
            wr.flush().await?;
            log::info!("{}: Wrote message", peer_connection.get_name());
            //log::info!("{}: Wrote message", peer_connection.get_name());
        }
        if let Some(tx) = tx {
            let r = tx.send(PeerRequestResponse::Sent);
            if let Err(_e) = r {
                log::error!(
                    "{}: Got error when sending response:",
                    peer_connection.get_name()
                );
            }
        }
    }
    close_receiver(rx).await;
    Ok(())
}

async fn handle_hello(
    stream: &mut (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    peer_connection: &PeerConnection,
) -> Result<(), PeerConnectionError> {
    let hello = items::exchange_hellos(stream, peer_connection.get_name().to_string()).await?;
    log::info!(
        "{}: Received hello from {}",
        peer_connection.get_name(),
        hello.client_name
    );
    Ok(())
}

pub fn drain_requests(peer_connection: &PeerConnection) {
    let mut requests = peer_connection.requests.write().unwrap();
    for (_key, channel) in requests.drain() {
        let s = channel.peer_connection.send(PeerRequestResponse::Closed);
        assert!(s.is_ok());
    }
}

/// Starts two tasks, one that sends data over TCP, and one that receives
/// If as a result of receiving a message, the server wants to transmit something,
/// it simply adds is to the tx queue
pub async fn handle_connection(
    mut stream: (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    mut peer_connection: PeerConnection,
    rx: Receiver<(Vec<u8>, Option<oneshot::Sender<PeerRequestResponse>>)>,
    mut shutdown_recv: UnboundedReceiver<()>,
    connector: bool,
) -> Result<(), PeerConnectionError> {
    handle_hello(&mut stream, &peer_connection).await?;

    let peerids = peer_connection
        .state
        .state
        .lock()
        .unwrap()
        .get_peers()
        .into_iter()
        .map(|x| x.device_id.unwrap_or_default().clone())
        .collect();
    let certificate = tokio_rustls::rustls::Certificate(
        peer_connection
            .state
            .state
            .lock()
            .unwrap()
            .get_certificate(),
    );
    let key =
        tokio_rustls::rustls::PrivateKey(peer_connection.state.state.lock().unwrap().get_key());

    let conn_result = verify_connection(stream, certificate, key, peerids, connector).await;
    if conn_result.is_err() {
        return Err(PeerConnectionError::UnknownPeer);
    }
    let (tcpstream, peer_id) = conn_result.unwrap();
    log::info!("Peer id: {:?}", peer_id);
    peer_connection.set_peer(peer_id);
    let (mut rd, wr) = tokio::io::split(tcpstream);
    peer_connection.send_index().await?;

    let name = peer_connection.get_name();
    let sendclone = peer_connection.shutdown_send.clone();
    let peer_connectionclone = peer_connection.clone();
    tokio::spawn(async move {
        let r = handle_reading(&mut rd, peer_connectionclone.clone()).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from reader: {}", name, e);
            peer_connectionclone.close_err(e).await?;
        }
        let _r = sendclone.send(());
        r
    });

    let sendclone = peer_connection.shutdown_send.clone();
    let peer_connectionclone = peer_connection.clone();
    let name = peer_connection.get_name();
    tokio::spawn(async move {
        let r = handle_writing(wr, peer_connectionclone, rx).await;
        if let Err(e) = &r {
            log::error!("{}: Got error from writer: {}", name, e);
        }
        let _r = sendclone.send(());
        r
    });

    shutdown_recv.recv().await;
    shutdown_recv.close();

    // Remove from list of listeners
    peer_connection
        .state
        .state
        .lock()
        .unwrap()
        .listeners
        .retain(|x| !x.tx.same_channel(&peer_connection.tx));

    log::info!("{}: Shutting down server", peer_connection.get_name());
    Ok(())
}
