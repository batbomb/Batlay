//! Réseau sans dépendance : HTTP/1.1 et WebSocket, serveur (127.0.0.1) et client.

pub mod client;
pub mod http;
pub mod server;
pub mod ws;

pub use client::{Connector, HttpClient, HttpRequest, HttpResponse, NetError, PlainConnector, Stream, WsClient, WsEvent};
pub use http::{Request, Response};
pub use server::{Handler, HttpServer, WsSession, WsSocket};

use std::sync::Arc;
use std::time::Duration;

/// Envoi d'une requête HTTP (le vrai client, ou une réponse simulée en test).
pub trait Http: Send + Sync {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, NetError>;
}

impl Http for HttpClient {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, NetError> {
        HttpClient::send(self, request)
    }
}

/// Connexion WebSocket sortante ouverte.
pub trait WsConn: Send {
    /// Message suivant ; `Idle` au bout d'environ une seconde sans rien.
    fn recv(&mut self) -> Result<WsEvent, NetError>;
    fn close(&mut self);
    /// Message binaire (flux TikTok). Non pris en charge par défaut.
    fn send_binary(&mut self, _data: &[u8]) -> Result<(), NetError> {
        Err(NetError::Protocol("envoi binaire non pris en charge".into()))
    }
}

impl WsConn for WsClient {
    fn recv(&mut self) -> Result<WsEvent, NetError> {
        WsClient::recv(self)
    }
    fn close(&mut self) {
        WsClient::close(self)
    }
    fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
        WsClient::send_binary(self, data)
    }
}

/// Ouvre des WebSocket sortantes.
pub trait WsConnect: Send + Sync {
    fn connect(&self, url: &str) -> Result<Box<dyn WsConn>, NetError>;
    /// Avec des en-têtes en plus et des messages jusqu'à `max_message` octets.
    fn connect_with(&self, url: &str, _headers: &[(String, String)], _max_message: usize) -> Result<Box<dyn WsConn>, NetError> {
        self.connect(url)
    }
}

/// Connexions réelles : délai d'ouverture 10 s, puis `Idle` toutes les secondes.
pub struct WsConnector(pub Arc<dyn Connector>);

impl WsConnect for WsConnector {
    fn connect(&self, url: &str) -> Result<Box<dyn WsConn>, NetError> {
        self.connect_with(url, &[], 1024 * 1024)
    }
    fn connect_with(&self, url: &str, headers: &[(String, String)], max_message: usize) -> Result<Box<dyn WsConn>, NetError> {
        let mut c = WsClient::connect_with_headers(self.0.as_ref(), url, headers, Duration::from_secs(10), max_message)?;
        c.set_read_timeout(Some(Duration::from_secs(1)));
        Ok(Box::new(c))
    }
}

