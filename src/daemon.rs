use std::error::Error;
use std::{thread, time};
use std::str;
use super::bep_state::BepState;
use std::io::{Write, Read};

use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

pub struct Daemon {
    bind_address: Option<String>,
    state: BepState,
}

const SERVER: Token = Token(0);
const CLIENT: Token = Token(1);

fn run_server(address: String) -> Result<i32, Box<dyn Error>> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let mut listener = TcpListener::bind(address.parse().unwrap()).unwrap();

    poll.registry()
        .register(&mut listener, SERVER, Interest::READABLE)?;

    loop {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            match event.token() {
                SERVER => {
                    let connection = listener.accept();
                    if let Ok((mut stream, _addr)) = connection {
                        stream.write(b"hello")?;
                    } else {
                        drop(connection);
                    }
                }
                _ => {

                }
            }
        }
    }

}

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
        Daemon { bind_address: Some("0.0.0.0:21027".to_string()), state: state }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        if let Some(address) = &mut self.bind_address {
            let address = address.clone();
            thread::spawn(move || {
                let r = run_server(address);
                if let Err(_e) = r {
                    println!("Some sort of error happened with the webserver");
                }
            });
        }
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
