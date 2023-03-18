use log::{info, warn, error};
use prost::Message;
use rand::distributions::Standard;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::fs::File;
use std::io::{self, Write};
use super::bep_state;
use super::items;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// TODO: All of these commands assume very sequential messages, with no initiative from the other
// end. This is terrible, and very liable to breaking.

/* Basically overwrites local file with the remote contents
 */
pub async fn get_file(stream: &mut TcpStream, directory: &bep_state::Directory, sync_file: &bep_state::File) -> io::Result<()> {
    info!("Requesting file {}", sync_file.name);

    let header = items::Header { r#type: items::MessageType::Request as i32, compression: items::MessageCompression::r#None as i32 };
    let message_id = StdRng::from_entropy().sample(Standard);

    // TODO: Support bigger files
    let message = items::Request {id: message_id, folder: directory.id.clone(), name: sync_file.name.clone(), offset: 0, size: 8, hash: vec![0], from_temporary: false};

    info!("Sending size {} header", header.encoded_len());
    info!("Sending header {:?}", header.encode_to_vec());
    info!("Sending request {:?}", message);
    stream.write_all(&u16::to_be_bytes(header.encoded_len() as u16)).await?;
    stream.write_all(&header.encode_to_vec()).await?;
    stream.write_all(&u32::to_be_bytes(message.encoded_len() as u32)).await?;
    stream.write_all(&message.encode_to_vec()).await?;

    // Handle response

    let header_len = stream.read_u16().await? as usize;
    let mut header = vec![0u8; header_len];
    stream.read_exact(&mut header).await?;
    let header = items::Header::decode(&*header)?;
    if header.r#type != 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Received unexpected response type"));
    }
    if header.compression != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Received compressed message, but I don't know how to decompress data"));
    }

    let message_len = stream.read_u32().await? as usize;
    let mut message = vec![0u8; message_len];
    stream.read_exact(&mut message).await?;
    let message = items::Response::decode(&*message)?;
    if message.id != message_id {
        error!("Expected message id {}, got {}", message_id, message.id);
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Got out of order Response"));
    }
    if message.code != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "Got error on file request, and I don't know how to handle errors."));
    }

    let mut file = directory.path.clone();
    file.push(sync_file.name.clone());
    info!("Writing to path {:?}", file);
    let mut o = File::create(file)?;
    o.write_all(message.data.as_slice())?;



    Ok(())
}
