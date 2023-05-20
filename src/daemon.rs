use super::bep_state::BepState;
use super::models::Peer;
use log;
use std::error::Error;
use std::io;
use std::{thread, time};

use tokio::net::TcpStream;

use super::peer_connection::PeerConnection;
use std::sync::{Arc, Mutex};

pub struct Daemon {
    state: Arc<Mutex<BepState>>,
}

/// Try and connect to the server at addr
async fn connect_to_server(state: Arc<Mutex<BepState>>, addr: String) -> io::Result<()> {
    log::info!(target: "Daemon", "");
    log::info!(target: "Daemon", "Connecting to {addr}");

    let stream = TcpStream::connect(addr).await?;
    let mut connection = PeerConnection::new(stream, state, true);
    connection.wait_for_close().await;
    Ok(())
}

impl Daemon {
    pub fn new(state: BepState) -> Self {
        Daemon {
            state: Arc::new(Mutex::new(state)),
        }
    }

    /// Runs the Daemon
    ///
    /// Currently, the daemon simply tries to connect to every peer,
    /// as defined by the client state, in a loop
    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        loop {
            let mut peers: Option<Vec<Peer>> = None;
            if let Ok(mut l) = self.state.lock() {
                peers = Some(l.get_peers());
            }
            if let Some(list) = peers {
                for peer in list {
                    let addrs = self.state.lock().unwrap().get_addresses(peer);

                    for addr in addrs {
                        let rt = tokio::runtime::Runtime::new().unwrap();
                        let state = self.state.clone();
                        rt.block_on(async { connect_to_server(state, addr).await })?;
                    }
                }
            }
            thread::sleep(time::Duration::from_millis(2000));
        }
    }
}
