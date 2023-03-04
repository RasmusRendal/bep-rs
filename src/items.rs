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
            $stream.read_exact(&mut msg_len)?;
            let msg_len = u32::from_be_bytes(msg_len) as usize;
            if msg_len == 0 {
                println!("Message is empty");
            }
            let mut message_buffer = vec![0u8; msg_len];
            $stream.read_exact(&mut message_buffer)?;
            <$type>::decode(&*message_buffer)
        }
    };
}
