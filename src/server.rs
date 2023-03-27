use super::bep_state::BepState;
use log;
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

use super::peer_connection::PeerConnection;

pub struct Server {
    bind_address: Option<String>,
    state: Arc<Mutex<BepState>>,
}

/// Listen for connections at address, printing whatever the client sends us
async fn run_server(address: String, state: Arc<Mutex<BepState>>) -> io::Result<()> {
    let listener = TcpListener::bind(address).await?;

    loop {
        let (socket, _) = listener.accept().await?;

        PeerConnection::new(socket, state.clone());
    }
}

impl Server {
    pub fn new(state: Arc<Mutex<BepState>>) -> Self {
        Server {
            bind_address: Some("0.0.0.0:21027".to_string()),
            state,
        }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        let address = self.bind_address.clone().unwrap();
        log::info!("Starting server, listening on {} ...", address);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let state = self.state.clone();
        rt.block_on(async { run_server(address, state).await })?;
        Ok(0)
    }
}
