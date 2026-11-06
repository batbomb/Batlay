//! Client HTTP/1.1 et WebSocket au-dessus d'un `Connector`.
//!
//! Le chiffrement TLS n'est PAS ici : l'application fournit un `Connector`
//! qui ouvre une connexion TLS (SChannel de Windows, voir src-tauri). Les
//! tests et l'outil `batlay-serve` utilisent `PlainConnector` (TCP en clair,
//! refuse https/wss).

use super::http::{read_response_body, read_response_head, Headers, ReadError};
use super::ws::{self, Assembler, Message, Opcode};
use crate::json::Value;
use crate::util::encoding::base64_encode;
use crate::util::random::random_bytes;
use crate::util::url::Url;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

/// Flux d'une connexion sortante (TCP ou TLS).
pub trait Stream: Read + Write + Send {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()>;
    fn shutdown(&mut self);
}

impl Stream for TcpStream {
    fn set_read_timeout(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, timeout)
    }
    fn shutdown(&mut self) {
        let _ = TcpStream::shutdown(self, Shutdown::Both);
    }
}

/// Ouvre des connexions sortantes.
pub trait Connector: Send + Sync {
    fn connect(&self, host: &str, port: u16, tls: bool, timeout: Duration) -> io::Result<Box<dyn Stream>>;
}

/// Ouvre une connexion TCP (première adresse qui répond).
pub fn tcp_connect(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, format!("adresse introuvable : {host}"));
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}

/// TCP en clair uniquement (tests, outils locaux). Refuse https/wss.
pub struct PlainConnector;

impl Connector for PlainConnector {
    fn connect(&self, host: &str, port: u16, tls: bool, timeout: Duration) -> io::Result<Box<dyn Stream>> {
        if tls {
            return Err(io::Error::new(io::ErrorKind::Unsupported, "TLS indisponible dans ce connecteur"));
        }
        Ok(Box::new(tcp_connect(host, port, timeout)?))
    }
}

// -- HTTP ------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout: Duration,
    pub max_body: usize,
    pub follow_redirects: u8,
}

impl HttpRequest {
    pub fn get(url: impl Into<String>) -> HttpRequest {
        HttpRequest { method: "GET", url: url.into(), headers: Vec::new(), body: Vec::new(), timeout: Duration::from_secs(10), max_body: 8 * 1024 * 1024, follow_redirects: 5 }
    }
    pub fn post(url: impl Into<String>, content_type: &str, body: impl Into<Vec<u8>>) -> HttpRequest {
        let mut r = HttpRequest::get(url);
        r.method = "POST";
        r.body = body.into();
        r.follow_redirects = 0;
        r.headers.push(("Content-Type".into(), content_type.into()));
        r
    }
    pub fn post_json(url: impl Into<String>, body: &Value) -> HttpRequest {
        HttpRequest::post(url, "application/json", body.to_json().into_bytes())
    }
    pub fn post_form(url: impl Into<String>, pairs: &[(&str, &str)]) -> HttpRequest {
        HttpRequest::post(url, "application/x-www-form-urlencoded", crate::util::url::build_form(pairs).into_bytes())
    }
    pub fn head(url: impl Into<String>) -> HttpRequest {
        let mut r = HttpRequest::get(url);
        r.method = "HEAD";
        r
    }
    pub fn header(mut self, name: &str, value: impl Into<String>) -> HttpRequest {
        self.headers.retain(|(k, _)| !k.eq_ignore_ascii_case(name));
        self.headers.push((name.to_string(), value.into()));
        self
    }
    pub fn timeout(mut self, timeout: Duration) -> HttpRequest {
        self.timeout = timeout;
        self
    }
    pub fn max_body(mut self, max: usize) -> HttpRequest {
        self.max_body = max;
        self
    }
}

#[derive(Debug, Clone)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Headers,
    pub body: Vec<u8>,
    /// Adresse finale (après redirections).
    pub url: String,
}

