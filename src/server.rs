use std::error::Error;
use super::bep_state::BepState;
use std::io::{Write, Read};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};


const SERVER: Token = Token(0);

pub struct Server {
    bind_address: Option<String>,
    state: BepState,
}

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


impl Server {
    pub fn new(state: BepState) -> Self {
        Server { bind_address: Some("0.0.0.0:21027".to_string()), state: state }
    }

    pub fn set_address(&mut self, new_addr: String) {
        self.bind_address = Some(new_addr);
    }

    pub fn run(&mut self) -> Result<i32, Box<dyn Error>> {
        let address = self.bind_address.clone().unwrap();
        let r = run_server(address);
        Ok(0)
    }


}
