use std::error::Error;
use std::{thread, time};
use std::str;
use super::bep_state::BepState;
use std::io::{Write, Read};

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

pub struct Daemon {
    state: BepState,
}

const CLIENT: Token = Token(1);

fn connect_to_server(addr: String) -> Result<u8, Box<dyn Error>> {
    let mut stream = TcpStream::connect(addr.parse()?)?;
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);

    poll.registry().register(&mut stream, CLIENT, Interest::READABLE)?;
    poll.poll(&mut events, Some(time::Duration::from_millis(100)))?;
    for event in events.iter() {
        if event.token() == CLIENT {
            let mut buf : [u8; 8] = [0;8];
            stream.read(&mut buf)?;
            match str::from_utf8(&buf) {
                Ok(v) => println!("Received data: {}", v),
                Err(e) => panic!("Invalid UTF-8 sequence: {}", e),
            };

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
                    connect_to_server(addr);
                }
            }
            thread::sleep(time::Duration::from_millis(1000));
        }
    }
}
