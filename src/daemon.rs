use std::error::Error;
use std::{thread, time};
use super::bep_state::{self, BepState};
use std::io::{self, Write, Read};
use log::{info, warn, error};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::path::PathBuf;
use std::fs::File;
use super::commands;

use tokio::net::TcpStream;

use prost::Message;

pub struct Daemon {
    state: BepState,
}

use super::items;

/// Try and connect to the server at addr
async fn connect_to_server(state: &mut BepState, addr: String) -> io::Result<()> {
    info!(target: "Daemon", "");
    info!(target: "Daemon", "Connecting to {addr}");

    let mut stream = TcpStream::connect(addr).await?;
    items::exchange_hellos(&mut stream).await?;

    for folder in state.get_sync_directories() {
        info!("Syncing directory {}!", folder.label);
        let file = bep_state::File { name: "testfile".to_string(), hash: vec![], blocks: vec![]};
        commands::get_file(&mut stream, &folder, &file).await?;
        info!("Synced file {}!", file.name);
    }

    Ok(())
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
                    rt.block_on(async { connect_to_server(&mut self.state, addr).await })?;
                }
            }
            thread::sleep(time::Duration::from_millis(2000));
        }
    }
}
