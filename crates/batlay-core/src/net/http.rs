//! HTTP/1.1 : lecture d'une requête (serveur) ou d'une réponse (client), écriture.
//! Limites strictes : en-têtes ≤ 16 Ko, 100 en-têtes, corps plafonné par l'appelant.

use std::io::{self, BufRead, Read, Write};

pub const MAX_HEADER_BYTES: usize = 16 * 1024;
const MAX_HEADERS: usize = 100;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Headers(Vec<(String, String)>);

impl Headers {
    pub fn new() -> Self {
        Headers(Vec::new())
    }
    /// Valeur d'un en-tête (nom insensible à la casse).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn get_all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.0.iter().filter(move |(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        self.0.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        self.0.push((name.to_string(), value.into()));
    }
    pub fn add(&mut self, name: &str, value: impl Into<String>) {
        self.0.push((name.to_string(), value.into()));
    }
    pub fn has_token(&self, name: &str, token: &str) -> bool {
        self.get_all(name).any(|v| v.split(',').any(|t| t.trim().eq_ignore_ascii_case(token)))
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// Cible brute (`/overlay/abc?x=1`).
    pub target: String,
    pub version_11: bool,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Request {
    pub fn path(&self) -> &str {
        self.target.split(['?', '#']).next().unwrap_or("/")
    }
    pub fn query(&self) -> Vec<(String, String)> {
        let q = self.target.split('#').next().unwrap_or("");
        match q.split_once('?') {
            Some((_, q)) => crate::util::url::parse_query(q),
            None => Vec::new(),
        }
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }
    pub fn wants_keep_alive(&self) -> bool {
        if self.headers.has_token("Connection", "close") {
            return false;
        }
        self.version_11 || self.headers.has_token("Connection", "keep-alive")
    }
    pub fn is_websocket_upgrade(&self) -> bool {
        self.method == "GET" && self.headers.has_token("Connection", "upgrade") && self.headers.get("Upgrade").is_some_and(|u| u.eq_ignore_ascii_case("websocket"))
    }
}

#[derive(Debug)]
pub enum ReadError {
    /// Connexion fermée proprement avant toute requête.
    Closed,
    Io(io::Error),
    Bad(&'static str),
    TooLarge,
}

impl From<io::Error> for ReadError {
    fn from(e: io::Error) -> Self {
        ReadError::Io(e)
    }
}

/// Lit une ligne terminée par CRLF (ou LF), en comptant les octets lus.
fn read_line(input: &mut impl BufRead, budget: &mut usize) -> Result<Option<String>, ReadError> {
    let mut line = Vec::new();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() { Ok(None) } else { Err(ReadError::Bad("ligne incomplète")) };
        }
        let (take, done) = match available.iter().position(|&b| b == b'\n') {
            Some(i) => (i + 1, true),
            None => (available.len(), false),
        };
        if take > *budget {
            return Err(ReadError::TooLarge);
        }
        *budget -= take;
        line.extend_from_slice(&available[..take]);
        input.consume(take);
        if done {
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            return String::from_utf8(line).map(Some).map_err(|_| ReadError::Bad("en-tête non UTF-8"));
        }
    }
}

fn read_headers(input: &mut impl BufRead, budget: &mut usize) -> Result<Headers, ReadError> {
    let mut headers = Headers::new();
    loop {
        let line = read_line(input, budget)?.ok_or(ReadError::Bad("en-têtes incomplets"))?;
        if line.is_empty() {
            return Ok(headers);
        }
        if headers.0.len() >= MAX_HEADERS {
            return Err(ReadError::TooLarge);
        }
        let (name, value) = line.split_once(':').ok_or(ReadError::Bad("en-tête invalide"))?;
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(ReadError::Bad("nom d'en-tête invalide"));
        }
        headers.add(name, value.trim());
    }
}

