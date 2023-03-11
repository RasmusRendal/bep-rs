use std::error::Error;
use super::bep_state::BepState;
use std::io::{self, Read};
use log::{info, warn, error, debug};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use prost::Message;
use super::items;

pub struct Server {
    bind_address: Option<String>,
    state: BepState,
}

/// Receive some messages from a new client, and print them
async fn handle_connection(stream: &mut TcpStream) -> io::Result<()> {
    items::exchange_hellos(stream).await?;
    Ok(())
}

/// Listen for connections at address, printing whatever the client sends us
async fn run_server(address: String) -> io::Result<()> {
    let listener = TcpListener::bind(address).await?;

    loop {
        let (mut socket, _) = listener.accept().await?;

        tokio::spawn(async move {
            if let Err(e) = handle_connection(&mut socket).await {
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
