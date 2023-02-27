include!(concat!(env!("OUT_DIR"), "/beercanlib.items.rs"));

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


#[macro_export]
macro_rules! receive_message {
    ( $type:ty, $stream:expr )  => {
        {
            let mut len_buf: [u8;4] = [0;4];
            $stream.read_exact(&mut len_buf)?;
            let msg_len: usize = u32::from_be_bytes(len_buf) as usize;

            if msg_len > 2000 {
                println!("too big a buffer {}", msg_len);
                return Ok(1);
            }

            let mut buf = vec![0u8; msg_len];
            $stream.read_exact(&mut buf)?;
            <$type>::decode(&*buf)?;

        }
    };
}

