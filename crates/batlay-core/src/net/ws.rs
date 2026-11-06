//! WebSocket (RFC 6455) : poignée de main et trames, côté serveur et côté client.

use crate::util::encoding::base64_encode;
use crate::util::hash::sha1;
use std::io::{self, Read, Write};

const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Valeur de `Sec-WebSocket-Accept` pour une clé donnée.
pub fn accept_key(key: &str) -> String {
    base64_encode(&sha1(format!("{}{}", key.trim(), GUID).as_bytes()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opcode {
    Continuation = 0,
    Text = 1,
    Binary = 2,
    Close = 8,
    Ping = 9,
    Pong = 10,
}

impl Opcode {
    fn from(v: u8) -> Option<Opcode> {
        Some(match v {
            0 => Opcode::Continuation,
            1 => Opcode::Text,
            2 => Opcode::Binary,
            8 => Opcode::Close,
            9 => Opcode::Ping,
            10 => Opcode::Pong,
            _ => return None,
        })
    }
    fn is_control(self) -> bool {
        (self as u8) >= 8
    }
}

#[derive(Debug)]
pub struct Frame {
    pub fin: bool,
    pub opcode: Opcode,
    pub payload: Vec<u8>,
}

/// Écrit une trame. `mask` : obligatoire côté client (clé de 4 octets), interdit côté serveur.
pub fn write_frame(out: &mut impl Write, opcode: Opcode, payload: &[u8], mask: Option<[u8; 4]>) -> io::Result<()> {
    let mut header = Vec::with_capacity(14);
    header.push(0x80 | opcode as u8);
    let mask_bit = if mask.is_some() { 0x80 } else { 0 };
    match payload.len() {
        n if n < 126 => header.push(mask_bit | n as u8),
        n if n <= 0xFFFF => {
            header.push(mask_bit | 126);
            header.extend_from_slice(&(n as u16).to_be_bytes());
        }
        n => {
            header.push(mask_bit | 127);
            header.extend_from_slice(&(n as u64).to_be_bytes());
        }
    }
    match mask {
        Some(key) => {
            header.extend_from_slice(&key);
            let mut masked = payload.to_vec();
            for (i, b) in masked.iter_mut().enumerate() {
                *b ^= key[i % 4];
            }
            header.extend_from_slice(&masked);
            out.write_all(&header)?;
        }
        None => {
            header.extend_from_slice(payload);
            out.write_all(&header)?;
        }
    }
    out.flush()
}

#[derive(Debug)]
pub enum FrameError {
    Io(io::Error),
    Protocol(&'static str),
    TooLarge,
}

impl From<io::Error> for FrameError {
    fn from(e: io::Error) -> Self {
        FrameError::Io(e)
    }
}

/// Lit une trame. `expect_masked` : vrai côté serveur (les clients doivent masquer).
pub fn read_frame(input: &mut impl Read, expect_masked: bool, max_payload: usize) -> Result<Frame, FrameError> {
    let mut head = [0u8; 2];
    input.read_exact(&mut head)?;
    if head[0] & 0x70 != 0 {
        return Err(FrameError::Protocol("bits réservés"));
    }
    let fin = head[0] & 0x80 != 0;
    let opcode = Opcode::from(head[0] & 0x0F).ok_or(FrameError::Protocol("opcode inconnu"))?;
    let masked = head[1] & 0x80 != 0;
    if masked != expect_masked {
        return Err(FrameError::Protocol("masquage incorrect"));
    }
    let len = match head[1] & 0x7F {
        126 => {
            let mut b = [0u8; 2];
            input.read_exact(&mut b)?;
            u64::from(u16::from_be_bytes(b))
        }
        127 => {
            let mut b = [0u8; 8];
            input.read_exact(&mut b)?;
            u64::from_be_bytes(b)
        }
        n => u64::from(n),
    };
    if opcode.is_control() && (len > 125 || !fin) {
        return Err(FrameError::Protocol("trame de contrôle invalide"));
    }
    if len > max_payload as u64 {
        return Err(FrameError::TooLarge);
    }
    let mut key = [0u8; 4];
    if masked {
        input.read_exact(&mut key)?;
    }
    let mut payload = vec![0u8; len as usize];
    input.read_exact(&mut payload)?;
    if masked {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= key[i % 4];
        }
    }
    Ok(Frame { fin, opcode, payload })
}

/// Message complet (trames de continuation réassemblées).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
    Close(Option<u16>, String),
}

/// Réassemble les messages fragmentés ; les trames de contrôle passent au travers.
#[derive(Default)]
pub struct Assembler {
    partial: Option<(Opcode, Vec<u8>)>,
}

impl Assembler {
    pub fn push(&mut self, frame: Frame, max_message: usize) -> Result<Option<Message>, FrameError> {
        match frame.opcode {
            Opcode::Ping => return Ok(Some(Message::Ping(frame.payload))),
            Opcode::Pong => return Ok(Some(Message::Pong(frame.payload))),
            Opcode::Close => {
                let code = (frame.payload.len() >= 2).then(|| u16::from_be_bytes([frame.payload[0], frame.payload[1]]));
                let reason = String::from_utf8_lossy(frame.payload.get(2..).unwrap_or(&[])).into_owned();
                return Ok(Some(Message::Close(code, reason)));
            }
            Opcode::Continuation => {
                let (_, buf) = self.partial.as_mut().ok_or(FrameError::Protocol("continuation inattendue"))?;
                buf.extend_from_slice(&frame.payload);
                if buf.len() > max_message {
                    return Err(FrameError::TooLarge);
                }
            }
            Opcode::Text | Opcode::Binary => {
                if self.partial.is_some() {
                    return Err(FrameError::Protocol("message non terminé"));
                }
                self.partial = Some((frame.opcode, frame.payload));
            }
        }
        if !frame.fin {
            return Ok(None);
        }
        let (opcode, data) = self.partial.take().expect("message en cours");
        Ok(Some(match opcode {
            Opcode::Text => Message::Text(String::from_utf8(data).map_err(|_| FrameError::Protocol("texte non UTF-8"))?),
            _ => Message::Binary(data),
        }))
    }
}

/// Corps d'une trame de fermeture : code + raison (≤ 123 octets).
pub fn close_payload(code: u16, reason: &str) -> Vec<u8> {
    let mut out = code.to_be_bytes().to_vec();
    let mut r = reason.as_bytes();
    while r.len() > 123 {
        // coupe sur une frontière de caractère
        let mut end = 123;
        while !reason.is_char_boundary(end) {
            end -= 1;
        }
        r = &reason.as_bytes()[..end];
    }
    out.extend_from_slice(r);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_vector_from_rfc() {
        assert_eq!(accept_key("dGhlIHNhbXBsZSBub25jZQ=="), "s3pPLMBiTxaQ9kYGzzhZRbK+xOo=");
    }

    #[test]
    fn frames_round_trip_both_directions() {
        for size in [0usize, 5, 125, 126, 70_000] {
            let payload: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();
            let mut buf = Vec::new();
            write_frame(&mut buf, Opcode::Binary, &payload, Some([1, 2, 3, 4])).unwrap();
            let f = read_frame(&mut buf.as_slice(), true, 100_000).unwrap();
            assert_eq!((f.fin, f.opcode, f.payload), (true, Opcode::Binary, payload.clone()));
            let mut buf = Vec::new();
            write_frame(&mut buf, Opcode::Text, &payload, None).unwrap();
            assert!(read_frame(&mut buf.as_slice(), true, 100_000).is_err(), "un client doit masquer");
        }
    }

    #[test]
    fn limits_and_fragments() {
        let mut buf = Vec::new();
        write_frame(&mut buf, Opcode::Text, &[b'a'; 2000], Some([9, 9, 9, 9])).unwrap();
        assert!(matches!(read_frame(&mut buf.as_slice(), true, 1024), Err(FrameError::TooLarge)));

        let mut a = Assembler::default();
        assert_eq!(a.push(Frame { fin: false, opcode: Opcode::Text, payload: b"Bon".to_vec() }, 100).unwrap(), None);
        assert_eq!(a.push(Frame { fin: true, opcode: Opcode::Ping, payload: vec![] }, 100).unwrap(), Some(Message::Ping(vec![])));
        assert_eq!(a.push(Frame { fin: true, opcode: Opcode::Continuation, payload: "jour é".into() }, 100).unwrap(), Some(Message::Text("Bonjour é".into())));
        assert!(a.push(Frame { fin: true, opcode: Opcode::Continuation, payload: vec![] }, 100).is_err());
        let close = a.push(Frame { fin: true, opcode: Opcode::Close, payload: close_payload(1008, "révoqué") }, 100).unwrap();
        assert_eq!(close, Some(Message::Close(Some(1008), "révoqué".into())));
    }
}
