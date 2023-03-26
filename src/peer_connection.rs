use super::bep_state;
use super::items;
use super::peer_connection_inner::{
    handle_connection, PeerConnectionInner, PeerRequest, PeerRequestResponseType,
};
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
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// TODO: When this rust PR lands
// https://github.com/rust-lang/rust/issues/63063
//type Stream = (impl AsyncWriteExt+AsyncReadExt+Unpin+Send+'static);

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
        socket: (impl AsyncWriteExt + AsyncReadExt + Unpin + Send + 'static),
        name: &'static str,
    ) -> Self {
        let inner = PeerConnectionInner::new(name.to_string());
        let me = PeerConnection {
            inner: inner.clone(),
        };
        tokio::spawn(async move {
            if let Err(e) = handle_connection(socket, inner).await {
                log::error!("Error occured in client {}", e);
            }
        });
        me
    }

    fn submit_request(
        &mut self,
        id: i32,
        response_type: PeerRequestResponseType,
        msg: Vec<u8>,
    ) -> oneshot::Receiver<items::Response> {
        self.inner.submit_request(id, response_type, msg)
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
        let message = self
            .submit_request(
                message_id,
                PeerRequestResponseType::WhenResponse,
                message_buffer,
            )
            .await
            .unwrap();

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
        let _response = self
            .submit_request(-1, PeerRequestResponseType::WhenClosed, message_buffer)
            .await;

        // If it crashes, it's because the server was closed, which we are kind of okay with
        /*
        if let Err(e) = response {
            log::error!("Connection was closed: {}", e);
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "Connection was aborted",
            ));
        }
        */
        Ok(())
    }

    pub fn get_peer_name(&self) -> Option<String> {
        return self.inner.get_peer_name();
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 5)]
    async fn test_open_close() -> io::Result<()> {
        let _ = env_logger::builder().is_test(true).try_init();
        let (client, server) = tokio::io::duplex(64);
        let mut connection1 = PeerConnection::new(client, "con1");
        let mut connection2 = PeerConnection::new(server, "con2");
        assert!(connection1.close().await.is_ok());
        assert!(connection2.close().await.is_ok());
        assert!(connection1.get_peer_name().unwrap() == "con2".to_string());
        assert!(connection2.get_peer_name().unwrap() == "con1".to_string());
        Ok(())
    }
}