impl HttpResponse {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
    pub fn json(&self) -> Option<Value> {
        crate::json::parse_bytes(&self.body).ok()
    }
}

#[derive(Debug)]
pub enum NetError {
    InvalidUrl,
    Connect(io::Error),
    Io(io::Error),
    Protocol(String),
    TooLarge,
    TooManyRedirects,
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::InvalidUrl => write!(f, "adresse invalide"),
            NetError::Connect(e) => write!(f, "connexion impossible ({e})"),
            NetError::Io(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => write!(f, "délai dépassé"),
            NetError::Io(e) => write!(f, "erreur réseau ({e})"),
            NetError::Protocol(m) => write!(f, "réponse invalide ({m})"),
            NetError::TooLarge => write!(f, "réponse trop volumineuse"),
            NetError::TooManyRedirects => write!(f, "trop de redirections"),
        }
    }
}
impl std::error::Error for NetError {}

impl From<ReadError> for NetError {
    fn from(e: ReadError) -> Self {
        match e {
            ReadError::Io(e) => NetError::Io(e),
            ReadError::TooLarge => NetError::TooLarge,
            ReadError::Closed => NetError::Protocol("connexion fermée".into()),
            ReadError::Bad(m) => NetError::Protocol(m.into()),
        }
    }
}

/// Client HTTP. Une connexion par requête (`Connection: close`) : simple et sans état.
#[derive(Clone)]
pub struct HttpClient {
    connector: Arc<dyn Connector>,
    user_agent: String,
}

impl HttpClient {
    pub fn new(connector: Arc<dyn Connector>, user_agent: impl Into<String>) -> HttpClient {
        HttpClient { connector, user_agent: user_agent.into() }
    }

    pub fn connector(&self) -> Arc<dyn Connector> {
        self.connector.clone()
    }

    pub fn send(&self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        let mut url = Url::parse(&request.url).ok_or(NetError::InvalidUrl)?;
        if url.scheme != "http" && url.scheme != "https" {
            return Err(NetError::InvalidUrl);
        }
        let mut method = request.method;
        let mut body = request.body.clone();
        for _ in 0..=request.follow_redirects {
            let (status, headers, data) = self.send_once(method, &url, &request.headers, &body, request)?;
            if matches!(status, 301 | 302 | 303 | 307 | 308) && request.follow_redirects > 0 {
                if let Some(next) = headers.get("Location").and_then(|l| url.join(l)) {
                    // Jamais de https → http.
                    if url.is_tls() && !next.is_tls() {
                        return Err(NetError::Protocol("redirection vers http refusée".into()));
                    }
                    if status == 303 || ((status == 301 || status == 302) && method == "POST") {
                        method = "GET";
                        body.clear();
                    }
                    url = next;
                    continue;
                }
            }
            return Ok(HttpResponse { status, headers, body: data, url: url.to_string_full() });
        }
        Err(NetError::TooManyRedirects)
    }

