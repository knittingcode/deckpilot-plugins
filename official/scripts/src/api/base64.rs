//! Standard base64 (RFC 4648 §4, with padding), the wire encoding of file
//! content in chunked I/O (`api::fs`). Small enough to keep here rather
//! than add a dependency to a module whose every byte is reviewed.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// The length of the encoding of `n` bytes.
pub const fn encoded_len(n: usize) -> usize {
    n.div_ceil(3) * 4
}

pub fn encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(encoded_len(bytes.len()));
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn value(c: u8) -> Option<u32> {
    Some(match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    } as u32)
}

/// Decodes canonical padded base64; `None` for anything else (a wrong
/// length, a character outside the alphabet, misplaced padding, or
/// non-zero bits after the last byte).
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    let quads = bytes.len() / 4;
    for (q, quad) in bytes.chunks(4).enumerate() {
        let last = q + 1 == quads;
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for &c in &quad[..4 - pad] {
            n = (n << 6) | value(c)?;
        }
        n <<= 6 * pad as u32;
        let decoded = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
        let keep = 3 - pad;
        // Canonical: the bits a padded quad drops are zero.
        if decoded[keep..].iter().any(|&b| b != 0) {
            return None;
        }
        out.extend_from_slice(&decoded[..keep]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_rfc_vectors() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(encode(plain.as_bytes()), encoded);
            assert_eq!(decode(encoded).unwrap(), plain.as_bytes());
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(decode(&encode(&all)).unwrap(), all);
        assert_eq!(encoded_len(32 * 1024), encode(&[0; 32 * 1024]).len());
    }

    #[test]
    fn refuses_what_is_not_canonical_base64() {
        for bad in [
            "Zg", "Zg=", "Zg===", "Z===", "Zg==Zg==", "Zh==", "Zm9=", "Zm 9v", "Zm9v\n", "Zm-_",
            "====",
        ] {
            assert!(decode(bad).is_none(), "{bad}");
        }
    }
}
