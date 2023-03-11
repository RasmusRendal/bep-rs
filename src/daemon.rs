use std::error::Error;
use std::{thread, time};
use super::bep_state::BepState;
use std::io::{Write, Read};
use log::{info, warn, error};

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

use prost::Message;

pub struct Daemon {
    state: BepState,
}

use super::items;

/// Try and connect to the server at addr
async fn connect_to_server(addr: String) -> Result<u8, Box<dyn Error>> {
    info!(target: "Daemon", "");
    info!(target: "Daemon", "Connecting to {addr}");

    let mut stream = TcpStream::connect(addr).await?;
    items::exchange_hellos(&mut stream).await?;

    Ok(1)
}

impl Daemon {
    pub fn new(state: BepState) -> Self {
        Daemon { state }
    }

    /// Runs the Daemon
    ///
    /// Currently, the daemon simply tries to connect to every peer,
    /// as defined by the client state, in a loop
    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        loop {
            for peer in self.state.get_peers() {
                for addr in self.state.get_addresses(peer) {
                    let rt = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .unwrap();
                    rt.block_on(async { connect_to_server(addr).await })?;
                }
            }
            thread::sleep(time::Duration::from_millis(2000));
        }
    }
}
