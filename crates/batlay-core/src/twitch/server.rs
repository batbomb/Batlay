//! Serveur Twitch de Batlay : port FIXE 4000, séparé du serveur des overlays.
//!
//!   GET  /                 page de retour OAuth (URI de redirection déclarée chez Twitch)
//!   POST /auth/callback    relais du jeton lu dans le fragment par cette page
//!   GET  /overlay/chat     page du chat pour OBS
//!   WS   /ws/chat          messages en temps réel vers cette page
//!
//! 127.0.0.1 uniquement ; `Host` / `Origin` étrangers refusés.

use super::model::{TWITCH_OVERLAY_PATH, TWITCH_WS_PATH};
use super::oauth::{callback_page, callback_page_csp, parse_callback, CallbackPayload};
use crate::json::Value;
use crate::loopback::{is_allowed_host, is_allowed_origin};
use crate::net::http::{Request, Response};
use crate::net::server::{Handler, HttpServer, WsSession, WsSocket};
use crate::obj;
use crate::overlay_server::{forbidden_host, not_found, static_response};
use crate::util::encoding::base64_encode;
use crate::util::random::random_bytes;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::{Arc, Mutex};

const BACKLOG_SIZE: usize = 50;

pub trait TwitchServerEvents: Send + Sync {
    /// Retour OAuth relayé par la page : (ok, message affiché dans le navigateur).
    fn auth_callback(&self, payload: CallbackPayload) -> (bool, String);
    fn clients_changed(&self, count: usize);
}

struct State {
    clients: Vec<WsSocket>,
    backlog: VecDeque<Value>,
    badges: Value,
}

struct Inner {
    port: AtomicU16,
    static_dir: PathBuf,
    state: Mutex<State>,
    events: Arc<dyn TwitchServerEvents>,
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn port(&self) -> u16 {
        self.port.load(Ordering::SeqCst)
    }
}

pub struct TwitchServer {
    inner: Arc<Inner>,
    http: Mutex<Option<HttpServer>>,
}

impl TwitchServer {
    /// `port` : toujours 4000 en production (0 dans les tests).
    pub fn new(port: u16, static_dir: impl Into<PathBuf>, events: Arc<dyn TwitchServerEvents>) -> TwitchServer {
        TwitchServer {
            inner: Arc::new(Inner {
                port: AtomicU16::new(port),
                static_dir: static_dir.into(),
                state: Mutex::new(State { clients: Vec::new(), backlog: VecDeque::new(), badges: Value::object() }),
                events,
            }),
            http: Mutex::new(None),
        }
    }

    pub fn start(&self) -> std::io::Result<()> {
        let mut http = self.http.lock().unwrap_or_else(|e| e.into_inner());
        if http.is_some() {
            return Ok(());
        }
        let server = HttpServer::start(self.inner.port(), Arc::new(TwitchHandler(self.inner.clone())))?;
        self.inner.port.store(server.port(), Ordering::SeqCst);
        *http = Some(server);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut s) = self.http.lock().unwrap_or_else(|e| e.into_inner()).take() {
            s.stop();
        }
        self.inner.lock().clients.clear();
    }

    pub fn is_running(&self) -> bool {
        self.http.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.is_running())
    }

    pub fn port(&self) -> u16 {
        self.inner.port()
    }

    /// Diffuse à toutes les pages ; sauf `remember = false` (test), met à jour l'historique.
    pub fn broadcast(&self, message: &Value, remember: bool) {
        if message.get("type").as_str() == Some("init") {
            return;
        }
        let sockets = {
            let mut st = self.inner.lock();
            if remember {
                match message.get("type").as_str() {
                    Some("chat") => {
                        st.backlog.push_back(message.get("message").clone());
                        while st.backlog.len() > BACKLOG_SIZE {
                            st.backlog.pop_front();
                        }
                    }
                    Some("delete") => {
                        let id = message.get("messageId").as_str().map(str::to_string);
                        st.backlog.retain(|m| m.get("id").as_str() != id.as_deref());
                    }
                    Some("clear_user") => {
                        let id = message.get("userId").as_str().map(str::to_string);
                        st.backlog.retain(|m| m.get("userId").as_str() != id.as_deref());
                    }
                    Some("clear") => st.backlog.clear(),
                    Some("badges") => st.badges = message.get("badges").clone(),
                    _ => {}
                }
            }
            st.clients.retain(|s| s.is_open());
            st.clients.clone()
        };
        let text = message.to_json();
        for ws in sockets {
            ws.send_text(&text);
        }
    }

    /// Déconnexion du compte : écran et historique vidés, badges oubliés.
    pub fn reset(&self) {
        self.broadcast(&obj! { "type" => "clear" }, true);
        self.broadcast(&obj! { "type" => "badges", "badges" => Value::object() }, true);
    }

    /// Fait recharger les sources OBS connectées ; renvoie leur nombre.
    pub fn reload_clients(&self) -> usize {
        let count = self.client_count();
        self.broadcast(&obj! { "type" => "reload" }, false);
        count
    }

    pub fn client_count(&self) -> usize {
        let mut st = self.inner.lock();
        st.clients.retain(|s| s.is_open());
        st.clients.len()
    }
}

