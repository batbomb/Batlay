//! Base64 (standard et « URL »), hexadécimal, UTF-16LE.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn encode_with(data: &[u8], alphabet: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        let chars = [(n >> 18) & 63, (n >> 12) & 63, (n >> 6) & 63, n & 63];
        let keep = chunk.len() + 1;
        for (i, c) in chars.iter().enumerate() {
            if i < keep {
                out.push(alphabet[*c as usize] as char);
            } else if pad {
                out.push('=');
            }
        }
    }
    out
}

pub fn base64_encode(data: &[u8]) -> String {
    encode_with(data, STD, true)
}

/// Base64 « URL » sans `=` (PKCE, jetons).
pub fn base64url_encode(data: &[u8]) -> String {
    encode_with(data, URL, false)
}

/// Décode le Base64 standard OU « URL », avec ou sans `=`. Les blancs sont ignorés.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut padding = 0;
    for c in text.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        if padding > 0 {
            return None; // données après le remplissage
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if padding > 2 || bits >= 6 {
        return None;
    }
    Some(out)
}

pub fn hex_encode(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    out
}

pub fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Texte → octets UTF-16 petit-boutiste (`powershell -EncodedCommand`).
pub fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(|u| u.to_le_bytes()).collect()
}

/// Comparaison en temps constant (clés, `state` OAuth).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vectors() {
        for (plain, enc) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64_encode(plain.as_bytes()), enc);
            assert_eq!(base64_decode(enc).unwrap(), plain.as_bytes());
        }
        assert_eq!(base64url_encode(&[0xfb, 0xff]), "-_8");
        assert_eq!(base64_decode("-_8").unwrap(), vec![0xfb, 0xff]);
        assert_eq!(base64_decode("Zm9v\r\nYmFy").unwrap(), b"foobar");
        assert!(base64_decode("Zm9v!").is_none());
        assert!(base64_decode("Zg==Zg").is_none());
    }

    #[test]
    fn hex_and_utf16() {
        assert_eq!(hex_encode(&[0, 15, 255]), "000fff");
        assert_eq!(hex_decode("000fff").unwrap(), vec![0, 15, 255]);
        assert!(hex_decode("0g").is_none());
        assert_eq!(utf16le("Aé"), vec![0x41, 0, 0xe9, 0]);
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
