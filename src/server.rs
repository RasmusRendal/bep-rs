use std::error::Error;
use super::bep_state::BepState;
use std::io::{self, Read, Write};
use log::{info, warn, error, debug};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use prost::Message;
use super::items;

pub struct Server {
    bind_address: Option<String>,
    state: BepState,
}

/// Call this function when the client has indicated it want to send a Request
/// At the moment, it always responds with a hardcoded file
async fn handle_request(stream: &mut TcpStream) -> io::Result<()> {
    let request = receive_message!(items::Request, stream)?;
    info!("Received request {:?}", request);
    let response = if request.name == "testfile" {
         items::Response {id: request.id, data: "hello world".to_string().into_bytes(), code: 0 }
    } else {
         items::Response {id: request.id, data: vec![], code: 2 }
    };
    info!("Sending response {:?}", response);
    let header = items::Header { r#type: 4, compression: 0 };
    stream.write_all(&u16::to_be_bytes(header.encoded_len() as u16)).await?;
    stream.write_all(&header.encode_to_vec()).await?;
    stream.write_all(&u32::to_be_bytes(response.encoded_len() as u32)).await?;
    stream.write_all(&response.encode_to_vec()).await?;
    Ok(())
}

/// The main function of the server. Decode a message, and handle it accordingly
async fn handle_message(stream: &mut TcpStream) -> io::Result<()> {
    let header_len = stream.read_u16().await? as usize;
    info!("Receiveng len {} header", header_len);
    let mut header = vec![0u8; header_len];
    stream.read_exact(&mut header).await?;
    info!("Received header {:?} ", header);
    let header = items::Header::decode(&*header)?;
    info!("decoded the header");
    if header.compression != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Peer is trying to use compression, but we do not support it"));
    }
    if header.r#type == 3 {
        info!("Handling request");
        handle_request(stream).await?;
    } else {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Unknown message type"));

    }

    Ok(())
}

/// Receive some messages from a new client, and print them
async fn handle_connection(stream: &mut TcpStream) -> io::Result<()> {
    items::exchange_hellos(stream).await?;

    info!("Ready to handle messages");
    loop {
        handle_message(stream).await?;
    }
}

/// Listen for connections at address, printing whatever the client sends us
async fn run_server(address: String) -> io::Result<()> {
    let listener = TcpListener::bind(address).await?;

    loop {
        let (mut socket, _) = listener.accept().await?;

        tokio::spawn(async move {
            if let Err(e) = handle_connection(&mut socket).await {
                error!("Error occured in client {}", e);
                error!("Error occured in client {}", e);
            }
        });
    }
}


impl Server {
    pub fn new(state: BepState) -> Self {
        Server { bind_address: Some("0.0.0.0:21027".to_string()), state: state }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        let address = self.bind_address.clone().unwrap();
        info!("Starting server, listening on {} ...",address);
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async { run_server(address).await })?;
        Ok(0)
    }


}
