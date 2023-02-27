use std::error::Error;
use std::{thread, time};
use std::str;
use super::bep_state::BepState;
use std::io::{Write, Read};

use mio::net::TcpStream;
use mio::{Events, Interest, Poll, Token};

use prost::Message;

pub struct Daemon {
    state: BepState,
}

use super::items;

const CLIENT: Token = Token(1);

fn connect_to_server(addr: String) -> Result<u8, Box<dyn Error>> {
    let mut stream = TcpStream::connect(addr.parse()?)?;
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    poll.registry().register(&mut stream, CLIENT, Interest::WRITABLE | Interest::READABLE)?;
    poll.poll(&mut events, Some(time::Duration::from_millis(100)))?;
    for event in events.iter() {
        if event.token() == CLIENT {
            let r = items::Request {id: 1, folder: "default".to_string(), name: "file".to_string(), offset: 0, size: 0, hash: vec!(), from_temporary: false};
            send_message!(r, stream);
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
                        println!("Something went wrong: {}", e);
                    }
                }
            }
            thread::sleep(time::Duration::from_millis(1000));
        }
    }
}