    fn send_once(&self, method: &str, url: &Url, headers: &[(String, String)], body: &[u8], request: &HttpRequest) -> Result<(u16, Headers, Vec<u8>), NetError> {
        let mut stream = self.connector.connect(&url.host, url.port, url.is_tls(), request.timeout).map_err(NetError::Connect)?;
        stream.set_read_timeout(Some(request.timeout)).map_err(NetError::Io)?;
        let mut head = format!("{method} {} HTTP/1.1\r\nConnection: close\r\nAccept-Encoding: gzip\r\n", url.path_and_query);
        if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("Host")) {
            head.push_str(&format!("Host: {}\r\n", url.host_header()));
        }
        let mut has_ua = false;
        for (k, v) in headers {
            if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
                return Err(NetError::Protocol("en-tête invalide".into()));
            }
            has_ua |= k.eq_ignore_ascii_case("User-Agent");
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        if !has_ua {
            head.push_str(&format!("User-Agent: {}\r\n", self.user_agent));
        }
        if !body.is_empty() || method == "POST" || method == "PUT" {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        head.push_str("\r\n");
        let mut data = head.into_bytes();
        data.extend_from_slice(body);
        stream.write_all(&data).map_err(NetError::Io)?;
        stream.flush().map_err(NetError::Io)?;
        let mut reader = BufReader::new(stream);
        let (status, headers) = read_response_head(&mut reader)?;
        let raw = read_response_body(&mut reader, &headers, status, method == "HEAD", request.max_body)?;
        reader.get_mut().shutdown();
        let body = match headers.get("Content-Encoding").map(|v| v.trim().to_ascii_lowercase()) {
            Some(enc) if enc == "gzip" || enc == "deflate" => {
                crate::util::inflate::decompress_auto(&raw, request.max_body).map_err(|e| NetError::Protocol(e.to_string()))?
            }
            _ => raw,
        };
        Ok((status, headers, body))
    }
}

// -- WebSocket ----------------------------------------------------------------------

/// Connexion WebSocket sortante (EventSub de Twitch).
pub struct WsClient {
    reader: BufReader<Box<dyn Stream>>,
    assembler: Assembler,
    max_message: usize,
    closed: bool,
    /// Délai d'attente d'un message (`Idle` ensuite) ; une trame commencée a, elle, 15 s pour arriver.
    idle_timeout: Option<Duration>,
}

const FRAME_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, PartialEq, Eq)]
pub enum WsEvent {
    Text(String),
    Binary(Vec<u8>),
    /// Rien reçu pendant le délai de lecture (permet de vérifier un arrêt demandé).
    Idle,
    Closed(Option<u16>),
}

impl WsClient {
    pub fn connect(connector: &dyn Connector, url: &str, timeout: Duration, max_message: usize) -> Result<WsClient, NetError> {
        WsClient::connect_with_headers(connector, url, &[], timeout, max_message)
    }

