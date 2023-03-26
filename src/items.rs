use prost::Message;
use std::io;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

include!(concat!(env!("OUT_DIR"), "/beercanlib.items.rs"));

const HELLO_MAGIC: u32 = 0x2EA7D90B_u32;

/// Write a message defined in items.proto to the given stream
#[macro_export]
macro_rules! send_message {
    ( $msg:expr, $stream:expr ) => {{
        let mut msg_len: [u8; 8] = $msg.encoded_len().to_be_bytes();
        $stream.write_all(&mut msg_len[1..4]).await?;
        let mut buf: Vec<u8> = Vec::new();
        buf.reserve_exact($msg.encoded_len());
        let mut buf = $msg.encode_length_delimited_to_vec();
        $stream.write_all(&mut buf).await?;
    }};
}

/// Given a stream, read a four-byte length, and then
/// the message
#[macro_export]
macro_rules! receive_message {
    ( $type:ty, $stream:expr ) => {{
        let mut msg_len: [u8; 4] = [0; 4];
        $stream.read_exact(&mut msg_len).await?;
        let msg_len = u32::from_be_bytes(msg_len) as usize;
        if msg_len == 0 {
            log::error!("Message is empty");
        }
        let mut message_buffer = vec![0u8; msg_len];
        $stream.read_exact(&mut message_buffer).await?;
        <$type>::decode(&*message_buffer)
    }};
}
// TODO: Stop returning integers constantly, figure out how to have a result return type with
// void/error
/// Given a socket, send a BEP hello message
pub async fn send_hello(socket: &mut (impl AsyncWriteExt + Unpin)) -> io::Result<()> {
    let magic = HELLO_MAGIC.to_be_bytes().to_vec();
    socket.write_all(&magic).await?;

    let hello = Hello {
        device_name: "device".to_string(),
        client_name: "beercan".to_string(),
        client_version: "0.1".to_string(),
    };
    socket
        .write_all(&u32::to_be_bytes(hello.encoded_len() as u32))
        .await?;
    socket.write_all(&hello.encode_to_vec()).await?;
    Ok(())
}

pub async fn receive_hello(socket: &mut (impl AsyncReadExt + Unpin)) -> io::Result<Hello> {
    let mut hello_buffer: [u8; 4] = [0; 4];
    socket.read_exact(&mut hello_buffer).await?;
    let magic = u32::from_be_bytes(hello_buffer);
    if magic == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Did not receive any magic bytes",
        ));
    } else if magic != HELLO_MAGIC {
        log::error!("Invalid magic bytes: {:X}, {magic}", magic);
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid magic bytes received",
        ));
    }

    Ok(receive_message!(Hello, socket)?)
}

pub async fn exchange_hellos(
    socket: &mut (impl AsyncReadExt + AsyncWriteExt + Unpin),
) -> io::Result<Hello> {
    send_hello(socket).await?;
    receive_hello(socket).await
}
