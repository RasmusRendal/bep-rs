include!(concat!(env!("OUT_DIR"), "/beercanlib.items.rs"));

/// Write a message defined in items.proto to the given stream
#[macro_export]
macro_rules! send_message {
    ( $msg:expr, $stream:expr )  => {
        {
            let mut msg_len: [u8;8] = $msg.encoded_len().to_be_bytes();
            $stream.write(&mut msg_len[1..4])?;
            let mut buf: Vec<u8> = Vec::new();
            buf.reserve_exact($msg.encoded_len());
            let mut buf = $msg.encode_length_delimited_to_vec();
            $stream.write(&mut buf)?;

        }
    };
}

/// Given a stream, read a four-byte length, and then
/// the message
#[macro_export]
macro_rules! receive_message {
    ( $type:ty, $stream:expr )  => {
        {
            let mut msg_len: [u8;4] = [0;4];
            $stream.read_exact(&mut msg_len).await?;
            let msg_len = u32::from_be_bytes(msg_len) as usize;
            if msg_len == 0 {
                error!("Message is empty");
            }
            let mut message_buffer = vec![0u8; msg_len];
            $stream.read_exact(&mut message_buffer).await?;
            <$type>::decode(&*message_buffer)
        }
    };
}

use tokio::net::TcpStream;
use std::io::{self, Read};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use prost::Message;
use log::{info, warn, error, debug};

// TODO: Stop returning integers constantly, figure out how to have a result return type with
// void/error
/// Given a socket, send a BEP hello message
pub async fn send_hello(socket: &mut TcpStream) -> io::Result<()> {
    let magic = (0x2EA7D90B as u32).to_be_bytes().to_vec();
    socket.write_all(&magic).await?;

    let hello = Hello {device_name: "device".to_string(), client_name: "beercan".to_string(), client_version: "0.1".to_string()};
    socket.write_all(&u32::to_be_bytes(hello.encoded_len() as u32)).await?;
    socket.write_all(&hello.encode_to_vec()).await?;
    Ok(())
}

pub async fn receive_hello(socket: &mut TcpStream) -> io::Result<()> {
    let mut hello_buffer: [u8;4] = [0;4];
    socket.read_exact(&mut hello_buffer).await?;
    let magic = u32::from_be_bytes(hello_buffer);
    if magic == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Did not receive any magic bytes"));
    } else if magic != 0x2EA7D90B {
        error!("Invalid magic bytes: {:X}, {magic}", magic);
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid magic bytes received"));
    }

    let hello = receive_message!(Hello, socket)?;

    info!("Received hello from {}: {:?}", socket.peer_addr()?, hello);
    Ok(())

}

pub async fn exchange_hellos(socket: &mut TcpStream) -> io::Result<()> {
    send_hello(socket).await?;
    receive_hello(socket).await?;
    Ok(())
}

