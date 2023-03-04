use std::error::Error;
use std::{thread, time};
use std::str;
use super::bep_state::BepState;
use std::io::{Write, Read};
use log::{info, warn, error};

use mio::net::TcpStream;
use mio::{Events, Interest, Poll, Token};

use prost::Message;

pub struct Daemon {
    state: BepState,
}

use super::items;

const CLIENT: Token = Token(1);

// TODO: Stop returning integers constantly, figure out how to have a result return type with
// void/error
/// Given a socket, send a BEP hello message
pub fn send_hello(socket: &mut TcpStream) -> Result<u8, Box<dyn Error>> {
    let hello = items::Hello {device_name: "device".to_string(), client_name: "beercan".to_string(), client_version: "0.1".to_string()};

    let magic = (0x2EA7D90B as u32).to_be_bytes().to_vec();
    socket.write_all(&magic)?;
    socket.write_all(&u32::to_be_bytes(hello.encoded_len() as u32))?;
    socket.write_all(&hello.encode_to_vec())?;

    Ok(1)
}

/// Try and connect to the server at addr
fn connect_to_server(addr: String) -> Result<u8, Box<dyn Error>> {
    info!(target: "Daemon", "");
    info!(target: "Daemon", "Connecting to {addr}");
    let mut stream = TcpStream::connect(addr.parse()?)?;
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    poll.registry().register(&mut stream, CLIENT, Interest::WRITABLE)?;
    let mut closed = false;
    while !closed {
        poll.poll(&mut events, Some(time::Duration::from_millis(100)))?;
        for event in events.iter() {
            if event.token() == CLIENT {
                info!(target: "Daemon", "{:?}", event);
                if event.is_read_closed() || event.is_write_closed() {
                    closed = true;
                } else {
                    send_hello(&mut stream)?;
                    stream.flush()?;
                    drop(&mut stream);
                    closed = true;
                }
            }
        }
    }
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
                    if let Err(e) = connect_to_server(addr) {
                        error!("Something went wrong: {}", e);
                    }
                }
            }
            thread::sleep(time::Duration::from_millis(2000));
        }
    }
}
