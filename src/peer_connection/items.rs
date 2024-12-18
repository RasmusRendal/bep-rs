use std::time::{SystemTime, UNIX_EPOCH};

use prost::Message;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

use super::PeerConnectionError;

include!(concat!(env!("OUT_DIR"), "/bep.items.rs"));

const HELLO_MAGIC: u32 = 0x2EA7D90B_u32;

pub trait EncodableItem {
    fn encode_for_bep(&self) -> Vec<u8>;
}

macro_rules! implement {
    ($msg_type:ident) => {
        impl EncodableItem for $msg_type {
            fn encode_for_bep(&self) -> Vec<u8> {
                let mut message_buffer: Vec<u8> = Vec::new();
                let header = Header {
                    r#type: MessageType::$msg_type as i32,
                    compression: MessageCompression::r#None as i32,
                };

                message_buffer.extend_from_slice(&u16::to_be_bytes(header.encoded_len() as u16));
                message_buffer.append(&mut header.encode_to_vec());
                message_buffer.extend_from_slice(&u32::to_be_bytes(self.encoded_len() as u32));
                message_buffer.append(&mut self.encode_to_vec());
                message_buffer
            }
        }
    };
}

implement!(Close);
implement!(Request);
implement!(Response);
implement!(Index);
implement!(ClusterConfig);
implement!(Ping);

pub async fn receive_message<T: prost::Message + std::default::Default>(
    stream: &mut (impl AsyncRead + Unpin),
    compression: bool,
) -> Result<T, std::io::Error> {
    let msg_len = stream.read_u32().await? as usize;
    if msg_len == 0 {
        log::error!("Message is empty");
    }
    let mut message_buffer = vec![0u8; msg_len];
    stream.read_exact(&mut message_buffer).await?;
    if !compression {
        return Ok(T::decode(std::io::Cursor::new(message_buffer))?);
    }
    let result = lz4_flex::block::decompress_size_prepended(&message_buffer).unwrap();
    Ok(T::decode(std::io::Cursor::new(result))?)
}

/// Given a socket, send a BEP hello message
pub async fn send_hello(
    socket: &mut (impl AsyncWriteExt + Unpin),
    name: String,
) -> Result<(), PeerConnectionError> {
    let magic = HELLO_MAGIC.to_be_bytes().to_vec();
    socket.write_all(&magic).await?;

    let hello = Hello {
        device_name: name,
        client_name: "bep.rs".to_string(),
        client_version: "0.1".to_string(),
        num_connections: 1,
        timestamp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64,
    };
    socket
        .write_all(&u16::to_be_bytes(hello.encoded_len() as u16))
        .await?;
    socket.write_all(&hello.encode_to_vec()).await?;
    socket.flush().await?;
    Ok(())
}

pub async fn receive_hello(
    socket: &mut (impl AsyncReadExt + Unpin),
) -> Result<Hello, PeerConnectionError> {
    let mut hello_buffer: [u8; 4] = [0; 4];
    socket.read_exact(&mut hello_buffer).await?;
    let magic = u32::from_be_bytes(hello_buffer);
    if magic == 0 {
        return Err(PeerConnectionError::InvalidMagicBytes);
    } else if magic != HELLO_MAGIC {
        log::error!("Invalid magic bytes: {:X}, {magic}", magic);
        return Err(PeerConnectionError::InvalidMagicBytes);
    }

    let hello_len = socket.read_u16().await? as usize;
    let mut message_buffer = vec![0u8; hello_len];
    socket.read_exact(&mut message_buffer).await?;
    Ok(Hello::decode(&*message_buffer)?)
}
