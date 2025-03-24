use crate::bep_state_reference::BepStateRef;
use crate::storage_backend::StorageBackend;

use log;
use std::error::Error;
use std::io;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;

use super::peer_connection::PeerConnection;

pub struct Server {
    bind_address: Option<String>,
    state: BepStateRef,
    storage_backend: Arc<Mutex<dyn StorageBackend + Send>>,
}

/// Listen for connections at address, printing whatever the client sends us
async fn run_server(
    address: String,
    state: BepStateRef,
    storage_backend: Arc<Mutex<dyn StorageBackend + Send>>,
) -> io::Result<()> {
    let listener = TcpListener::bind(address).await?;

    loop {
        let (socket, _) = listener.accept().await?;

        let mut pc = PeerConnection::new(socket, state.clone(), storage_backend.clone(), true);
        let _ = pc.wait_for_ready().await;
        pc.watch();
    }
}

impl Server {
    pub fn new(state: BepStateRef, storage_backend: Arc<Mutex<dyn StorageBackend + Send>>) -> Self {
        Server {
            bind_address: Some("0.0.0.0:21027".to_string()),
            state,
            storage_backend,
        }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub async fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        let address = self.bind_address.clone().unwrap();
        log::info!("Starting server, listening on {} ...", address);
        run_server(address, self.state.clone(), self.storage_backend.clone()).await?;
        Ok(0)
    }
}