impl Drop for TwitchServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct TwitchHandler(Arc<Inner>);

impl Handler for TwitchHandler {
    fn handle(&self, req: &Request) -> Response {
        let inner = &self.0;
        if !is_allowed_host(req.header("Host"), inner.port()) {
            return forbidden_host();
        }
        route(inner, req).header("X-Content-Type-Options", "nosniff")
    }

    fn websocket(&self, req: &Request) -> Result<Box<dyn WsSession>, Response> {
        let inner = &self.0;
        if req.path() != TWITCH_WS_PATH || !is_allowed_host(req.header("Host"), inner.port()) || !is_allowed_origin(req.header("Origin"), inner.port(), false) {
            return Err(Response::text(403, "Refusé"));
        }
        Ok(Box::new(ChatSession { inner: inner.clone(), socket: None }))
    }
}

fn route(inner: &Inner, req: &Request) -> Response {
    let path = req.path();
    match (req.method.as_str(), path) {
        ("GET" | "HEAD", "/") => {
            let nonce = base64_encode(&random_bytes(16));
            Response::html(200, &callback_page(&nonce))
                .header("Content-Security-Policy", callback_page_csp(&nonce))
                .header("Cache-Control", "no-store")
                .header("Referrer-Policy", "no-referrer")
        }
        ("POST", "/auth/callback") => {
            let reply = |status: u16, ok: bool, message: &str| Response::json(status, &obj! { "ok" => ok, "message" => message }).header("Cache-Control", "no-store");
            if !is_allowed_origin(req.header("Origin"), inner.port(), true) {
                return reply(403, false, "Origine refusée.");
            }
            let is_json = req.header("Content-Type").is_some_and(|c| c.split(';').next().unwrap_or("").trim().eq_ignore_ascii_case("application/json"));
            if !is_json || req.body.len() > 4096 {
                return reply(400, false, "Requête invalide.");
            }
            let Some(payload) = crate::json::parse_bytes(&req.body).ok().and_then(|b| parse_callback(&b)) else {
                return reply(400, false, "Réponse de Twitch illisible. Relancez la connexion depuis Batlay.");
            };
            let (ok, message) = inner.events.auth_callback(payload);
            reply(200, ok, &message)
        }
        ("GET" | "HEAD", TWITCH_OVERLAY_PATH) => static_response(&inner.static_dir, "/twitch-overlay.html").map(|r| r.header("Cache-Control", "no-store")).unwrap_or_else(not_found),
        ("GET" | "HEAD", "/health") => Response::json(200, &obj! { "status" => "ok" }),
        // Seulement les fichiers buildés : ni tableau de bord, ni overlay musical.
        ("GET" | "HEAD", p) if p.starts_with("/assets/") => static_response(&inner.static_dir, p).unwrap_or_else(not_found),
        _ => not_found(),
    }
}

struct ChatSession {
    inner: Arc<Inner>,
    socket: Option<WsSocket>,
}

