use std::error::Error;
use super::bep_state::BepState;
use std::io::{self, Read};
use mio::net::{TcpStream, TcpListener};
use mio::{Events, Interest, Poll, Token};
use log::{info, warn, error, debug};

use std::collections::HashMap;

use prost::Message;
use super::items;
use super::daemon::send_hello;

const SERVER: Token = Token(0);

pub struct Server {
    bind_address: Option<String>,
    state: BepState,
}

/// Receive some messages from a new client, and print them
fn handle_connection(stream: &mut TcpStream) -> io::Result<bool> {
    let mut hello_buffer: [u8;4] = [0;4];
    stream.read_exact(&mut hello_buffer)?;
    let magic = u32::from_be_bytes(hello_buffer);
    if magic == 0 {
        // The connection has been closed / There is no message for us
        return Ok(false);
    } else if magic != 0x2EA7D90B {
        error!("Invalid magic bytes: {:X}, {magic}", magic);
        //TODO: Find out how to return a proper error
        return Ok(false);
    }

    let hello = receive_message!(items::Hello, stream)?;
    info!(target: "Server", "{:?}", hello);

    Ok(true)
}

/// Listen for connections at address, printing whatever the client sends us
fn run_server(address: String) -> io::Result<()> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(128);
    let mut listener = TcpListener::bind(address.parse().unwrap()).unwrap();

    poll.registry()
        .register(&mut listener, SERVER, Interest::READABLE)?;

    let mut counter: usize = 1;
    let mut sockets: HashMap<Token, TcpStream> = HashMap::new();

    loop {
        poll.poll(&mut events, None)?;
        for event in events.iter() {
            match event.token() {
                SERVER => loop {
                    let (mut connection, _) = match listener.accept() {
                        Ok((connection, address)) => (connection, address),
                        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                            // If we get a `WouldBlock` error we know our
                            // listener has no more incoming connections queued,
                            // so we can return to polling and wait for some
                            // more.
                            break;
                        }
                        Err(e) => {
                            // If it was any other kind of error, something went
                            // wrong and we terminate with an error.
                            //return Err(e);
                            break;
                        }
                    };
                    counter += 1;
                    let token = Token(counter);
                    poll.registry().register(&mut connection, token, Interest::READABLE)?;
                    sockets.insert(token, connection);
                }
                token => {
                    let done = if let Some(mut connection) = sockets.get_mut(&token) {
                        handle_connection(&mut connection).unwrap_or(false)
                    } else {
                        // Sporadic events happen, we can safely ignore them.
                        false
                    };
                    if done {
                        if let Some(mut connection) = sockets.remove(&token) {
                            poll.registry().deregister(&mut connection)?;
                        }
                    }
                }
            }
        }
    }

}


impl Server {
    pub fn new(state: BepState) -> Self {
        Server { bind_address: Some("0.0.0.0:21027".to_string()), state: state }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        let address = self.bind_address.clone().unwrap();
        info!("Starting server, listening on {} ...",address);
        run_server(address)?;
        Ok(0)
    }


}
