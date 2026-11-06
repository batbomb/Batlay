//! Contrôles « boucle locale » communs aux serveurs de Batlay.
//!
//! Les serveurs n'écoutent que sur 127.0.0.1, mais cela ne suffit pas : une
//! page web quelconque peut viser http://127.0.0.1 ou passer par un
//! DNS-rebinding (`evil.example` résolu en 127.0.0.1). Vérifier l'en-tête
//! `Host` (et l'`Origin` des WebSocket) ferme ces deux portes, sans gêner OBS,
//! Streamlabs, XSplit ni TikTok LIVE Studio (même origine que la page).

/// Nom local optionnel (fichier hosts) pour les logiciels qui refusent `localhost`.
pub const LOCAL_ALIAS_HOSTNAME: &str = "batlay.test";
pub const LOOPBACK_HOSTNAMES: [&str; 3] = ["localhost", "127.0.0.1", LOCAL_ALIAS_HOSTNAME];

/// Vrai si l'en-tête `Host` désigne exactement ce serveur local.
pub fn is_allowed_host(host: Option<&str>, port: u16) -> bool {
    let Some(host) = host else { return false };
    let host = host.trim().to_ascii_lowercase();
    LOOPBACK_HOSTNAMES.iter().any(|name| host == format!("{name}:{port}"))
}

/// Vrai si l'`Origin` est une page de ce même serveur. Absent : accepté seulement
/// si `required` est faux ; `"null"` est toujours refusé.
pub fn is_allowed_origin(origin: Option<&str>, port: u16, required: bool) -> bool {
    let Some(origin) = origin else { return !required };
    let origin = origin.trim().to_ascii_lowercase();
    LOOPBACK_HOSTNAMES.iter().any(|name| origin == format!("http://{name}:{port}"))
}

/// Adresse http locale de Batlay (jamais https, jamais un autre hôte, port obligatoire).
pub fn is_loopback_http_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://") else { return false };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.contains('@') {
        return false;
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => LOOPBACK_HOSTNAMES.contains(&host.to_ascii_lowercase().as_str()) && port.parse::<u16>().is_ok_and(|p| p > 0),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_and_origin_rules() {
        assert!(is_allowed_host(Some("localhost:3000"), 3000));
        assert!(is_allowed_host(Some(" Batlay.Test:3000 "), 3000));
        assert!(!is_allowed_host(Some("localhost:3001"), 3000));
        assert!(!is_allowed_host(Some("evil.example:3000"), 3000));
        assert!(!is_allowed_host(None, 3000));
        assert!(is_allowed_origin(None, 3000, false));
        assert!(!is_allowed_origin(None, 3000, true));
        assert!(!is_allowed_origin(Some("null"), 3000, false));
        assert!(is_allowed_origin(Some("http://127.0.0.1:3000"), 3000, true));
        assert!(!is_allowed_origin(Some("https://localhost:3000"), 3000, true));
    }

    #[test]
    fn loopback_urls() {
        assert!(is_loopback_http_url("http://batlay.test:3000/overlay/x"));
        assert!(!is_loopback_http_url("https://localhost:3000/"));
        assert!(!is_loopback_http_url("http://localhost/"));
        assert!(!is_loopback_http_url("http://user@localhost:3000/"));
        assert!(!is_loopback_http_url("http://evil.example:3000/"));
    }
}
