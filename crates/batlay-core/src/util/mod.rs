//! Briques sans dépendance : encodages, empreintes, décompression, adresses, texte, aléa.

pub mod encoding;
pub mod hash;
pub mod inflate;
pub mod random;
pub mod text;
pub mod url;

/// Horloge murale en millisecondes depuis l'époque Unix (comme `Date.now()`).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
