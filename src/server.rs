use crate::bep_state_reference::BepStateRef;

use log;
use std::error::Error;
use std::io;
use tokio::net::TcpListener;

use super::peer_connection::PeerConnection;

pub struct Server {
    bind_address: Option<String>,
    state: BepStateRef,
}

/// Listen for connections at address, printing whatever the client sends us
async fn run_server(address: String, state: BepStateRef) -> io::Result<()> {
    let listener = TcpListener::bind(address).await?;

    loop {
        let (socket, _) = listener.accept().await?;

        PeerConnection::new(socket, state.clone(), true);
    }
}

impl Server {
    pub fn new(state: BepStateRef) -> Self {
        Server {
            bind_address: Some("0.0.0.0:21027".to_string()),
            state,
        }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub async fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        let address = self.bind_address.clone().unwrap();
        log::info!("Starting server, listening on {} ...", address);
        run_server(address, self.state.clone()).await?;
        Ok(0)
    }
}