impl WsSession for ChatSession {
    fn on_open(&mut self, socket: WsSocket) {
        let (init, count) = {
            let mut st = self.inner.lock();
            st.clients.push(socket.clone());
            let init = obj! { "type" => "init", "badges" => st.badges.clone(), "backlog" => Value::Array(st.backlog.iter().cloned().collect()) };
            (init, st.clients.len())
        };
        socket.send_text(&init.to_json());
        self.socket = Some(socket);
        self.inner.events.clients_changed(count);
    }
    fn on_close(&mut self) {
        let count = {
            let mut st = self.inner.lock();
            if let Some(s) = &self.socket {
                st.clients.retain(|c| c.id() != s.id());
            }
            st.clients.retain(|c| c.is_open());
            st.clients.len()
        };
        self.inner.events.clients_changed(count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;
    use crate::net::client::{HttpClient, HttpRequest, PlainConnector, WsClient, WsEvent};
    use std::time::Duration;

    #[derive(Default)]
    struct Events {
        callbacks: Mutex<Vec<CallbackPayload>>,
        counts: Mutex<Vec<usize>>,
    }
    impl TwitchServerEvents for Events {
        fn auth_callback(&self, p: CallbackPayload) -> (bool, String) {
            self.callbacks.lock().unwrap().push(p);
            (true, "Compte connecté.".into())
        }
        fn clients_changed(&self, n: usize) {
            self.counts.lock().unwrap().push(n);
        }
    }

    fn start() -> (TwitchServer, Arc<Events>, PathBuf) {
        let dir = std::env::temp_dir().join(format!("batlay-tw-{}", crate::util::random::random_hex(4)));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("twitch-overlay.html"), "<html>chat</html>").unwrap();
        std::fs::write(dir.join("overlay.html"), "<html>musique</html>").unwrap();
        let events = Arc::new(Events::default());
        let server = TwitchServer::new(0, &dir, events.clone());
        server.start().unwrap();
        (server, events, dir)
    }

    #[test]
    fn callback_page_and_relay() {
        let (server, events, dir) = start();
        let port = server.port();
        let client = HttpClient::new(Arc::new(PlainConnector), "t");
        let page = client.send(&HttpRequest::get(format!("http://127.0.0.1:{port}/"))).unwrap();
        assert!(page.text().contains("Serveur Twitch de Batlay"));
        assert!(page.headers.get("Content-Security-Policy").unwrap().contains("script-src 'nonce-"));
        let body = br#"{"access_token":"abcdefghijklmnop0123","scope":"user:read:chat","token_type":"bearer","state":"abcdefgh12"}"#;
        let post = |origin: Option<&str>| {
            let mut r = HttpRequest::post(format!("http://127.0.0.1:{port}/auth/callback"), "application/json", body.to_vec());
            if let Some(o) = origin {
                r = r.header("Origin", o);
            }
            client.send(&r).unwrap()
        };
        assert_eq!(post(None).status, 403, "Origin obligatoire");
        assert_eq!(post(Some("https://evil.example")).status, 403);
        let ok = post(Some(&format!("http://localhost:{port}")));
        assert_eq!(ok.text(), r#"{"ok":true,"message":"Compte connecté."}"#);
        assert_eq!(events.callbacks.lock().unwrap().len(), 1);
        assert_eq!(client.send(&HttpRequest::get(format!("http://127.0.0.1:{port}/overlay/chat"))).unwrap().text(), "<html>chat</html>");
        assert_eq!(client.send(&HttpRequest::get(format!("http://127.0.0.1:{port}/overlay.html"))).unwrap().status, 404, "rien d'autre que le chat");
        server.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn chat_backlog_and_moderation() {
        let (server, events, dir) = start();
        let port = server.port();
        server.broadcast(&parse(r#"{"type":"badges","badges":{"vip/1":"https://static-cdn.jtvnw.net/x"}}"#).unwrap(), true);
        for id in ["a", "b", "c"] {
            server.broadcast(&parse(&format!(r#"{{"type":"chat","message":{{"id":"{id}","userId":"u-{id}","text":"{id}"}}}}"#)).unwrap(), true);
        }
        server.broadcast(&parse(r#"{"type":"delete","messageId":"b"}"#).unwrap(), true);
        server.broadcast(&parse(r#"{"type":"chat","message":{"id":"t","userId":"batlay-test"}}"#).unwrap(), false);
        let mut ws = WsClient::connect(&PlainConnector, &format!("ws://127.0.0.1:{port}/ws/chat"), Duration::from_secs(5), 1 << 20).unwrap();
        let WsEvent::Text(init) = ws.recv().unwrap() else { panic!() };
        let init = parse(&init).unwrap();
        assert_eq!(init.get("backlog").items().iter().map(|m| m.get("id").as_str().unwrap()).collect::<Vec<_>>(), vec!["a", "c"]);
        assert_eq!(init.path(&["badges", "vip/1"]).as_str(), Some("https://static-cdn.jtvnw.net/x"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(server.reload_clients(), 1);
        let WsEvent::Text(reload) = ws.recv().unwrap() else { panic!() };
        assert_eq!(reload, r#"{"type":"reload"}"#);
        assert_eq!(events.counts.lock().unwrap().as_slice(), [1]);
        assert!(WsClient::connect(&PlainConnector, &format!("ws://127.0.0.1:{port}/ws/autre"), Duration::from_secs(5), 1024).is_err());
        server.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
