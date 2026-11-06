//! Serveur des overlays (port 3000 par défaut) : pages musicales, page des
//! alertes, adresse locale des outils TikTok, fichiers importés, pochettes.
//!
//!   GET /overlay/:id          page d'overlay (OBS, Streamlabs, TikTok LIVE Studio…)
//!   WS  /ws/:id               état de lecture, réglages, infos du live
//!   GET /api/overlays/:id     configuration d'un overlay
//!   GET /api/artwork/:id      pochettes « data: » sorties du flux WebSocket
//!   GET /alerts               page des alertes   — WS /ws-alerts
//!   GET|POST /api/alerts/hook/:clé   adresse locale (TikFinity, Streamer.bot…)
//!   GET /api/alert-media/:id  images et sons importés
//!
//! N'écoute que sur 127.0.0.1 ; `Host` (et `Origin` des WebSocket) doivent
//! désigner ce serveur : bloque le DNS-rebinding et les pages qui viseraient localhost.

use crate::alerts::model::{ALERTS_HOOK_PREFIX, ALERTS_PAGE_PATH, ALERTS_WS_PATH, ALERT_MEDIA_PREFIX};
use crate::json::Value;
use crate::loopback::{is_allowed_host, is_allowed_origin};
use crate::net::http::{Request, Response};
use crate::net::server::{Handler, HttpServer, WsSession, WsSocket};
use crate::obj;
use crate::util::encoding::{base64_decode, hex_encode};
use crate::util::hash::sha1;
use crate::util::url::decode as url_decode;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Branchements fournis par l'application (alertes, fichiers importés).
pub trait OverlayHooks: Send + Sync {
    fn read_media(&self, id: &str) -> Option<(Vec<u8>, &'static str)>;
    fn handle_hook(&self, secret: &str, payload: &Value) -> (u16, String);
    fn alerts_config(&self) -> Value;
}

// -- Pochettes « data: » ------------------------------------------------------------

/// Sort les pochettes « data: » (Lecture système, souvent 100 à 500 Ko) du flux
/// WebSocket : stockées une fois, remplacées par une URL courte mise en cache.
pub struct ArtworkStore {
    by_id: VecDeque<(String, String, Vec<u8>)>,
    last: Option<(String, String)>,
    capacity: usize,
}

impl Default for ArtworkStore {
    fn default() -> Self {
        ArtworkStore { by_id: VecDeque::new(), last: None, capacity: 8 }
    }
}

impl ArtworkStore {
    pub fn get(&self, id: &str) -> Option<(&str, &[u8])> {
        self.by_id.iter().find(|(i, _, _)| i == id).map(|(_, m, d)| (m.as_str(), d.as_slice()))
    }

    /// État dont la pochette « data: » est remplacée par `/api/artwork/<id>`.
    pub fn externalize(&mut self, state: &Value) -> Value {
        let Some(artwork) = state.path(&["track", "artwork"]).as_str().filter(|a| a.starts_with("data:")) else {
            return state.clone();
        };
        let cached = self.last.as_ref().filter(|(src, id)| src == artwork && self.by_id.iter().any(|(i, _, _)| i == id)).map(|(_, id)| id.clone());
        let id = match cached.or_else(|| self.store(artwork)) {
            Some(id) => {
                self.last = Some((artwork.to_string(), id.clone()));
                id
            }
            None => {
                // Data URI illisible : mieux vaut pas de pochette qu'une image cassée.
                let mut out = state.clone();
                if let Some(Value::Object(track)) = out.as_object_mut().and_then(|o| o.get_mut("track")) {
                    track.remove("artwork");
                }
                return out;
            }
        };
        let mut out = state.clone();
        if let Some(track) = out.as_object_mut().and_then(|o| o.get_mut("track")) {
            track.set("artwork", format!("/api/artwork/{id}"));
        }
        out
    }