/// Lit une requête. `max_body` : au-delà, `TooLarge` (la connexion doit être fermée).
pub fn read_request(input: &mut impl BufRead, max_body: usize) -> Result<Request, ReadError> {
    let mut budget = MAX_HEADER_BYTES;
    // Lignes vides tolérées avant la requête (RFC 9112 §2.2).
    let line = loop {
        match read_line(input, &mut budget)? {
            None => return Err(ReadError::Closed),
            Some(l) if l.is_empty() => continue,
            Some(l) => break l,
        }
    };
    let mut parts = line.split(' ');
    let (method, target, version) = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(m), Some(t), Some(v), None) => (m, t, v),
        _ => return Err(ReadError::Bad("ligne de requête invalide")),
    };
    if !method.chars().all(|c| c.is_ascii_uppercase()) || method.is_empty() {
        return Err(ReadError::Bad("méthode invalide"));
    }
    if !target.starts_with('/') {
        return Err(ReadError::Bad("cible invalide"));
    }
    let version_11 = match version {
        "HTTP/1.1" => true,
        "HTTP/1.0" => false,
        _ => return Err(ReadError::Bad("version HTTP non prise en charge")),
    };
    let headers = read_headers(input, &mut budget)?;
    if headers.get("Transfer-Encoding").is_some() {
        // Aucun client légitime de Batlay n'envoie de corps « chunked » : refus (et pas de désynchronisation).
        return Err(ReadError::Bad("Transfer-Encoding non pris en charge"));
    }
    let length = match headers.get("Content-Length") {
        Some(v) => v.trim().parse::<usize>().map_err(|_| ReadError::Bad("Content-Length invalide"))?,
        None => 0,
    };
    if length > max_body {
        return Err(ReadError::TooLarge);
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    Ok(Request { method: method.to_string(), target: target.to_string(), version_11, headers, body })
}

#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
}

