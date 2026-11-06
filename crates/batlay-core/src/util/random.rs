//! Aléa cryptographique du système (clés d'adresse locale, `state` OAuth, PKCE).
//!
//! Windows : `ProcessPrng` (bcryptprimitives.dll), le générateur qu'utilise la
//! bibliothèque standard de Rust elle-même. Ailleurs : `/dev/urandom`.

#[cfg(windows)]
#[link(name = "bcryptprimitives", kind = "raw-dylib")]
extern "system" {
    fn ProcessPrng(data: *mut u8, len: usize) -> i32;
}

/// `n` octets aléatoires. Panique seulement si le système n'a AUCUNE source d'aléa
/// (situation où continuer serait dangereux : clés prévisibles).
pub fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    fill(&mut buf);
    buf
}

#[cfg(windows)]
fn fill(buf: &mut [u8]) {
    // SAFETY : tampon valide de `buf.len()` octets ; ProcessPrng renvoie toujours TRUE (documenté).
    let ok = unsafe { ProcessPrng(buf.as_mut_ptr(), buf.len()) };
    assert!(ok != 0, "générateur aléatoire du système indisponible");
}

#[cfg(not(windows))]
fn fill(buf: &mut [u8]) {
    use std::io::Read;
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(buf))
        .expect("générateur aléatoire du système indisponible");
}

/// `n` octets aléatoires en hexadécimal (2·n caractères).
pub fn random_hex(n: usize) -> String {
    crate::util::encoding::hex_encode(&random_bytes(n))
}

#[cfg(test)]
mod tests {
    #[test]
    fn differs_each_time() {
        let a = super::random_hex(16);
        let b = super::random_hex(16);
        assert_eq!(a.len(), 32);
        assert_ne!(a, b);
    }
}