    /// Comme `connect`, avec des en-têtes en plus (User-Agent, Cookie…). Les en-têtes
    /// propres à la poignée de main WebSocket ne peuvent pas être remplacés.
    pub fn connect_with_headers(connector: &dyn Connector, url: &str, headers: &[(String, String)], timeout: Duration, max_message: usize) -> Result<WsClient, NetError> {
        let url = Url::parse(url).ok_or(NetError::InvalidUrl)?;
        if url.scheme != "ws" && url.scheme != "wss" {
            return Err(NetError::InvalidUrl);
        }
        const RESERVED: [&str; 6] = ["host", "upgrade", "connection", "sec-websocket-key", "sec-websocket-version", "sec-websocket-extensions"];
        let mut extra = String::new();
        let mut has_ua = false;
        for (k, v) in headers {
            if k.is_empty() || k.contains(['\r', '\n', ':', ' ']) || v.contains(['\r', '\n']) || RESERVED.contains(&k.to_ascii_lowercase().as_str()) {
                return Err(NetError::Protocol("en-tête invalide".into()));
            }
            has_ua |= k.eq_ignore_ascii_case("User-Agent");
            extra.push_str(&format!("{k}: {v}\r\n"));
        }
        if !has_ua {
            extra.push_str("User-Agent: Batlay\r\n");
        }
        let mut stream = connector.connect(&url.host, url.port, url.is_tls(), timeout).map_err(NetError::Connect)?;
        stream.set_read_timeout(Some(timeout)).map_err(NetError::Io)?;
        let key = base64_encode(&random_bytes(16));
        let head = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: {key}\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n",
            url.path_and_query,
            url.host_header()
        );
        stream.write_all(head.as_bytes()).map_err(NetError::Io)?;
        stream.flush().map_err(NetError::Io)?;
        let mut reader = BufReader::new(stream);
        let (status, headers) = read_response_head(&mut reader)?;
        if status != 101 {
            return Err(NetError::Protocol(format!("HTTP {status} au lieu de 101")));
        }
        if headers.get("Sec-WebSocket-Accept") != Some(ws::accept_key(&key).as_str()) {
            return Err(NetError::Protocol("Sec-WebSocket-Accept incorrect".into()));
        }
        Ok(WsClient { reader, assembler: Assembler::default(), max_message, closed: false, idle_timeout: Some(timeout) })
    }

    /// Délai au bout duquel `recv` rend `Idle` (`None` : attente sans limite).
    pub fn set_read_timeout(&mut self, timeout: Option<Duration>) {
        self.idle_timeout = timeout;
        let _ = self.reader.get_mut().set_read_timeout(timeout);
    }

    /// Message suivant. Répond seul aux pings ; `Idle` au bout du délai de lecture.
    pub fn recv(&mut self) -> Result<WsEvent, NetError> {
        if self.closed {
            return Ok(WsEvent::Closed(None));
        }
        loop {
            // Délai de lecture écoulé AVANT le début d'une trame : simple attente.
            match self.reader.fill_buf() {
                Ok([]) => {
                    self.closed = true;
                    return Ok(WsEvent::Closed(None));
                }
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::WouldBlock || e.kind() == io::ErrorKind::TimedOut => return Ok(WsEvent::Idle),
                Err(e) => return Err(NetError::Io(e)),
            }
            // Une trame a commencé : elle doit arriver en entier, même si l'attente « Idle » est courte.
            let _ = self.reader.get_mut().set_read_timeout(Some(FRAME_TIMEOUT));
            let frame = ws::read_frame(&mut self.reader, false, self.max_message);
            let _ = self.reader.get_mut().set_read_timeout(self.idle_timeout);
            let frame = match frame {
                Ok(f) => f,
                Err(ws::FrameError::TooLarge) => return Err(NetError::TooLarge),
                Err(ws::FrameError::Protocol(m)) => return Err(NetError::Protocol(m.into())),
                Err(ws::FrameError::Io(e)) => return Err(NetError::Io(e)),
            };
            match self.assembler.push(frame, self.max_message) {
                Ok(Some(Message::Text(t))) => return Ok(WsEvent::Text(t)),
                Ok(Some(Message::Binary(b))) => return Ok(WsEvent::Binary(b)),
                Ok(Some(Message::Ping(data))) => self.send_frame(Opcode::Pong, &data)?,
                Ok(Some(Message::Pong(_))) | Ok(None) => {}
                Ok(Some(Message::Close(code, _))) => {
                    let _ = self.send_frame(Opcode::Close, &code.map(|c| c.to_be_bytes().to_vec()).unwrap_or_default());
                    self.closed = true;
                    self.reader.get_mut().shutdown();
                    return Ok(WsEvent::Closed(code));
                }
                Err(_) => return Err(NetError::Protocol("trame invalide".into())),
            }
        }
    }

    pub fn send_text(&mut self, text: &str) -> Result<(), NetError> {
        self.send_frame(Opcode::Text, text.as_bytes())
    }

    pub fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        if self.closed {
            return Err(NetError::Protocol("connexion fermée".into()));
        }
        self.send_frame(Opcode::Binary, data)
    }

    fn send_frame(&mut self, opcode: Opcode, payload: &[u8]) -> Result<(), NetError> {
        let mask: [u8; 4] = random_bytes(4).try_into().unwrap_or([7, 7, 7, 7]);
        ws::write_frame(self.reader.get_mut(), opcode, payload, Some(mask)).map_err(NetError::Io)
    }

    pub fn close(&mut self) {
        if !self.closed {
            self.closed = true;
            let _ = self.send_frame(Opcode::Close, &1000u16.to_be_bytes());
            self.reader.get_mut().shutdown();
        }
    }
}

impl Drop for WsClient {
    fn drop(&mut self) {
        self.close();
    }
}
