//! Décompression DEFLATE (RFC 1951), gzip (RFC 1952) et zlib (RFC 1950).
//!
//! Sert aux trames du flux d'un live TikTok, souvent compressées en gzip, et aux
//! réponses HTTP `Content-Encoding: gzip`. Décodeur canonique (même algorithme
//! que `puff` de zlib) avec une taille de sortie plafonnée : une « bombe » de
//! décompression ne peut pas épuiser la mémoire.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InflateError {
    Truncated,
    Invalid(&'static str),
    TooLarge,
}

impl std::fmt::Display for InflateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InflateError::Truncated => write!(f, "données compressées tronquées"),
            InflateError::Invalid(why) => write!(f, "données compressées invalides ({why})"),
            InflateError::TooLarge => write!(f, "données décompressées trop volumineuses"),
        }
    }
}
impl std::error::Error for InflateError {}

struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit_buf: u64,
    bit_count: u32,
}

impl<'a> Bits<'a> {
    fn need(&mut self, n: u32) -> Result<(), InflateError> {
        while self.bit_count < n {
            let byte = *self.data.get(self.pos).ok_or(InflateError::Truncated)?;
            self.pos += 1;
            self.bit_buf |= u64::from(byte) << self.bit_count;
            self.bit_count += 8;
        }
        Ok(())
    }
    fn bits(&mut self, n: u32) -> Result<u32, InflateError> {
        if n == 0 {
            return Ok(0);
        }
        self.need(n)?;
        let v = (self.bit_buf & ((1u64 << n) - 1)) as u32;
        self.bit_buf >>= n;
        self.bit_count -= n;
        Ok(v)
    }
    fn align(&mut self) {
        self.bit_buf = 0;
        self.bit_count = 0;
    }
}