impl Response {
    pub fn new(status: u16) -> Response {
        Response { status, headers: Headers::new(), body: Vec::new() }
    }
    pub fn with_body(status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> Response {
        let mut r = Response::new(status);
        r.headers.set("Content-Type", content_type);
        r.body = body.into();
        r
    }
    pub fn text(status: u16, text: &str) -> Response {
        Response::with_body(status, "text/plain; charset=utf-8", text.as_bytes().to_vec())
    }
    pub fn html(status: u16, html: &str) -> Response {
        Response::with_body(status, "text/html; charset=utf-8", html.as_bytes().to_vec())
    }
    pub fn json(status: u16, value: &crate::json::Value) -> Response {
        Response::with_body(status, "application/json; charset=utf-8", value.to_json().into_bytes())
    }
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Response {
        self.headers.set(name, value);
        self
    }
    pub fn header_value(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub fn status_text(status: u16) -> &'static str {
    match status {
        101 => "Switching Protocols",
        200 => "OK",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

/// Écrit la réponse (corps omis pour `HEAD`). Ajoute `Content-Length` et `Connection`.
pub fn write_response(out: &mut impl Write, response: &Response, head_only: bool, keep_alive: bool) -> io::Result<()> {
    let mut text = format!("HTTP/1.1 {} {}\r\n", response.status, status_text(response.status));
    for (k, v) in response.headers.iter() {
        // Content-Length et Connection sont calculés ici (sauf pour 101, où « Connection: Upgrade » est obligatoire).
        if k.eq_ignore_ascii_case("Content-Length") || (k.eq_ignore_ascii_case("Connection") && response.status != 101) {
            continue;
        }
        // Une valeur ne peut jamais contenir de retour à la ligne (injection d'en-tête).
        let v: String = v.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        text.push_str(&format!("{k}: {v}\r\n"));
    }
    if response.status != 101 && response.status != 204 && response.status != 304 {
        text.push_str(&format!("Content-Length: {}\r\n", response.body.len()));
    }
    if response.status != 101 {
        text.push_str(if keep_alive { "Connection: keep-alive\r\n" } else { "Connection: close\r\n" });
    }
    text.push_str("\r\n");
    out.write_all(text.as_bytes())?;
    if !head_only {
        out.write_all(&response.body)?;
    }
    out.flush()
}

// -- Côté client ---------------------------------------------------------------

/// Lit l'en-tête d'une réponse : statut + en-têtes.
pub fn read_response_head(input: &mut impl BufRead) -> Result<(u16, Headers), ReadError> {
    let mut budget = MAX_HEADER_BYTES * 4;
    let line = read_line(input, &mut budget)?.ok_or(ReadError::Closed)?;
    let mut parts = line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(ReadError::Bad("réponse HTTP invalide"));
    }
    let status = parts.next().and_then(|s| s.parse::<u16>().ok()).ok_or(ReadError::Bad("statut invalide"))?;
    let headers = read_headers(input, &mut budget)?;
    Ok((status, headers))
}

/// Lit le corps d'une réponse (Content-Length, chunked, ou jusqu'à la fermeture).
pub fn read_response_body(input: &mut impl BufRead, headers: &Headers, status: u16, head_request: bool, max_body: usize) -> Result<Vec<u8>, ReadError> {
    if head_request || status == 204 || status == 304 || (100..200).contains(&status) {
        return Ok(Vec::new());
    }
    if headers.has_token("Transfer-Encoding", "chunked") {
        let mut body = Vec::new();
        loop {
            let mut budget = 1024;
            let line = read_line(input, &mut budget)?.ok_or(ReadError::Bad("bloc tronqué"))?;
            let size_text = line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_text, 16).map_err(|_| ReadError::Bad("taille de bloc invalide"))?;
            if size == 0 {
                // En-têtes de fin éventuels, puis ligne vide.
                let mut budget = MAX_HEADER_BYTES;
                loop {
                    match read_line(input, &mut budget)? {
                        None => break,
                        Some(l) if l.is_empty() => break,
                        Some(_) => continue,
                    }
                }
                return Ok(body);
            }
            if body.len() + size > max_body {
                return Err(ReadError::TooLarge);
            }
            let start = body.len();
            body.resize(start + size, 0);
            input.read_exact(&mut body[start..])?;
            let mut crlf = [0u8; 2];
            input.read_exact(&mut crlf)?;
        }
    }
    if let Some(len) = headers.get("Content-Length") {
        let len: usize = len.trim().parse().map_err(|_| ReadError::Bad("Content-Length invalide"))?;
        if len > max_body {
            return Err(ReadError::TooLarge);
        }
        let mut body = vec![0u8; len];
        input.read_exact(&mut body)?;
        return Ok(body);
    }
    let mut body = Vec::new();
    input.take(max_body as u64 + 1).read_to_end(&mut body)?;
    if body.len() > max_body {
        return Err(ReadError::TooLarge);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    #[test]
    fn parses_requests_with_limits() {
        let raw = b"\r\nPOST /api/alerts/hook/abc?x=1 HTTP/1.1\r\nHost: localhost:3000\r\nContent-Length: 5\r\nConnection: keep-alive, Upgrade\r\n\r\nhelloEXTRA";
        let mut input = BufReader::new(&raw[..]);
        let r = read_request(&mut input, 100).unwrap();
        assert_eq!((r.method.as_str(), r.path(), r.body.as_slice()), ("POST", "/api/alerts/hook/abc", &b"hello"[..]));
        assert_eq!(r.query(), vec![("x".to_string(), "1".to_string())]);
        assert_eq!(r.header("host"), Some("localhost:3000"));
        assert!(r.wants_keep_alive());
        assert!(matches!(read_request(&mut BufReader::new(&b"POST / HTTP/1.1\r\nContent-Length: 50\r\n\r\n"[..]), 10), Err(ReadError::TooLarge)));
        assert!(matches!(read_request(&mut BufReader::new(&b""[..]), 10), Err(ReadError::Closed)));
        for bad in [&b"GET http://x/ HTTP/1.1\r\n\r\n"[..], b"GET / HTTP/2\r\n\r\n", b"GET / HTTP/1.1\r\nBad Header\r\n\r\n", b"GET / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n"] {
            assert!(read_request(&mut BufReader::new(bad), 10).is_err());
        }
        let huge = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(20_000));
        assert!(matches!(read_request(&mut BufReader::new(huge.as_bytes()), 10), Err(ReadError::TooLarge)));
    }

    #[test]
    fn writes_responses_safely() {
        let mut out = Vec::new();
        let r = Response::text(403, "Hôte refusé").header("X-Evil", "a\r\nSet-Cookie: x");
        write_response(&mut out, &r, false, false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.starts_with("HTTP/1.1 403 Forbidden\r\n"));
        assert!(text.contains("X-Evil: aSet-Cookie: x\r\n"));
        assert!(text.contains(&format!("Content-Length: {}\r\n", "Hôte refusé".len())));
        assert!(text.ends_with("\r\n\r\nHôte refusé"));
        let mut out = Vec::new();
        write_response(&mut out, &Response::new(101).header("Upgrade", "websocket").header("Connection", "Upgrade"), false, false).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Connection: Upgrade\r\n") && !text.contains("Content-Length"), "{text}");
    }

    #[test]
    fn reads_chunked_and_plain_bodies() {
        let raw = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6;x=1\r\n world\r\n0\r\nX: y\r\n\r\n";
        let mut input = BufReader::new(&raw[..]);
        let (status, headers) = read_response_head(&mut input).unwrap();
        assert_eq!(status, 200);
        assert_eq!(read_response_body(&mut input, &headers, status, false, 100).unwrap(), b"hello world");
        let raw = b"HTTP/1.0 404 Not Found\r\n\r\nrien";
        let mut input = BufReader::new(&raw[..]);
        let (status, headers) = read_response_head(&mut input).unwrap();
        assert_eq!(read_response_body(&mut input, &headers, status, false, 100).unwrap(), b"rien");
        let mut input = BufReader::new(&raw[..]);
        let (status, headers) = read_response_head(&mut input).unwrap();
        assert!(read_response_body(&mut input, &headers, status, false, 2).is_err());
    }
}
