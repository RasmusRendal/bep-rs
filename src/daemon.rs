use crate::bep_state_reference::BepStateRef;
use crate::peer_connection::error::PeerConnectionError;
use crate::storage_backend::StorageBackend;

use log;
use std::error::Error;
use std::sync::{Arc, Mutex};
use std::{thread, time};

use tokio::net::TcpStream;

use super::peer_connection::PeerConnection;

pub struct Daemon {
    state: BepStateRef,
    storage_backend: Arc<Mutex<dyn StorageBackend + Send>>,
}

/// Try and connect to the server at addr
async fn connect_to_server(
    state: BepStateRef,
    addr: String,
    storage_backend: Arc<Mutex<dyn StorageBackend + Send>>,
) -> Result<(), PeerConnectionError> {
    log::info!(target: "Daemon", "");
    log::info!(target: "Daemon", "Connecting to {addr}");

    let stream = TcpStream::connect(addr).await?;
    let mut connection = PeerConnection::new(stream, state, storage_backend, false);
    connection.watch();
    connection.wait_for_close().await?;
    Ok(())
}

impl Daemon {
    pub fn new(state: BepStateRef, storage_backend: Arc<Mutex<dyn StorageBackend + Send>>) -> Self {
        Daemon {
            state,
            storage_backend,
        }
    }

    /// Runs the Daemon
    ///
    /// Currently, the daemon simply tries to connect to every peer,
    /// as defined by the client state, in a loop
    pub async fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        loop {
            for peer in self.state.get_peers().await {
                let addrs = self.state.get_addresses(peer).await;

                for addr in addrs {
                    connect_to_server(self.state.clone(), addr, self.storage_backend.clone())
                        .await?;
                }
            }
            thread::sleep(time::Duration::from_millis(2000));
        }
    }
}
