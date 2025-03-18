use data_encoding::BASE32;
use data_encoding::BASE32_NOPAD;

pub type DeviceID = [u8; 32];

const LUHN32: &[u8] = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".as_bytes();

fn codepoint32(c: char) -> usize {
    if c.is_ascii_uppercase() {
        (c as usize) - ('A' as usize)
    } else if ('2'..='7').contains(&c) {
        (c as usize) + 26 - ('2' as usize)
    } else {
        panic!("no bueno")
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

pub fn deviceid_from_string(id: &str) -> [u8; 32] {
    BASE32_NOPAD
        .decode(unluhnify(&unchunkify(id)).as_bytes())
        .unwrap()
        .try_into()
        .unwrap()
}

/// Converts a device ID to the Syncthing format
pub fn deviceid_to_string(bytes: &DeviceID) -> String {
    let l = BASE32.encode(bytes).replace("=", "");
    chunkify(&luhnify(&l))
}

/// Converts a device ID to the base32. Legacy method for older syncthing clients
pub fn deviceid_to_base32(bytes: &DeviceID) -> String {
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
        let str = deviceid_to_string(&id);
        let byt = deviceid_from_string(&str);
        assert_eq!(byt, id);

        let bytes: DeviceID = BASE32_NOPAD
            .decode(b"DOTMVXYJJDOMO4QTANVMLDQX6HEKDOHU4J2MG6UAGRQAJFNIYSNQ")
            .unwrap()
            .try_into()
            .unwrap();
        let deviceid = "DOTMVXY-JJDOMOF-4QTANVM-LDQX6HX-EKDOHU4-J2MG6UC-AGRQAJF-NIYSNQ4";
        assert_eq!(deviceid_to_string(&bytes), deviceid);
        assert_eq!(bytes, deviceid_from_string(deviceid));
    }
}
