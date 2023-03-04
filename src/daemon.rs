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
fn send_hello(socket: &mut TcpStream) -> Result<u8, Box<dyn Error>> {
    let mut magic = (0x2EA7D90B as u32).to_be_bytes().to_vec();
    let hello = items::Hello {device_name: "device".to_string(), client_name: "beercan".to_string(), client_version: "0.1".to_string()};
    let hello_len = u32::to_be_bytes(hello.encoded_len() as u32);
    let mut hello = hello.encode_to_vec();
    let mut msg = Vec::new();
    msg.append(&mut magic);
    msg.extend_from_slice(&hello_len);
    msg.append(&mut hello);
    socket.write(&mut msg)?;

    let r = items::Request {id: 1, folder: "default".to_string(), name: "file".to_string(), offset: 0, size: 0, hash: vec!(), from_temporary: false};
    send_message!(r, socket);
    Ok(1)
}

fn connect_to_server(addr: String) -> Result<u8, Box<dyn Error>> {
    let mut stream = TcpStream::connect(addr.parse()?)?;
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    info!(target: "Daemon", "Connecting to server at {} ...", addr);
    poll.registry().register(&mut stream, CLIENT, Interest::WRITABLE | Interest::READABLE)?;
    poll.poll(&mut events, Some(time::Duration::from_millis(100)))?;
    for event in events.iter() {
        if event.token() == CLIENT {
            send_hello(&mut stream)?;
        }
    }
    Ok(1)
}

impl Daemon {
    pub fn new(state: BepState) -> Self {
        Daemon { state }
    }

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
