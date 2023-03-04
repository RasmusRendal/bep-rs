use std::error::Error;
use super::bep_state::BepState;
use std::io::Read;
use mio::net::{TcpStream, TcpListener};
use mio::{Events, Interest, Poll, Token};

use std::collections::HashMap;

use prost::Message;
use super::items;

const SERVER: Token = Token(0);

pub struct Server {
    bind_address: Option<String>,
    state: BepState,
}

fn handle_connection(stream: &mut TcpStream) -> Result<i32, Box<dyn Error>> {
    let mut buffer = Vec::new();
    let l = stream.read_to_end(&mut buffer)?;
    if l == 0 {
        return Ok(1);
    }
    let magic = u32::from_be_bytes(buffer[0..4].try_into()?);
    if magic != 0x2EA7D90B {
        println!("Invalid magic bytes: {:X}, {magic}", magic);
        //TODO: Find out how to return a proper error
        return Ok(1);
    }

    let msg_buf = &buffer[4..];
    let hello = parse_message!(items::Hello, msg_buf);
    println!("{:?}", hello);

    Ok(1)
}

fn run_server(address: String) -> Result<i32, Box<dyn Error>> {
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
                SERVER => {
                    let connection = listener.accept();
                    if let Ok((mut stream, _addr)) = connection {
                        counter += 1;
                        let token = Token(counter);
                        poll.registry().register(&mut stream, token, Interest::WRITABLE)?;
                        sockets.insert(token, stream);
                    } else {
                        drop(connection);
                    }
                }
                token if event.is_writable() => {
                    let mut w = sockets.get_mut(&token).unwrap();
                    if let Err(e) = handle_connection(&mut w) {
                        println!("Got error while handling request {}", e);
                    }
                }
                Token(_) => {
                    println!("Don't know what do do with this connection. Dropping");
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
        run_server(address)?;
        Ok(0)
    }


}
