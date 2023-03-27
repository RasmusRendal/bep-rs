use super::bep_state;
#[macro_use]
mod items;
mod peer_connection_inner;
use super::bep_state::BepState;
use log;
use peer_connection_inner::{
    handle_connection, PeerConnectionInner, PeerRequestResponse, PeerRequestResponseType,
};
use prost::Message;
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ring::digest;
use std::fs::File;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncRead, AsyncWrite};

// TODO: When this rust PR lands
// https://github.com/rust-lang/rust/issues/63063
//type Stream = (impl AsyncWrite+AsyncRead+Unpin+Send+'static);

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
        socket: (impl AsyncWrite + AsyncRead + Unpin + Send + 'static),
        state: Arc<Mutex<BepState>>,
    ) -> Self {
        let inner = PeerConnectionInner::new(state);
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

    async fn submit_request(
        &mut self,
        id: i32,
        response_type: PeerRequestResponseType,
        msg: Vec<u8>,
    ) -> io::Result<PeerRequestResponse> {
        self.inner.submit_request(id, response_type, msg).await
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
            hash: sync_file.blocks[0].hash.clone(),
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
            .await?;

        match message {
            PeerRequestResponse::Response(response) => {
                let hash = digest::digest(&digest::SHA256, &response.data);
                if hash.as_ref() != sync_file.blocks[0].hash.clone() {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Received file does not correspond to requested hash",
                    ));
                }

                let mut file = directory.path.clone();
                file.push(sync_file.name.clone());
                log::info!("Writing to path {:?}", file);
                let mut o = File::create(file)?;
                o.write_all(response.data.as_slice())?;
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

    pub async fn close(&mut self) -> tokio::io::Result<()> {
        log::info!("Connection close requested");
        let response = self.inner.close().await?;

        match response {
            PeerRequestResponse::Closed => Ok(()),
            PeerRequestResponse::Error(e) => {
                log::error!(
                    "{}: Got error while trying to close request: {}",
                    self.inner.get_name(),
                    e
                );
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "Got an error while trying to close connection",
                ));
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Invalid response to close request. This should not happen.",
                ));
            }
        }
    }

    pub fn get_peer_name(&self) -> Option<String> {
        return self.inner.get_peer_name();
    }
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;
    use std::io::prelude::*;
    use std::io::BufReader;

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_open_close() -> io::Result<()> {
        console_subscriber::init();
        let _ = env_logger::builder().is_test(true).try_init();
        let tempdir1 = tempfile::tempdir().unwrap().into_path();
        let state1 = Arc::new(Mutex::new(BepState::new(tempdir1)));
        state1.lock().unwrap().set_name("con1".to_string());
        let tempdir2 = tempfile::tempdir().unwrap().into_path();
        let state2 = Arc::new(Mutex::new(BepState::new(tempdir2)));
        state2.lock().unwrap().set_name("con2".to_string());
        let (client, server) = tokio::io::duplex(64);
        let mut connection1 = PeerConnection::new(client, state1);
        let mut connection2 = PeerConnection::new(server, state2);
        connection1.close().await.unwrap();
        connection2.close().await.unwrap();
        assert!(connection1.get_peer_name().unwrap() == "con2".to_string());
        assert!(connection2.get_peer_name().unwrap() == "con1".to_string());
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_get_file() -> io::Result<()> {
        let _ = env_logger::builder().is_test(true).try_init();
        let (client, server) = tokio::io::duplex(1024);
        let tempdir1 = tempfile::tempdir().unwrap().into_path();
        let state1 = Arc::new(Mutex::new(BepState::new(tempdir1)));
        let tempdir2 = tempfile::tempdir().unwrap().into_path();
        let state2 = Arc::new(Mutex::new(BepState::new(tempdir2)));

        let mut connection1 = PeerConnection::new(client, state1);
        let mut connection2 = PeerConnection::new(server, state2);
        let hash: Vec<u8> = b"\xb9\x4d\x27\xb9\x93\x4d\x3e\x08\xa5\x2e\x52\xd7\xda\x7d\xab\xfa\xc4\x84\xef\xe3\x7a\x53\x80\xee\x90\x88\xf7\xac\xe2\xef\xcd\xe9".to_vec();
        let block = bep_state::Block {
            offset: 0,
            size: 8,
            hash,
        };
        let file = bep_state::File {
            name: "testfile".to_string(),
            hash: vec![],
            blocks: vec![block],
        };
        let mut tempdir = tempfile::tempdir().unwrap().into_path();
        let dir = bep_state::Directory {
            id: "garbage".to_string(),
            label: "directory".to_string(),
            path: tempdir.clone(),
            files: vec![],
        };
        connection1.get_file(&dir, &file).await?;
        tempdir.push("testfile");
        let file = File::open(tempdir).unwrap();
        let mut buf_reader = BufReader::new(file);
        let mut contents = String::new();
        buf_reader.read_to_string(&mut contents)?;
        assert_eq!(contents, "hello world");
        connection1.close().await?;
        connection2.close().await?;
        Ok(())
    }
}