    fn store(&mut self, uri: &str) -> Option<String> {
        let rest = uri.strip_prefix("data:")?;
        let (meta, payload) = rest.split_once(',')?;
        let mut parts = meta.split(';');
        let mime = parts.next()?.trim().to_string();
        if mime.is_empty() || !mime.contains('/') || mime.contains(['\r', '\n']) {
            return None;
        }
        let is_b64 = meta.split(';').any(|p| p == "base64");
        let data = if is_b64 { base64_decode(payload)? } else { url_decode(payload, false).into_bytes() };
        if data.is_empty() {
            return None;
        }
        let id = hex_encode(&sha1(&data))[..16].to_string();
        if !self.by_id.iter().any(|(i, _, _)| *i == id) {
            self.by_id.push_back((id.clone(), mime, data));
            while self.by_id.len() > self.capacity {
                self.by_id.pop_front();
            }
        }
        Some(id)
    }
}

// -- Fichiers statiques -----------------------------------------------------------------

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("").to_ascii_lowercase().as_str() {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Fichier du build (`dist/`) : chemin relatif sûr uniquement (pas de `..`, pas de `\`).
pub fn read_static(dir: &Path, url_path: &str) -> Option<(Vec<u8>, &'static str)> {
    let decoded = url_decode(url_path, false);
    let rel = decoded.trim_start_matches('/');
    if rel.is_empty() || rel.contains('\\') || rel.contains('\0') || rel.split('/').any(|seg| seg.is_empty() || seg == ".." || seg == "." || seg.contains(':')) {
        return None;
    }
    let full: PathBuf = dir.join(rel);
    let meta = std::fs::metadata(&full).ok()?;
    if !meta.is_file() {
        return None;
    }
    // Lien symbolique vers l'extérieur : refusé.
    let canon_dir = dir.canonicalize().ok()?;
    let canon = full.canonicalize().ok()?;
    if !canon.starts_with(&canon_dir) {
        return None;
    }
    Some((std::fs::read(&canon).ok()?, content_type(rel)))
}

pub fn static_response(dir: &Path, url_path: &str) -> Option<Response> {
    let (data, ctype) = read_static(dir, url_path)?;
    let cache = if url_path.starts_with("/assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
    Some(Response::with_body(200, ctype, data).header("Cache-Control", cache))
}

pub fn not_found() -> Response {
    Response::text(404, "Introuvable")
}

pub fn forbidden_host() -> Response {
    Response::text(403, "Hôte refusé")
}

// -- Serveur ---------------------------------------------------------------------------

struct OState {
    last_state: Value,
    last_live: Option<Value>,
    configs: Vec<(String, Value, String)>,
    clients: HashMap<String, Vec<WsSocket>>,
    alert_clients: Vec<WsSocket>,
    artwork: ArtworkStore,
}

struct Inner {
    /// Port réellement écouté (le port 0 des tests devient le port attribué).
    port: std::sync::atomic::AtomicU16,
    static_dir: PathBuf,
    state: Mutex<OState>,
    hooks: Arc<dyn OverlayHooks>,
}

impl Inner {
    fn port(&self) -> u16 {
        self.port.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, OState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state_message(st: &mut OState) -> String {
        let payload = st.artwork.externalize(&st.last_state);
        obj! { "type" => "state", "payload" => payload, "sentAt" => crate::util::now_ms() }.to_json()
    }
}

pub struct OverlayServer {
    inner: Arc<Inner>,
    http: Mutex<Option<HttpServer>>,
}

impl OverlayServer {
    pub fn new(port: u16, static_dir: impl Into<PathBuf>, hooks: Arc<dyn OverlayHooks>) -> OverlayServer {
        let state = OState {
            last_state: obj! { "track" => Value::Null, "isPlaying" => false, "updatedAt" => crate::util::now_ms() },
            last_live: None,
            configs: Vec::new(),
            clients: HashMap::new(),
            alert_clients: Vec::new(),
            artwork: ArtworkStore::default(),
        };
        OverlayServer { inner: Arc::new(Inner { port: std::sync::atomic::AtomicU16::new(port), static_dir: static_dir.into(), state: Mutex::new(state), hooks }), http: Mutex::new(None) }
    }

    pub fn start(&self) -> std::io::Result<()> {
        let mut http = self.http.lock().unwrap_or_else(|e| e.into_inner());
        if http.is_some() {
            return Ok(());
        }
        let server = HttpServer::start(self.inner.port(), Arc::new(OverlayHandler(self.inner.clone())))?;
        self.inner.port.store(server.port(), std::sync::atomic::Ordering::SeqCst);
        *http = Some(server);
        Ok(())
    }

    pub fn stop(&self) {
        if let Some(mut s) = self.http.lock().unwrap_or_else(|e| e.into_inner()).take() {
            s.stop();
        }
        let mut st = self.inner.lock();
        st.clients.clear();
        st.alert_clients.clear();
    }

    pub fn is_running(&self) -> bool {
        self.http.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(|s| s.is_running())
    }

    pub fn port(&self) -> u16 {
        self.http.lock().unwrap_or_else(|e| e.into_inner()).as_ref().map(|s| s.port()).unwrap_or(self.inner.port())
    }

    pub fn overlay_url(&self, overlay_id: &str, host: &str) -> String {
        format!("http://{host}:{}/overlay/{}", self.port(), crate::util::url::encode_component(overlay_id))
    }

    /// Nouvel état de lecture (envoyé par le tableau de bord), diffusé à toutes les pages.
    pub fn broadcast_state(&self, state: Value) {
        let (message, sockets) = {
            let mut st = self.inner.lock();
            st.last_state = state;
            let message = Inner::state_message(&mut st);
            (message, st.clients.values().flatten().cloned().collect::<Vec<_>>())
        };
        for ws in sockets {
            ws.send_text(&message);
        }
    }

    /// Infos du live (derniers spectateurs, totaux) pour les overlays personnalisés.
    pub fn broadcast_live_stats(&self, stats: Value) {
        let message = obj! { "type" => "live", "payload" => stats.clone() }.to_json();
        let sockets: Vec<WsSocket> = {
            let mut st = self.inner.lock();
            st.last_live = Some(stats);
            st.clients.values().flatten().cloned().collect()
        };
        for ws in sockets {
            ws.send_text(&message);
        }
    }

    /// Sources réellement connectées, par overlay.
    pub fn client_counts(&self) -> Value {
        let st = self.inner.lock();
        let mut out = Value::object();
        for (id, sockets) in &st.clients {
            let open = sockets.iter().filter(|s| s.is_open()).count();
            if open > 0 {
                out.set(id.clone(), open);
            }
        }
        out
    }

    /// Met à jour les overlays disponibles ; pousse aux pages ouvertes les configurations
    /// modifiées ; révoque (1008) les pages d'un overlay supprimé.
    pub fn set_overlay_configs(&self, overlays: &Value) {
        let next: Vec<(String, Value, String)> = overlays
            .items()
            .iter()
            .filter_map(|o| {
                let id = o.get("id").as_str().filter(|id| !id.is_empty() && id.len() <= 100)?;
                Some((id.to_string(), o.clone(), o.to_json()))
            })
            .collect();
        let mut pushes: Vec<(WsSocket, String)> = Vec::new();
        let mut revoked: Vec<WsSocket> = Vec::new();
        {
            let mut st = self.inner.lock();
            for (id, config, json) in &next {
                let before = st.configs.iter().find(|(i, _, _)| i == id).map(|(_, _, j)| j.clone());
                if before.is_some_and(|b| b != *json) {
                    let message = obj! { "type" => "config", "payload" => config.clone() }.to_json();
                    for ws in st.clients.get(id).into_iter().flatten() {
                        pushes.push((ws.clone(), message.clone()));
                    }
                }
            }
            let ids: Vec<String> = next.iter().map(|(i, _, _)| i.clone()).collect();
            st.clients.retain(|id, sockets| {
                if ids.contains(id) {
                    true
                } else {
                    revoked.extend(sockets.drain(..));
                    false
                }
            });
            st.configs = next;
        }
        for (ws, m) in pushes {
            ws.send_text(&m);
        }
        for ws in revoked {
            ws.close(1008, "Lien OBS révoqué");
        }
    }

    // -- Alertes ----------------------------------------------------------------------

    fn alert_sockets(&self) -> Vec<WsSocket> {
        let mut st = self.inner.lock();
        st.alert_clients.retain(|s| s.is_open());
        st.alert_clients.clone()
    }

    /// Renvoie le nombre de pages d'alertes prévenues (0 = aucune n'affichera l'alerte).
    pub fn broadcast_alert(&self, event: &Value) -> usize {
        let message = obj! { "type" => "alert", "payload" => event.clone() }.to_json();
        self.alert_sockets().iter().filter(|ws| ws.send_text(&message)).count()
    }

    pub fn set_alerts_config(&self, config: &Value) {
        let message = obj! { "type" => "config", "payload" => config.clone() }.to_json();
        for ws in self.alert_sockets() {
            ws.send_text(&message);
        }
    }

    pub fn skip_alerts(&self) {
        let message = obj! { "type" => "skip" }.to_json();
        for ws in self.alert_sockets() {
            ws.send_text(&message);
        }
    }

    pub fn alert_client_count(&self) -> usize {
        self.alert_sockets().len()
    }
}

impl Drop for OverlayServer {
    fn drop(&mut self) {
        self.stop();
    }
}

struct OverlayHandler(Arc<Inner>);

fn segment(path: &str, prefix: &str) -> Option<String> {
    let rest = path.strip_prefix(prefix)?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(url_decode(rest, false))
}

impl Handler for OverlayHandler {
    fn handle(&self, req: &Request) -> Response {
        let inner = &self.0;
        if !is_allowed_host(req.header("Host"), inner.port()) {
            return forbidden_host();
        }
        let response = route(inner, req);
        response.header("X-Content-Type-Options", "nosniff")
    }

    fn websocket(&self, req: &Request) -> Result<Box<dyn WsSession>, Response> {
        let inner = &self.0;
        if !is_allowed_host(req.header("Host"), inner.port()) || !is_allowed_origin(req.header("Origin"), inner.port(), false) {
            return Err(Response::text(403, "Refusé"));
        }
        if req.path() == ALERTS_WS_PATH {
            return Ok(Box::new(AlertsSession { inner: inner.clone(), socket: None }));
        }
        match segment(req.path(), "/ws/") {
            Some(id) => Ok(Box::new(OverlaySession { inner: inner.clone(), overlay_id: id, socket: None })),
            None => Err(not_found()),
        }
    }
}

fn route(inner: &Inner, req: &Request) -> Response {
    let path = req.path();
    let method = req.method.as_str();
    if let Some(secret) = segment(path, ALERTS_HOOK_PREFIX) {
        // Adresse « à clé » : sans la clé (32 caractères aléatoires), rien n'est déclenché.
        let payload = match method {
            "GET" | "HEAD" => {
                let mut v = Value::object();
                for (k, val) in req.query() {
                    if !v.as_object().is_some_and(|m| m.contains_key(&k)) {
                        v.set(k, val);
                    }
                }
                v
            }
            "POST" => {
                if req.body.len() > 4096 {
                    return Response::json(413, &obj! { "ok" => false, "message" => "Requête trop volumineuse." }).header("Cache-Control", "no-store");
                }
                match crate::json::parse_bytes(&req.body) {
                    Ok(v) => v,
                    Err(_) => return Response::json(400, &obj! { "ok" => false, "message" => "JSON invalide." }).header("Cache-Control", "no-store"),
                }
            }
            _ => return Response::text(405, "Méthode refusée"),
        };
        let (status, message) = inner.hooks.handle_hook(&secret, &payload);
        return Response::json(status, &obj! { "ok" => status < 300, "message" => message }).header("Cache-Control", "no-store");
    }
    if method != "GET" && method != "HEAD" {
        return Response::text(405, "Méthode refusée");
    }
    if path == "/health" {
        return Response::json(200, &obj! { "status" => "ok" });
    }
    if path == ALERTS_PAGE_PATH {
        return static_response(&inner.static_dir, "/alerts.html").map(|r| r.header("Cache-Control", "no-store")).unwrap_or_else(not_found);
    }
    if segment(path, "/overlay/").is_some() {
        return static_response(&inner.static_dir, "/overlay.html").map(|r| r.header("Cache-Control", "no-store")).unwrap_or_else(not_found);
    }
    if let Some(id) = segment(path, "/api/overlays/") {
        let st = inner.lock();
        return match st.configs.iter().find(|(i, _, _)| *i == id) {
            Some((_, config, _)) => Response::json(200, config),
            None => Response::json(404, &obj! { "error" => "Overlay introuvable" }),
        };
    }
    if let Some(id) = segment(path, "/api/artwork/") {
        let st = inner.lock();
        return match st.artwork.get(&id) {
            Some((mime, data)) => Response::with_body(200, mime, data.to_vec()).header("Cache-Control", "public, max-age=31536000, immutable"),
            None => not_found(),
        };
    }
    if let Some(id) = segment(path, ALERT_MEDIA_PREFIX) {
        return match inner.hooks.read_media(&id) {
            Some((data, mime)) => Response::with_body(200, mime, data).header("Cache-Control", "public, max-age=31536000, immutable"),
            None => not_found(),
        };
    }
    // Le tableau de bord n'est jamais servi aux logiciels de streaming.
    if path == "/" || path == "/index.html" {
        return not_found();
    }
    static_response(&inner.static_dir, path).unwrap_or_else(not_found)
}

struct OverlaySession {
    inner: Arc<Inner>,
    overlay_id: String,
    socket: Option<WsSocket>,
}

impl WsSession for OverlaySession {
    fn on_open(&mut self, socket: WsSocket) {
        let (known, first, live) = {
            let mut st = self.inner.lock();
            let known = st.configs.iter().any(|(i, _, _)| *i == self.overlay_id);
            if known {
                st.clients.entry(self.overlay_id.clone()).or_default().push(socket.clone());
            }
            (known, known.then(|| Inner::state_message(&mut st)), st.last_live.clone())
        };
        // Sans ce refus, une source dont le lien a été régénéré se reconnecterait sans fin.
        if !known {
            socket.close(1008, "Overlay inconnu ou lien OBS révoqué");
            return;
        }
        if let Some(m) = first {
            socket.send_text(&m);
        }
        if let Some(stats) = live {
            socket.send_text(&obj! { "type" => "live", "payload" => stats }.to_json());
        }
        self.socket = Some(socket);
    }
    fn on_close(&mut self) {
        if let Some(socket) = &self.socket {
            let mut st = self.inner.lock();
            if let Some(list) = st.clients.get_mut(&self.overlay_id) {
                list.retain(|s| s.id() != socket.id());
            }
        }
    }
}

struct AlertsSession {
    inner: Arc<Inner>,
    socket: Option<WsSocket>,
}

impl WsSession for AlertsSession {
    fn on_open(&mut self, socket: WsSocket) {
        self.inner.lock().alert_clients.push(socket.clone());
        let config = self.inner.hooks.alerts_config();
        socket.send_text(&obj! { "type" => "config", "payload" => config }.to_json());
        self.socket = Some(socket);
    }
    fn on_close(&mut self) {
        if let Some(socket) = &self.socket {
            self.inner.lock().alert_clients.retain(|s| s.id() != socket.id());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;
    use crate::net::client::{HttpClient, HttpRequest, PlainConnector, WsClient, WsEvent};
    use std::time::Duration;

    struct Hooks;
    impl OverlayHooks for Hooks {
        fn read_media(&self, id: &str) -> Option<(Vec<u8>, &'static str)> {
            (id == "0123456789abcdef01234567.png").then(|| (vec![1, 2, 3], "image/png"))
        }
        fn handle_hook(&self, secret: &str, payload: &Value) -> (u16, String) {
            if secret == "cle" {
                (200, format!("ok {}", payload.get("type").as_str().unwrap_or("?")))
            } else {
                (404, "Adresse inconnue.".into())
            }
        }
        fn alerts_config(&self) -> Value {
            obj! { "version" => 1 }
        }
    }

    fn dist() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("batlay-dist-{}", crate::util::random::random_hex(4)));
        std::fs::create_dir_all(dir.join("assets")).unwrap();
        std::fs::write(dir.join("overlay.html"), "<html>overlay</html>").unwrap();
        std::fs::write(dir.join("alerts.html"), "<html>alertes</html>").unwrap();
        std::fs::write(dir.join("index.html"), "<html>tableau de bord</html>").unwrap();
        std::fs::write(dir.join("assets/overlay-abc.js"), "console.log(1)").unwrap();
        dir
    }

    fn get(client: &HttpClient, port: u16, path: &str, host: &str) -> (u16, String, Option<String>) {
        let r = client.send(&HttpRequest::get(format!("http://127.0.0.1:{port}{path}")).header("Host", host)).unwrap();
        (r.status, r.text(), r.headers.get("Content-Type").map(str::to_string))
    }

    #[test]
    fn serves_pages_with_host_checks() {
        let dir = dist();
        let server = OverlayServer::new(0, &dir, Arc::new(Hooks));
        server.start().unwrap();
        let port = server.port();
        let host = format!("localhost:{port}");
        let client = HttpClient::new(Arc::new(PlainConnector), "test");
        assert_eq!(get(&client, port, "/overlay/abc", &host).1, "<html>overlay</html>");
        assert_eq!(get(&client, port, "/overlay/abc", &format!("evil.example:{port}")).0, 403);
        assert_eq!(get(&client, port, "/overlay/abc", &format!("batlay.test:{port}")).0, 200);
        assert_eq!(get(&client, port, "/alerts", &host).1, "<html>alertes</html>");
        let (status, body, ctype) = get(&client, port, "/assets/overlay-abc.js", &host);
        assert_eq!((status, body.as_str(), ctype.as_deref()), (200, "console.log(1)", Some("text/javascript; charset=utf-8")));
        assert_eq!(get(&client, port, "/", &host).0, 404, "jamais le tableau de bord");
        assert_eq!(get(&client, port, "/index.html", &host).0, 404);
        for evil in ["/assets/../index.html", "/%2e%2e/etc/passwd", "/assets/..%2f..%2fx", "/assets\\x"] {
            assert_eq!(get(&client, port, evil, &host).0, 404, "{evil}");
        }
        assert_eq!(get(&client, port, "/api/alert-media/0123456789abcdef01234567.png", &host).0, 200);
        assert_eq!(get(&client, port, "/api/overlays/abc", &host).0, 404);
        server.set_overlay_configs(&parse(r#"[{"id":"abc","name":"Glass"}]"#).unwrap());
        assert_eq!(get(&client, port, "/api/overlays/abc", &host).1, r#"{"id":"abc","name":"Glass"}"#);
        // Adresse locale : GET et POST.
        assert_eq!(get(&client, port, "/api/alerts/hook/cle?type=follow&user=Lina", &host).1, r#"{"ok":true,"message":"ok follow"}"#);
        let r = client.send(&HttpRequest::post(format!("http://127.0.0.1:{port}/api/alerts/hook/cle"), "text/plain", br#"{"type":"gift"}"#.to_vec()).header("Host", host.clone())).unwrap();
        assert_eq!(r.text(), r#"{"ok":true,"message":"ok gift"}"#);
        let r = client.send(&HttpRequest::post(format!("http://127.0.0.1:{port}/api/alerts/hook/cle"), "application/json", b"{x".to_vec()).header("Host", host.clone())).unwrap();
        assert_eq!(r.status, 400);
        assert_eq!(get(&client, port, "/api/alerts/hook/mauvaise?type=follow", &host).0, 404);
        server.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn ws(port: u16, path: &str) -> WsClient {
        // PlainConnector envoie `Host: 127.0.0.1:<port>`, autorisé.
        WsClient::connect(&PlainConnector, &format!("ws://127.0.0.1:{port}{path}"), Duration::from_secs(5), 1 << 20).unwrap()
    }

    fn next_json(c: &mut WsClient) -> Value {
        match c.recv().unwrap() {
            WsEvent::Text(t) => parse(&t).unwrap(),
            other => panic!("attendu un texte, reçu {other:?}"),
        }
    }

    #[test]
    fn websocket_state_config_and_revocation() {
        let dir = dist();
        let server = OverlayServer::new(0, &dir, Arc::new(Hooks));
        server.start().unwrap();
        let port = server.port();
        server.set_overlay_configs(&parse(r#"[{"id":"abc","v":1}]"#).unwrap());
        server.broadcast_live_stats(obj! { "followers" => 3 });
        let mut c = ws(port, "/ws/abc");
        let first = next_json(&mut c);
        assert_eq!((first.get("type").as_str(), first.get("sentAt").is_number()), (Some("state"), true));
        assert_eq!(next_json(&mut c).path(&["payload", "followers"]).as_u64(), Some(3));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(server.client_counts().get("abc").as_u64(), Some(1));
        // Pochette « data: » : sortie du flux, servie à part.
        server.broadcast_state(parse(r#"{"track":{"title":"Starboy","artwork":"data:image/png;base64,AQID"},"isPlaying":true,"updatedAt":1}"#).unwrap());
        let state = next_json(&mut c);
        let art = state.path(&["payload", "track", "artwork"]).as_str().unwrap().to_string();
        assert!(art.starts_with("/api/artwork/"));
        let client = HttpClient::new(Arc::new(PlainConnector), "test");
        let r = client.send(&HttpRequest::get(format!("http://127.0.0.1:{port}{art}"))).unwrap();
        assert_eq!((r.status, r.body.as_slice(), r.headers.get("Content-Type")), (200, &[1u8, 2, 3][..], Some("image/png")));
        // Sauvegarde dans l'éditeur : poussée en direct ; suppression : révocation.
        server.set_overlay_configs(&parse(r#"[{"id":"abc","v":2}]"#).unwrap());
        assert_eq!(next_json(&mut c).path(&["payload", "v"]).as_u64(), Some(2));
        server.set_overlay_configs(&parse("[]").unwrap());
        assert_eq!(c.recv().unwrap(), WsEvent::Closed(Some(1008)));
        let mut unknown = ws(port, "/ws/zzz");
        assert_eq!(unknown.recv().unwrap(), WsEvent::Closed(Some(1008)));
        server.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn alerts_page_receives_config_alerts_and_skip() {
        let dir = dist();
        let server = OverlayServer::new(0, &dir, Arc::new(Hooks));
        server.start().unwrap();
        assert_eq!(server.broadcast_alert(&obj! { "id" => "1" }), 0);
        let mut c = ws(server.port(), "/ws-alerts");
        assert_eq!(next_json(&mut c).get("type").as_str(), Some("config"));
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(server.broadcast_alert(&obj! { "id" => "2", "kind" => "follow" }), 1);
        assert_eq!(next_json(&mut c).path(&["payload", "kind"]).as_str(), Some("follow"));
        server.skip_alerts();
        assert_eq!(next_json(&mut c).get("type").as_str(), Some("skip"));
        drop(c);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(server.alert_client_count(), 0);
        server.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn artwork_store_capacity_and_bad_uris() {
        let mut store = ArtworkStore::default();
        let state = |uri: &str| obj! { "track" => obj! { "artwork" => uri }, "isPlaying" => true };
        let out = store.externalize(&state("data:image/png;base64,!!!"));
        assert!(out.path(&["track", "artwork"]).is_null());
        let out = store.externalize(&state("https://i.scdn.co/image/x"));
        assert_eq!(out.path(&["track", "artwork"]).as_str(), Some("https://i.scdn.co/image/x"));
        for i in 0..10 {
            store.externalize(&state(&format!("data:image/png;base64,{}", crate::util::encoding::base64_encode(&[i, 1, 2]))));
        }
        assert_eq!(store.by_id.len(), 8);
    }
}
