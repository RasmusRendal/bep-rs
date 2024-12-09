use std::fmt::Write;

pub fn bytes_vec_to_hex(bytes: &Vec<u8>) -> String {
    let mut s = String::new();
    for &byte in bytes {
        write!(&mut s, "{:02X}", byte).expect("Unable to write");
    }
    s
}

pub fn bytes_to_hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &byte in bytes {
        write!(&mut s, "{:02X}", byte).expect("Unable to write");
    }
    s
}

pub fn hex_to_bytes(s: &str) -> Vec<u8> {
    let mut v = Vec::new();
    for i in 0..(s.len() / 2) {
        v.push(u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap());
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_to_hex() {
        let bytes: [u8; 4] = [2, 14, 174, 15];
        let hexstr = bytes_to_hex(&bytes);
        let result = hex_to_bytes(&hexstr);
        assert_eq!(bytes.len(), result.len());
        for i in 0..bytes.len() {
            assert_eq!(bytes[i], result[i]);
        }
    }
}
