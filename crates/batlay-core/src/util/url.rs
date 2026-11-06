//! Adresses : encodage « pourcent », chaînes de requête, découpage d'une URL.

/// Encode comme `encodeURIComponent` (lettres, chiffres et `-_.!~*'()` gardés).
pub fn encode_component(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Encode pour `application/x-www-form-urlencoded` (espace → `+`), comme `URLSearchParams`.
pub fn encode_form(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.*".contains(&b) {
            out.push(b as char);
        } else if b == b' ' {
            out.push('+');
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Décode `%XX` (et `+` en espace si `plus_is_space`). Séquences invalides gardées telles quelles ;
/// octets non UTF-8 remplacés par U+FFFD.
pub fn decode(text: &str, plus_is_space: bool) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b'+' if plus_is_space => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `a=1&b=x+y` → paires décodées (comme `URLSearchParams`).
pub fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .trim_start_matches('?')
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| match part.split_once('=') {
            Some((k, v)) => (decode(k, true), decode(v, true)),
            None => (decode(part, true), String::new()),
        })
        .collect()
}

pub fn query_get<'a>(pairs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

/// Construit `a=1&b=x%20y` (encodage `encodeURIComponent`).
pub fn build_query(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v))).collect::<Vec<_>>().join("&")
}

/// Corps `application/x-www-form-urlencoded`.
pub fn build_form(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{}={}", encode_form(k), encode_form(v))).collect::<Vec<_>>().join("&")
}

/// URL http(s)/ws(s) découpée. Pas de prise en charge des identifiants (`user:pass@`) : refusés.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    /// Chemin + requête, toujours commençant par `/`.
    pub path_and_query: String,
    pub fragment: Option<String>,
}

impl Url {
    pub fn parse(text: &str) -> Option<Url> {
        let text = text.trim();
        let (scheme, rest) = text.split_once("://")?;
        let scheme = scheme.to_ascii_lowercase();
        let default_port = match scheme.as_str() {
            "http" | "ws" => 80,
            "https" | "wss" => 443,
            _ => return None,
        };
        let (rest, fragment) = match rest.split_once('#') {
            Some((r, f)) => (r, Some(f.to_string())),
            None => (rest, None),
        };
        let authority_end = rest.find(['/', '?']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        if authority.is_empty() || authority.contains('@') || authority.contains(char::is_whitespace) {
            return None;
        }
        let mut path_and_query = rest[authority_end..].to_string();
        if path_and_query.is_empty() || path_and_query.starts_with('?') {
            path_and_query.insert(0, '/');
        }
        let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
            let (h, after) = stripped.split_once(']')?;
            let port = match after.strip_prefix(':') {
                Some(p) => p.parse().ok()?,
                None if after.is_empty() => default_port,
                None => return None,
            };
            (h.to_ascii_lowercase(), port)
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_ascii_lowercase(), p.parse().ok()?),
                None => (authority.to_ascii_lowercase(), default_port),
            }
        };
        if host.is_empty() || !host.chars().all(|c| c.is_ascii_alphanumeric() || "-._:".contains(c)) {
            return None;
        }
        Some(Url { scheme, host, port, path_and_query, fragment })
    }

    pub fn is_tls(&self) -> bool {
        self.scheme == "https" || self.scheme == "wss"
    }

    pub fn path(&self) -> &str {
        self.path_and_query.split('?').next().unwrap_or("/")
    }

    pub fn query(&self) -> Vec<(String, String)> {
        match self.path_and_query.split_once('?') {
            Some((_, q)) => parse_query(q),
            None => Vec::new(),
        }
    }

    /// `Host` à envoyer (port omis s'il est celui par défaut).
    pub fn host_header(&self) -> String {
        let default = if self.is_tls() { 443 } else { 80 };
        if self.port == default {
            self.host.clone()
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }

    /// Résout une adresse relative (`Location` d'une redirection).
    pub fn join(&self, location: &str) -> Option<Url> {
        if location.contains("://") {
            return Url::parse(location);
        }
        if let Some(rest) = location.strip_prefix("//") {
            return Url::parse(&format!("{}://{}", self.scheme, rest));
        }
        let path = if location.starts_with('/') {
            location.to_string()
        } else {
            let base = self.path();
            let dir = &base[..base.rfind('/').map(|i| i + 1).unwrap_or(1)];
            format!("{dir}{location}")
        };
        Some(Url { path_and_query: path, fragment: None, ..self.clone() })
    }

    pub fn to_string_full(&self) -> String {
        format!("{}://{}{}", self.scheme, self.host_header(), self.path_and_query)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_like_js() {
        assert_eq!(encode_component("a b&c=é/"), "a%20b%26c%3D%C3%A9%2F");
        assert_eq!(encode_form("a b*~"), "a+b*%7E");
        assert_eq!(decode("a%20b+c%C3%A9%zz%", true), "a b cé%zz%");
        assert_eq!(decode("a+b", false), "a+b");
    }

    #[test]
    fn query_round_trip() {
        let q = parse_query("?type=follow&user=L%C3%A9a+B&flag&=x");
        assert_eq!(query_get(&q, "type"), Some("follow"));
        assert_eq!(query_get(&q, "user"), Some("Léa B"));
        assert_eq!(query_get(&q, "flag"), Some(""));
        assert_eq!(build_query(&[("a", "1 2"), ("b", "é")]), "a=1%202&b=%C3%A9");
        assert_eq!(build_form(&[("grant_type", "refresh_token"), ("x", "a b")]), "grant_type=refresh_token&x=a+b");
    }

    #[test]
    fn url_parsing() {
        let u = Url::parse("https://WWW.TikTok.com/@adil.bli/live?lang=fr#x").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.path(), u.fragment.as_deref()), ("www.tiktok.com", 443, "/@adil.bli/live", Some("x")));
        assert_eq!(u.query(), vec![("lang".to_string(), "fr".to_string())]);
        let w = Url::parse("ws://localhost:3000").unwrap();
        assert_eq!((w.path_and_query.as_str(), w.host_header()), ("/", "localhost:3000".to_string()));
        assert!(Url::parse("ftp://x").is_none());
        assert!(Url::parse("https://user:pw@evil.example/").is_none());
        assert!(Url::parse("https://exa mple.com/").is_none());
        assert_eq!(Url::parse("http://[::1]:8080/a").unwrap().port, 8080);
        let base = Url::parse("https://coverartarchive.org/release-group/1/front").unwrap();
        assert_eq!(base.join("https://archive.org/x.jpg").unwrap().host, "archive.org");
        assert_eq!(base.join("/y").unwrap().to_string_full(), "https://coverartarchive.org/y");
        assert_eq!(base.join("z.jpg").unwrap().path(), "/release-group/1/z.jpg");
    }
}
