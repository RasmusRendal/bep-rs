use crate::bep_state_reference::BepStateRef;
use crate::peer_connection::error::PeerConnectionError;

use log;
use std::error::Error;
use std::{thread, time};

use tokio::net::TcpStream;

use super::peer_connection::PeerConnection;

pub struct Daemon {
    state: BepStateRef,
}

/// Try and connect to the server at addr
async fn connect_to_server(state: BepStateRef, addr: String) -> Result<(), PeerConnectionError> {
    log::info!(target: "Daemon", "");
    log::info!(target: "Daemon", "Connecting to {addr}");

    let stream = TcpStream::connect(addr).await?;
    let mut connection = PeerConnection::new(stream, state, false);
    connection.watch();
    connection.wait_for_close().await?;
    Ok(())
}

impl Daemon {
    pub fn new(state: BepStateRef) -> Self {
        Daemon { state }
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
                    connect_to_server(self.state.clone(), addr).await?;
                }
            }
            thread::sleep(time::Duration::from_millis(2000));
        }
    }
}