/// Réponse HTTP simulée (tests).
pub fn fake_response(status: u16, body: &str) -> HttpResponse {
    HttpResponse { status, headers: http::Headers::new(), body: body.as_bytes().to_vec(), url: String::new() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::inflate::gzip_store;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Default)]
    struct Hub {
        sockets: Mutex<Vec<WsSocket>>,
        received: Mutex<Vec<String>>,
    }

    struct TestHandler(Arc<Hub>);

    struct Session(Arc<Hub>);
    impl WsSession for Session {
        fn on_open(&mut self, socket: WsSocket) {
            socket.send_text("bienvenue");
            self.0.sockets.lock().unwrap().push(socket);
        }
        fn on_message(&mut self, message: ws::Message) {
            if let ws::Message::Text(t) = message {
                self.0.received.lock().unwrap().push(t);
            }
        }
        fn on_close(&mut self) {}
    }

    impl Handler for TestHandler {
        fn handle(&self, request: &Request) -> Response {
            match request.path() {
                "/hello" => Response::text(200, &format!("Bonjour {}", request.query().first().map(|q| q.1.clone()).unwrap_or_default())),
                "/echo" => Response::with_body(200, "application/octet-stream", request.body.clone()),
                "/gzip" => Response::with_body(200, "text/plain", gzip_store(b"compresse")).header("Content-Encoding", "gzip"),
                "/redirect" => Response::new(302).header("Location", "/hello?n=redir"),
                "/panic" => panic!("gestionnaire en panne"),
                _ => Response::text(404, "Introuvable"),
            }
        }
        fn websocket(&self, request: &Request) -> Result<Box<dyn WsSession>, Response> {
            if request.path() == "/ws" {
                Ok(Box::new(Session(self.0.clone())))
            } else {
                Err(Response::text(403, "refusé"))
            }
        }
    }

    fn start() -> (HttpServer, Arc<Hub>, HttpClient) {
        let hub = Arc::new(Hub::default());
        let server = HttpServer::start(0, Arc::new(TestHandler(hub.clone()))).unwrap();
        (server, hub, HttpClient::new(Arc::new(PlainConnector), "Batlay-test"))
    }

    #[test]
    fn http_round_trips() {
        let (server, _hub, client) = start();
        let base = format!("http://127.0.0.1:{}", server.port());
        let r = client.send(&HttpRequest::get(format!("{base}/hello?n=L%C3%A9a"))).unwrap();
        assert_eq!((r.status, r.text()), (200, "Bonjour Léa".to_string()));
        let r = client.send(&HttpRequest::post(format!("{base}/echo"), "text/plain", b"abc".to_vec())).unwrap();
        assert_eq!(r.body, b"abc");
        assert_eq!(client.send(&HttpRequest::get(format!("{base}/gzip"))).unwrap().text(), "compresse");
        let r = client.send(&HttpRequest::get(format!("{base}/redirect"))).unwrap();
        assert_eq!((r.text(), r.url.ends_with("/hello?n=redir")), ("Bonjour redir".to_string(), true));
        assert_eq!(client.send(&HttpRequest::get(format!("{base}/panic"))).unwrap().status, 500);
        assert_eq!(client.send(&HttpRequest::head(format!("{base}/hello"))).unwrap().body.len(), 0);
        assert!(client.send(&HttpRequest::get("https://example.com/")).is_err(), "pas de TLS en clair");
    }

    #[test]
    fn keep_alive_serves_several_requests() {
        use std::io::{BufReader, Write};
        let (server, _hub, _client) = start();
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", server.port())).unwrap();
        stream.write_all(b"GET /hello?n=1 HTTP/1.1\r\nHost: x\r\n\r\nGET /hello?n=2 HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n").unwrap();
        let mut reader = BufReader::new(stream);
        for expected in ["Bonjour 1", "Bonjour 2"] {
            let (status, headers) = http::read_response_head(&mut reader).unwrap();
            let body = http::read_response_body(&mut reader, &headers, status, false, 100).unwrap();
            assert_eq!(String::from_utf8(body).unwrap(), expected);
        }
    }

    #[test]
    fn websocket_both_ways_and_server_close() {
        let (server, hub, _client) = start();
        let url = format!("ws://127.0.0.1:{}/ws", server.port());
        let mut ws = WsClient::connect(&PlainConnector, &url, Duration::from_secs(5), 1 << 20).unwrap();
        assert_eq!(ws.recv().unwrap(), WsEvent::Text("bienvenue".into()));
        ws.send_text("ping depuis la page").unwrap();
        // Diffusion depuis un autre fil, puis fermeture « lien révoqué ».
        std::thread::sleep(Duration::from_millis(100));
        let socket = hub.sockets.lock().unwrap()[0].clone();
        assert!(socket.send_text("{\"type\":\"state\"}"));
        socket.close(1008, "Lien OBS révoqué");
        assert_eq!(ws.recv().unwrap(), WsEvent::Text("{\"type\":\"state\"}".into()));
        assert_eq!(ws.recv().unwrap(), WsEvent::Closed(Some(1008)));
        assert_eq!(hub.received.lock().unwrap().as_slice(), ["ping depuis la page"]);
        assert!(!socket.send_text("après fermeture"));
        assert!(matches!(WsClient::connect(&PlainConnector, &format!("ws://127.0.0.1:{}/autre", server.port()), Duration::from_secs(5), 1024), Err(NetError::Protocol(_))));
    }

    #[test]
    fn websocket_client_headers_and_binary_frames_on_a_real_socket() {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut head = Vec::new();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                head.push(line.trim_end().to_string());
            }
            let key = head.iter().find_map(|l| l.strip_prefix("Sec-WebSocket-Key: ")).unwrap().to_string();
            let mut out = stream;
            out.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", ws::accept_key(&key)).as_bytes()).unwrap();
            let frame = ws::read_frame(&mut reader, true, 1024).unwrap();
            ws::write_frame(&mut out, ws::Opcode::Binary, &[9, 8, 7], None).unwrap();
            (head, frame)
        });
        let headers = vec![("User-Agent".to_string(), "Mozilla/5.0 Test".to_string()), ("Cookie".to_string(), "ttwid=abc".to_string())];
        let mut ws = WsConnector(Arc::new(PlainConnector)).connect_with(&format!("ws://127.0.0.1:{port}/webcast?x=1"), &headers, 1024).unwrap();
        ws.send_binary(&[1, 2, 3, 0xff]).unwrap();
        assert_eq!(ws.recv().unwrap(), WsEvent::Binary(vec![9, 8, 7]));
        let (head, frame) = server.join().unwrap();
        assert_eq!(head[0], "GET /webcast?x=1 HTTP/1.1");
        assert!(head.contains(&"User-Agent: Mozilla/5.0 Test".to_string()) && head.contains(&"Cookie: ttwid=abc".to_string()));
        assert_eq!(head.iter().filter(|l| l.starts_with("User-Agent:")).count(), 1, "pas de second User-Agent");
        assert_eq!((frame.opcode, frame.payload), (ws::Opcode::Binary, vec![1, 2, 3, 0xff]));
        // En-têtes refusés : injection et en-têtes de la poignée de main.
        for bad in [("Cookie", "a\r\nX: y"), ("Host", "evil.example"), ("Sec-WebSocket-Key", "x"), ("Mauvais nom", "x")] {
            let h = vec![(bad.0.to_string(), bad.1.to_string())];
            assert!(matches!(WsClient::connect_with_headers(&PlainConnector, &format!("ws://127.0.0.1:{port}/"), &h, Duration::from_secs(1), 1024), Err(NetError::Protocol(_))), "{bad:?}");
        }
    }

    #[test]
    fn websocket_idle_then_message() {
        let (server, hub, _client) = start();
        let mut ws = WsClient::connect(&PlainConnector, &format!("ws://127.0.0.1:{}/ws", server.port()), Duration::from_secs(5), 1024).unwrap();
        ws.set_read_timeout(Some(Duration::from_millis(100)));
        assert_eq!(ws.recv().unwrap(), WsEvent::Text("bienvenue".into()));
        assert_eq!(ws.recv().unwrap(), WsEvent::Idle);
        hub.sockets.lock().unwrap()[0].send_text("après une pause");
        assert_eq!(ws.recv().unwrap(), WsEvent::Text("après une pause".into()));
    }

    #[test]
    fn stop_frees_the_port_and_closes_sockets() {
        let (mut server, _hub, client) = start();
        let port = server.port();
        let mut ws = WsClient::connect(&PlainConnector, &format!("ws://127.0.0.1:{port}/ws"), Duration::from_secs(5), 1024).unwrap();
        assert_eq!(ws.recv().unwrap(), WsEvent::Text("bienvenue".into()));
        server.stop();
        assert!(!server.is_running());
        assert!(matches!(ws.recv(), Ok(WsEvent::Closed(_)) | Err(_)));
        assert!(client.send(&HttpRequest::get(format!("http://127.0.0.1:{port}/hello"))).is_err());
        // Le port est de nouveau libre.
        let again = HttpServer::start(port, Arc::new(TestHandler(Arc::new(Hub::default()))));
        assert!(again.is_ok());
    }
}
