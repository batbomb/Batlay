//! Logiciels de streaming de la page « Intégrations » : seule information côté
//! cœur, l'adresse de leur documentation. Le tableau de bord ne transmet jamais
//! d'URL à ouvrir, seulement un identifiant ; l'adresse vient de cette liste fermée.

/// Documentation officielle d'un logiciel, ou `None` (identifiant inconnu, pas de doc).
pub fn docs_url(platform_id: &str) -> Option<&'static str> {
    match platform_id {
        "obs" => Some("https://obsproject.com/kb/browser-source"),
        // TikTok ne publie pas de documentation de la source « Link » : page officielle de téléchargement.
        "tiktok-live-studio" => Some("https://www.tiktok.com/studio/download"),
        "streamlabs" => Some("https://streamlabs.com/content-hub/post/introducing-browser-source-interaction-for-streamlabs-desktop"),
        "xsplit" => Some("https://support.xsplit.com/en/article/webpage-1y1l6v"),
        _ => None,
    }
}

/// Adresses que Batlay a le droit d'ouvrir dans le navigateur (liste fermée de préfixes https).
pub fn is_openable(url: &str) -> bool {
    const PREFIXES: [&str; 7] = [
        "https://id.twitch.tv/oauth2/authorize?",
        "https://accounts.spotify.com/authorize?",
        "https://github.com/BatStream-off/Batlay/releases",
        "https://obsproject.com/",
        "https://www.tiktok.com/studio/",
        "https://streamlabs.com/",
        "https://support.xsplit.com/",
    ];
    PREFIXES.iter().any(|p| url.starts_with(p)) && !url.contains(['\r', '\n', ' ', '"'])
}

#[cfg(test)]
mod tests {
    #[test]
    fn closed_list() {
        assert_eq!(super::docs_url("obs"), Some("https://obsproject.com/kb/browser-source"));
        assert_eq!(super::docs_url("other"), None);
        assert_eq!(super::docs_url("javascript:alert(1)"), None);
        assert!(super::is_openable("https://id.twitch.tv/oauth2/authorize?x=1"));
        assert!(!super::is_openable("https://evil.example/"));
        assert!(!super::is_openable("file:///C:/Windows/system32/calc.exe"));
    }
}
