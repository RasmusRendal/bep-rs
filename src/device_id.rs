use data_encoding::BASE32;
use data_encoding::BASE32_NOPAD;

const LUHN32: &[u8] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".as_bytes();

fn codepoint32(c: char) -> usize {
    if 'A' <= c && c <= 'Z' {
        return (c as usize) - ('A' as usize);
    } else if '2' <= c && c <= '7' {
        return (c as usize) + 26 - ('2' as usize);
    } else {
        panic!("no bueno");
    }
}

fn syncthing_luhn32(s: &str) -> char {
    let mut factor: usize = 1;
    let mut sum: usize = 0;
    const N: usize = 32;

    for c in s.chars() {
        let mut addend = factor * codepoint32(c);
        if factor == 2 {
            factor = 1;
        } else {
            factor = 2;
        }
        addend = (addend / N) + (addend % N);
        sum += addend;
    }
    let remainder = sum % N;
    let index = (N - remainder) % N;
    LUHN32[index] as char
}

fn luhnify(s: &str) -> String {
    assert!(s.len() == 52);
    let mut out = "".to_string();
    for i in 0..4 {
        let part = &s[i * 13..(i + 1) * 13];
        let l = syncthing_luhn32(part);
        out.push_str(part);
        out.push(l);
    }
    out
}

fn unluhnify(s: &str) -> String {
    let mut out = "".to_string();
    for i in 0..4 {
        let part = &s[i * (13 + 1)..(i + 1) * (13 + 1) - 1];
        let l = syncthing_luhn32(part);
        let c = s.as_bytes()[(i + 1) * 14 - 1] as char;
        assert!(l == c);
        out.push_str(part);
    }
    out
}

fn chunkify(s: &str) -> String {
    let mut out = "".to_string();
    let num_chunks = s.len() / 7;
    for i in 0..num_chunks {
        let part = &s[i * 7..(i + 1) * 7];
        out.push_str(part);
        if i != num_chunks - 1 {
            out.push('-');
        }
    }
    out
}

fn unchunkify(s: &str) -> String {
    s.replace("-", "")
}

pub fn deviceid_to_bytes(id: &str) -> [u8; 32] {
    BASE32_NOPAD
        .decode(unluhnify(&unchunkify(id)).as_bytes())
        .unwrap()
        .try_into()
        .unwrap()
}

pub fn bytes_to_deviceid(bytes: &[u8; 32]) -> String {
    let l = BASE32.encode(bytes).replace("=", "");
    chunkify(&luhnify(&l))
}

pub fn bytes_to_base32(bytes: &[u8; 32]) -> String {
    BASE32.encode(bytes).replace("=", "")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_id() {
        let id: [u8; 32] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24,
            25, 26, 27, 28, 29, 30, 31, 32,
        ];
        let str = bytes_to_deviceid(&id);
        let byt = deviceid_to_bytes(&str);
        assert_eq!(byt, id);

        let bytes: [u8; 32] = BASE32_NOPAD
            .decode(b"DOTMVXYJJDOMO4QTANVMLDQX6HEKDOHU4J2MG6UAGRQAJFNIYSNQ")
            .unwrap()
            .try_into()
            .unwrap();
        let deviceid = "DOTMVXY-JJDOMOF-4QTANVM-LDQX6HX-EKDOHU4-J2MG6UC-AGRQAJF-NIYSNQ4";
        assert_eq!(bytes_to_deviceid(&bytes), deviceid);
        assert_eq!(bytes, deviceid_to_bytes(deviceid));
    }
}
