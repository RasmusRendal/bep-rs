use std::{thread, time};
use super::bep_state::BepState;

pub struct Daemon {
    bind_address: Option<String>,
    state: BepState,
    bind_port: u16,
}

impl Daemon {
    pub fn new(state: BepState) -> Self {
        Daemon { bind_address: None, state: state, bind_port: 21027 }
    }

    pub fn run(&mut self) {
        loop {
            thread::sleep(time::Duration::from_millis(1000));
        }
    }
}