/// Table de Huffman canonique : nombre de codes par longueur + symboles triés.
struct Huffman {
    counts: [u16; 16],
    symbols: Vec<u16>,
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman, InflateError> {
        let mut counts = [0u16; 16];
        for &l in lengths {
            counts[l as usize] += 1;
        }
        // Code sur-souscrit = invalide (un code incomplet est toléré, comme zlib).
        let mut left: i32 = 1;
        for len in 1..16 {
            left <<= 1;
            left -= i32::from(counts[len]);
            if left < 0 {
                return Err(InflateError::Invalid("table de Huffman sur-souscrite"));
            }
        }
        let mut offsets = [0u16; 16];
        for len in 1..15 {
            offsets[len + 1] = offsets[len] + counts[len];
        }
        let mut symbols = vec![0u16; lengths.len()];
        for (symbol, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbols[offsets[l as usize] as usize] = symbol as u16;
                offsets[l as usize] += 1;
            }
        }
        counts[0] = 0;
        Ok(Huffman { counts, symbols })
    }

    fn decode(&self, bits: &mut Bits) -> Result<u16, InflateError> {
        let mut code: i32 = 0;
        let mut first: i32 = 0;
        let mut index: i32 = 0;
        for len in 1..16 {
            code |= bits.bits(1)? as i32;
            let count = i32::from(self.counts[len]);
            if code - count < first {
                return Ok(self.symbols[(index + (code - first)) as usize]);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(InflateError::Invalid("code de Huffman inconnu"))
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

/// Décompresse un flux DEFLATE brut. Renvoie aussi le nombre d'octets consommés.
pub fn inflate_raw(data: &[u8], max_output: usize) -> Result<(Vec<u8>, usize), InflateError> {
    let mut bits = Bits { data, pos: 0, bit_buf: 0, bit_count: 0 };
    let mut out: Vec<u8> = Vec::new();
    loop {
        let last = bits.bits(1)?;
        match bits.bits(2)? {
            0 => {
                bits.align();
                let header = data.get(bits.pos..bits.pos + 4).ok_or(InflateError::Truncated)?;
                let len = u16::from_le_bytes([header[0], header[1]]);
                let nlen = u16::from_le_bytes([header[2], header[3]]);
                if len != !nlen {
                    return Err(InflateError::Invalid("bloc stocké : longueur incohérente"));
                }
                bits.pos += 4;
                let block = data.get(bits.pos..bits.pos + len as usize).ok_or(InflateError::Truncated)?;
                if out.len() + block.len() > max_output {
                    return Err(InflateError::TooLarge);
                }
                out.extend_from_slice(block);
                bits.pos += len as usize;
            }
            1 => {
                let mut lengths = [0u8; 288];
                lengths[..144].fill(8);
                lengths[144..256].fill(9);
                lengths[256..280].fill(7);
                lengths[280..].fill(8);
                let lit = Huffman::new(&lengths)?;
                let dist = Huffman::new(&[5u8; 30])?;
                codes(&mut bits, &mut out, &lit, &dist, max_output)?;
            }
            2 => {
                let (lit, dist) = dynamic_tables(&mut bits)?;
                codes(&mut bits, &mut out, &lit, &dist, max_output)?;
            }
            _ => return Err(InflateError::Invalid("type de bloc inconnu")),
        }
        if last == 1 {
            break;
        }
    }
    Ok((out, bits.pos))
}

fn dynamic_tables(bits: &mut Bits) -> Result<(Huffman, Huffman), InflateError> {
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let nlen = bits.bits(5)? as usize + 257;
    let ndist = bits.bits(5)? as usize + 1;
    let ncode = bits.bits(4)? as usize + 4;
    if nlen > 286 || ndist > 30 {
        return Err(InflateError::Invalid("trop de codes"));
    }
    let mut code_lengths = [0u8; 19];
    for &index in ORDER.iter().take(ncode) {
        code_lengths[index] = bits.bits(3)? as u8;
    }
    let lencode = Huffman::new(&code_lengths)?;
    let mut lengths = vec![0u8; nlen + ndist];
    let mut i = 0;
    while i < nlen + ndist {
        let symbol = lencode.decode(bits)?;
        if symbol < 16 {
            lengths[i] = symbol as u8;
            i += 1;
            continue;
        }
        let (value, repeat) = match symbol {
            16 => {
                if i == 0 {
                    return Err(InflateError::Invalid("répétition sans longueur précédente"));
                }
                (lengths[i - 1], 3 + bits.bits(2)? as usize)
            }
            17 => (0, 3 + bits.bits(3)? as usize),
            _ => (0, 11 + bits.bits(7)? as usize),
        };
        if i + repeat > nlen + ndist {
            return Err(InflateError::Invalid("trop de longueurs"));
        }
        lengths[i..i + repeat].fill(value);
        i += repeat;
    }
    if lengths[256] == 0 {
        return Err(InflateError::Invalid("pas de code de fin de bloc"));
    }
    Ok((Huffman::new(&lengths[..nlen])?, Huffman::new(&lengths[nlen..])?))
}

fn codes(bits: &mut Bits, out: &mut Vec<u8>, lit: &Huffman, dist: &Huffman, max_output: usize) -> Result<(), InflateError> {
    loop {
        let symbol = lit.decode(bits)?;
        if symbol < 256 {
            if out.len() >= max_output {
                return Err(InflateError::TooLarge);
            }
            out.push(symbol as u8);
        } else if symbol == 256 {
            return Ok(());
        } else {
            let s = (symbol - 257) as usize;
            if s >= 29 {
                return Err(InflateError::Invalid("longueur invalide"));
            }
            let len = LEN_BASE[s] as usize + bits.bits(u32::from(LEN_EXTRA[s]))? as usize;
            let d = dist.decode(bits)? as usize;
            if d >= 30 {
                return Err(InflateError::Invalid("distance invalide"));
            }
            let distance = DIST_BASE[d] as usize + bits.bits(u32::from(DIST_EXTRA[d]))? as usize;
            if distance > out.len() {
                return Err(InflateError::Invalid("distance trop grande"));
            }
            if out.len() + len > max_output {
                return Err(InflateError::TooLarge);
            }
            let start = out.len() - distance;
            for k in 0..len {
                let b = out[start + k];
                out.push(b);
            }
        }
    }
}

/// Données gzip (RFC 1952) : en-tête, DEFLATE, CRC-32 vérifié.
pub fn gunzip(data: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    if data.len() < 18 || data[0] != 0x1f || data[1] != 0x8b {
        return Err(InflateError::Invalid("en-tête gzip absent"));
    }
    if data[2] != 8 {
        return Err(InflateError::Invalid("méthode gzip inconnue"));
    }
    let flags = data[3];
    let mut pos = 10;
    if flags & 0x04 != 0 {
        let extra = data.get(pos..pos + 2).ok_or(InflateError::Truncated)?;
        pos += 2 + u16::from_le_bytes([extra[0], extra[1]]) as usize;
    }
    for flag in [0x08, 0x10] {
        if flags & flag != 0 {
            while *data.get(pos).ok_or(InflateError::Truncated)? != 0 {
                pos += 1;
            }
            pos += 1;
        }
    }
    if flags & 0x02 != 0 {
        pos += 2;
    }
    let body = data.get(pos..).ok_or(InflateError::Truncated)?;
    let (out, used) = inflate_raw(body, max_output)?;
    let trailer = body.get(used..used + 8).ok_or(InflateError::Truncated)?;
    let crc = u32::from_le_bytes([trailer[0], trailer[1], trailer[2], trailer[3]]);
    if crc != crc32(&out) {
        return Err(InflateError::Invalid("CRC gzip incorrect"));
    }
    Ok(out)
}

/// Données zlib (RFC 1950).
pub fn zlib_decompress(data: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    if data.len() < 6 || data[0] & 0x0f != 8 || (u16::from(data[0]) << 8 | u16::from(data[1])) % 31 != 0 {
        return Err(InflateError::Invalid("en-tête zlib absent"));
    }
    if data[1] & 0x20 != 0 {
        return Err(InflateError::Invalid("dictionnaire zlib non pris en charge"));
    }
    Ok(inflate_raw(&data[2..], max_output)?.0)
}

/// gzip, zlib ou DEFLATE brut, selon les premiers octets (comme `zlib.unzip` de Node).
pub fn decompress_auto(data: &[u8], max_output: usize) -> Result<Vec<u8>, InflateError> {
    if data.len() >= 2 && data[0] == 0x1f && data[1] == 0x8b {
        gunzip(data, max_output)
    } else if data.len() >= 2 && data[0] & 0x0f == 8 && (u16::from(data[0]) << 8 | u16::from(data[1])) % 31 == 0 {
        zlib_decompress(data, max_output)
    } else {
        Ok(inflate_raw(data, max_output)?.0)
    }
}

pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// Compression « stockée » (sans réduction) au format gzip : sert seulement aux
/// tests et à produire un gzip valide sans dépendance.
pub fn gzip_store(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 0xff];
    let mut chunks = data.chunks(65535).peekable();
    if chunks.peek().is_none() {
        out.extend_from_slice(&[1, 0, 0, 0xff, 0xff]);
    }
    while let Some(chunk) = chunks.next() {
        out.push(if chunks.peek().is_none() { 1 } else { 0 });
        let len = chunk.len() as u16;
        out.extend_from_slice(&len.to_le_bytes());
        out.extend_from_slice(&(!len).to_le_bytes());
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&crc32(data).to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_gzip_from_node() {
        // `zlib.gzipSync("Bonjour Batlay ! Bonjour Batlay ! Bonjour Batlay !")` (Node 20).
        let gz = crate::util::encoding::base64_decode("H4sIAAAAAAAAA3PKz8vKLy1ScEosyUmsVFBUcCIkAADUtwhSMgAAAA==").unwrap();
        assert_eq!(gunzip(&gz, 1000).unwrap(), b"Bonjour Batlay ! Bonjour Batlay ! Bonjour Batlay !");
        assert_eq!(decompress_auto(&gz, 1000).unwrap(), b"Bonjour Batlay ! Bonjour Batlay ! Bonjour Batlay !");
    }

    #[test]
    fn dynamic_huffman_block() {
        // `zlib.deflateSync(texte de 600 caractères variés)` : bloc à tables dynamiques.
        let z = crate::util::encoding::base64_decode(
            "eJw90M1tQjEQReFWpgT8byu7rLOkAUOM9IThRTYkUBJ10FiE0Jylz2L0XX81OfTrTb6v0pffJtvluF2PUsd4vdpZLqOe2pTdcq7LaFP26+lntDmfjzblQw5r7+ufbOSzXnq9i3GabNTmirZgtcWgLWVtxdA02Y0m57T5yLmiLVltOUDRZDznkjaPOCCOiBPigjgzzDDMcw5xRJwQZ8RcM+y37PeIA+KIOCMuiNlv2e/Y7xFHxAlxRsw1w37Lfo84II6IM+KC2Lw/4B8KHcEH",
        )
        .unwrap();
        let out = zlib_decompress(&z, 10_000).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("Le flux du live TikTok"), "{text}");
        assert_eq!(text.len(), 601);
        assert_eq!(text.chars().count(), 600);
        assert!(text.contains("follow") && text.contains("Batlay"));
        assert_eq!(z[2] & 0b110, 0b100, "bloc à tables dynamiques");
    }

    #[test]
    fn stored_round_trip_and_limits() {
        let data: Vec<u8> = (0..200_000u32).map(|i| (i % 253) as u8).collect();
        let gz = gzip_store(&data);
        assert_eq!(gunzip(&gz, 300_000).unwrap(), data);
        assert_eq!(gunzip(&gz, 1000), Err(InflateError::TooLarge));
        let mut corrupt = gz.clone();
        let n = corrupt.len();
        corrupt[n - 6] ^= 1;
        assert!(gunzip(&corrupt, 300_000).is_err());
        assert!(gunzip(&gz[..gz.len() / 2], 300_000).is_err());
        assert_eq!(gunzip(&gzip_store(b""), 10).unwrap(), b"");
    }

    #[test]
    fn crc_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }

    #[test]
    fn garbage_never_panics() {
        let mut seed = 12345u32;
        for len in 0..300 {
            let data: Vec<u8> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
                    (seed >> 16) as u8
                })
                .collect();
            let _ = inflate_raw(&data, 4096);
            let _ = decompress_auto(&data, 4096);
        }
    }
}
