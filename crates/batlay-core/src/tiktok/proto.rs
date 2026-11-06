//! Décodeur protobuf minimal (format « wire ») : juste ce qu'il faut pour lire
//! les messages du flux d'un live TikTok. Entrée non fiable : toute anomalie
//! donne une erreur, jamais une panique ; imbrication bornée.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wire<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtoError(pub &'static str);

impl std::fmt::Display for ProtoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "protobuf invalide ({})", self.0)
    }
}
impl std::error::Error for ProtoError {}

fn varint(data: &[u8], pos: &mut usize) -> Result<u64, ProtoError> {
    let mut value: u64 = 0;
    for shift in (0..64).step_by(7) {
        let b = *data.get(*pos).ok_or(ProtoError("varint tronqué"))?;
        *pos += 1;
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(ProtoError("varint trop long"))
}

/// Message décodé (champs dans l'ordre d'arrivée).
#[derive(Debug, Clone, Default)]
pub struct Msg<'a> {
    fields: Vec<(u32, Wire<'a>)>,
}

impl<'a> Msg<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Msg<'a>, ProtoError> {
        let mut fields = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let key = varint(data, &mut pos)?;
            let number = (key >> 3) as u32;
            if number == 0 {
                return Err(ProtoError("champ 0"));
            }
            let wire = match key & 7 {
                0 => Wire::Varint(varint(data, &mut pos)?),
                1 => {
                    let b = data.get(pos..pos + 8).ok_or(ProtoError("fixed64 tronqué"))?;
                    pos += 8;
                    Wire::Fixed64(u64::from_le_bytes(b.try_into().map_err(|_| ProtoError("fixed64"))?))
                }
                2 => {
                    let len = varint(data, &mut pos)? as usize;
                    let end = pos.checked_add(len).ok_or(ProtoError("longueur"))?;
                    let b = data.get(pos..end).ok_or(ProtoError("champ tronqué"))?;
                    pos = end;
                    Wire::Bytes(b)
                }
                5 => {
                    let b = data.get(pos..pos + 4).ok_or(ProtoError("fixed32 tronqué"))?;
                    pos += 4;
                    Wire::Fixed32(u32::from_le_bytes(b.try_into().map_err(|_| ProtoError("fixed32"))?))
                }
                _ => return Err(ProtoError("type de champ non pris en charge")),
            };
            fields.push((number, wire));
        }
        Ok(Msg { fields })
    }

    fn last(&self, n: u32) -> Option<&Wire<'a>> {
        self.fields.iter().rev().find(|(k, _)| *k == n).map(|(_, w)| w)
    }

    pub fn uint(&self, n: u32) -> Option<u64> {
        match self.last(n)? {
            Wire::Varint(v) | Wire::Fixed64(v) => Some(*v),
            Wire::Fixed32(v) => Some(u64::from(*v)),
            Wire::Bytes(_) => None,
        }
    }

    pub fn bytes(&self, n: u32) -> Option<&'a [u8]> {
        match self.last(n)? {
            Wire::Bytes(b) => Some(b),
            _ => None,
        }
    }

    /// Chaîne UTF-8 (octets invalides remplacés).
    pub fn string(&self, n: u32) -> Option<String> {
        self.bytes(n).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// Sous-message (vide s'il est absent ; `None` s'il est illisible).
    pub fn msg(&self, n: u32) -> Option<Msg<'a>> {
        match self.bytes(n) {
            Some(b) => Msg::parse(b).ok(),
            None => Some(Msg::default()),
        }
    }

    /// Sous-messages répétés lisibles (les illisibles sont ignorés).
    pub fn msgs(&self, n: u32) -> Vec<Msg<'a>> {
        self.fields
            .iter()
            .filter(|(k, _)| *k == n)
            .filter_map(|(_, w)| match w {
                Wire::Bytes(b) => Msg::parse(b).ok(),
                _ => None,
            })
            .collect()
    }

    /// Entrées d'une `map<string, string>`.
    pub fn string_map(&self, n: u32) -> Vec<(String, String)> {
        self.msgs(n).into_iter().map(|e| (e.string(1).unwrap_or_default(), e.string(2).unwrap_or_default())).collect()
    }
}

pub mod encode {
    //! Encodeur minimal : trames envoyées au flux TikTok (méthode Euler) et messages des tests.
    pub fn varint(mut v: u64, out: &mut Vec<u8>) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
    pub fn field_varint(n: u32, v: u64, out: &mut Vec<u8>) {
        varint(u64::from(n) << 3, out);
        varint(v, out);
    }
    pub fn field_bytes(n: u32, b: &[u8], out: &mut Vec<u8>) {
        varint((u64::from(n) << 3) | 2, out);
        varint(b.len() as u64, out);
        out.extend_from_slice(b);
    }
}

#[cfg(test)]
mod tests {
    use super::encode::*;
    use super::*;

    #[test]
    fn decodes_nested_and_repeated() {
        let mut inner = Vec::new();
        field_bytes(3, "Lina ✨".as_bytes(), &mut inner);
        field_varint(1, 6812345678901234567, &mut inner);
        let mut outer = Vec::new();
        field_bytes(2, &inner, &mut outer);
        field_bytes(3, b"salut", &mut outer);
        field_bytes(3, b"re", &mut outer);
        outer.extend_from_slice(&[0x0d, 1, 0, 0, 0]); // fixed32, champ 1
        let m = Msg::parse(&outer).unwrap();
        assert_eq!(m.string(3).as_deref(), Some("re"), "le dernier gagne");
        let u = m.msg(2).unwrap();
        assert_eq!((u.uint(1), u.string(3).as_deref()), (Some(6812345678901234567), Some("Lina ✨")));
        assert_eq!(m.uint(1), Some(1));
        assert!(m.msg(9).unwrap().uint(1).is_none(), "absent = message vide");
    }

    #[test]
    fn garbage_is_an_error_never_a_panic() {
        for bad in [&[0x08][..], &[0x0a, 0x05, 1], &[0x07], &[0x00, 0x01], &[0xff; 11]] {
            assert!(Msg::parse(bad).is_err(), "{bad:?}");
        }
        let mut seed = 7u32;
        for len in 0..400 {
            let data: Vec<u8> = (0..len)
                .map(|_| {
                    seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                    (seed >> 24) as u8
                })
                .collect();
            if let Ok(m) = Msg::parse(&data) {
                let _ = (m.msgs(1), m.string(2), m.msg(3));
            }
        }
    }
}
