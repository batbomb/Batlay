//! Serveur HTTP/1.1 + WebSocket minimal, un fil d'exécution par connexion.
//!
//! N'écoute QUE sur 127.0.0.1 (jamais 0.0.0.0) : c'est le constructeur qui
//! choisit l'adresse, aucun appelant ne peut l'élargir. Les contrôles `Host` /
//! `Origin` sont faits par chaque gestionnaire (voir `crate::loopback`).

use super::http::{read_request, write_response, ReadError, Request, Response};
use super::ws::{self, Assembler, Message, Opcode};
use std::collections::HashMap;
use std::io::{self, BufReader, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Corps de requête accepté (le seul POST de Batlay porte un JSON de 4 Ko au plus).
pub const MAX_REQUEST_BODY: usize = 16 * 1024;
/// Connexions simultanées au plus (OBS ouvre une poignée de connexions par source).
const MAX_CONNECTIONS: usize = 128;
const KEEP_ALIVE_IDLE: Duration = Duration::from_secs(10);
const MAX_REQUESTS_PER_CONNECTION: usize = 200;
const WS_WRITE_TIMEOUT: Duration = Duration::from_secs(3);

/// Gestionnaire d'un serveur : requêtes HTTP et ouvertures de WebSocket.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, request: &Request) -> Response;
    /// Demande de WebSocket : `Err(réponse)` pour refuser (403…).
    fn websocket(&self, _request: &Request) -> Result<Box<dyn WsSession>, Response> {
        Err(Response::text(404, "Introuvable"))
    }
}

/// Une page connectée en WebSocket. Les pages de Batlay n'envoient rien
/// d'utile : la diffusion se fait par `WsSocket`, gardé par le gestionnaire.
pub trait WsSession: Send {
    fn on_open(&mut self, socket: WsSocket);
    fn on_message(&mut self, _message: Message) {}
    fn on_close(&mut self);
}

struct WsShared {
    id: u64,
    writer: Mutex<TcpStream>,
    open: AtomicBool,
}

/// Côté envoi d'une WebSocket ouverte (clonable, utilisable depuis n'importe quel fil).
#[derive(Clone)]
pub struct WsSocket(Arc<WsShared>);

impl WsSocket {
    pub fn id(&self) -> u64 {
        self.0.id
    }
    pub fn is_open(&self) -> bool {
        self.0.open.load(Ordering::SeqCst)
    }
    /// Envoie un texte ; faux si la page est partie (la socket est alors fermée).
    pub fn send_text(&self, text: &str) -> bool {
        self.send(Opcode::Text, text.as_bytes())
    }
    fn send(&self, opcode: Opcode, payload: &[u8]) -> bool {
        if !self.is_open() {
            return false;
        }
        let mut writer = self.0.writer.lock().unwrap_or_else(|e| e.into_inner());
        if ws::write_frame(&mut *writer, opcode, payload, None).is_err() {
            self.0.open.store(false, Ordering::SeqCst);
            let _ = writer.shutdown(Shutdown::Both);
            return false;
        }
        true
    }
    /// Ferme proprement avec un code (1008 = lien révoqué…).
    pub fn close(&self, code: u16, reason: &str) {
        if self.0.open.swap(false, Ordering::SeqCst) {
            let mut writer = self.0.writer.lock().unwrap_or_else(|e| e.into_inner());
            let _ = ws::write_frame(&mut *writer, Opcode::Close, &ws::close_payload(code, reason), None);
            let _ = writer.shutdown(Shutdown::Write);
        }
    }
}

struct Shared {
    stopping: AtomicBool,
    connections: Mutex<HashMap<u64, TcpStream>>,
    active: AtomicUsize,
    next_id: AtomicU64,
}

/// Serveur démarré ; `stop()` (ou la destruction) libère le port.
pub struct HttpServer {
    addr: SocketAddr,
    shared: Arc<Shared>,
    accept_thread: Option<thread::JoinHandle<()>>,
}

impl HttpServer {
    /// Démarre sur `127.0.0.1:port` (`port` 0 = port libre, pour les tests).
    pub fn start(port: u16, handler: Arc<dyn Handler>) -> io::Result<HttpServer> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        let addr = listener.local_addr()?;
        let shared = Arc::new(Shared {
            stopping: AtomicBool::new(false),
            connections: Mutex::new(HashMap::new()),
            active: AtomicUsize::new(0),
            next_id: AtomicU64::new(1),
        });
        let accept_shared = shared.clone();
        let accept_thread = thread::Builder::new().name(format!("http-{}", addr.port())).spawn(move || {
            for stream in listener.incoming() {
                if accept_shared.stopping.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if accept_shared.active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    let _ = stream.shutdown(Shutdown::Both);
                    continue;
                }
                let id = accept_shared.next_id.fetch_add(1, Ordering::SeqCst);
                if let Ok(clone) = stream.try_clone() {
                    accept_shared.connections.lock().unwrap_or_else(|e| e.into_inner()).insert(id, clone);
                }
                accept_shared.active.fetch_add(1, Ordering::SeqCst);
                let conn_shared = accept_shared.clone();
                let conn_handler = handler.clone();
                let spawned = thread::Builder::new().name("http-conn".into()).spawn(move || {
                    serve_connection(stream, id, conn_handler.as_ref(), &conn_shared);
                    conn_shared.connections.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                    conn_shared.active.fetch_sub(1, Ordering::SeqCst);
                });
                if spawned.is_err() {
                    accept_shared.connections.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                    accept_shared.active.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })?;
        Ok(HttpServer { addr, shared, accept_thread: Some(accept_thread) })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn is_running(&self) -> bool {
        !self.shared.stopping.load(Ordering::SeqCst)
    }

    /// Arrête d'accepter, ferme toutes les connexions et attend la libération du port.
    pub fn stop(&mut self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        // Débloque `accept()` en se connectant à soi-même.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(500));
        for (_, conn) in self.shared.connections.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            let _ = conn.shutdown(Shutdown::Both);
        }
        if let Some(t) = self.accept_thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for HttpServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve_connection(stream: TcpStream, id: u64, handler: &dyn Handler, shared: &Shared) {
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(KEEP_ALIVE_IDLE));
    let Ok(write_half) = stream.try_clone() else { return };
    let mut writer = write_half;
    let mut reader = BufReader::new(stream);
    let mut served = 0;
    loop {
        if shared.stopping.load(Ordering::SeqCst) {
            break;
        }
        let request = match read_request(&mut reader, MAX_REQUEST_BODY) {
            Ok(r) => r,
            Err(ReadError::Closed) | Err(ReadError::Io(_)) => break,
            Err(ReadError::TooLarge) => {
                let _ = write_response(&mut writer, &Response::text(413, "Requête trop volumineuse"), false, false);
                break;
            }
            Err(ReadError::Bad(_)) => {
                let _ = write_response(&mut writer, &Response::text(400, "Requête invalide"), false, false);
                break;
            }
        };
        served += 1;
        if request.is_websocket_upgrade() {
            run_websocket(request, id, reader, writer, handler);
            return;
        }
        let head_only = request.method == "HEAD";
        let response = match catch_unwind(AssertUnwindSafe(|| handler.handle(&request))) {
            Ok(r) => r,
            Err(_) => Response::text(500, "Erreur interne"),
        };
        let keep_alive = request.wants_keep_alive() && served < MAX_REQUESTS_PER_CONNECTION;
        if write_response(&mut writer, &response, head_only, keep_alive).is_err() || !keep_alive {
            break;
        }
    }
    let _ = writer.shutdown(Shutdown::Both);
}

fn run_websocket(request: Request, id: u64, mut reader: BufReader<TcpStream>, mut writer: TcpStream, handler: &dyn Handler) {
    let key = request.header("Sec-WebSocket-Key").map(str::to_string);
    let version_ok = request.header("Sec-WebSocket-Version").is_some_and(|v| v.trim() == "13");
    let Some(key) = key.filter(|_| version_ok) else {
        let _ = write_response(&mut writer, &Response::text(400, "WebSocket invalide"), false, false);
        let _ = writer.shutdown(Shutdown::Both);
        return;
    };
    let mut session = match catch_unwind(AssertUnwindSafe(|| handler.websocket(&request))) {
        Ok(Ok(session)) => session,
        Ok(Err(refusal)) => {
            let _ = write_response(&mut writer, &refusal, false, false);
            let _ = writer.shutdown(Shutdown::Both);
            return;
        }
        Err(_) => {
            let _ = writer.shutdown(Shutdown::Both);
            return;
        }
    };
    let accept = Response::new(101)
        .header("Upgrade", "websocket")
        .header("Connection", "Upgrade")
        .header("Sec-WebSocket-Accept", ws::accept_key(&key));
    if write_response(&mut writer, &accept, false, false).is_err() {
        return;
    }
    let _ = writer.set_write_timeout(Some(WS_WRITE_TIMEOUT));
    // Les pages restent connectées des heures : pas de délai de lecture.
    let _ = reader.get_ref().set_read_timeout(None);
    let socket = WsSocket(Arc::new(WsShared { id, writer: Mutex::new(writer), open: AtomicBool::new(true) }));
    session.on_open(socket.clone());
    let mut assembler = Assembler::default();
    // Les pages de Batlay n'envoient rien : 1 Ko suffit largement (comme `maxPayload: 1024`).
    while let Ok(frame) = ws::read_frame(&mut reader, true, 1024) {
        match assembler.push(frame, 1024) {
            Ok(Some(Message::Ping(data))) => {
                let mut w = socket.0.writer.lock().unwrap_or_else(|e| e.into_inner());
                if ws::write_frame(&mut *w, Opcode::Pong, &data, None).is_err() {
                    break;
                }
            }
            Ok(Some(Message::Close(..))) => {
                socket.close(1000, "");
                break;
            }
            Ok(Some(message)) => session.on_message(message),
            Ok(None) => {}
            Err(_) => {
                socket.close(1002, "protocole");
                break;
            }
        }
        if !socket.is_open() {
            break;
        }
    }
    socket.0.open.store(false, Ordering::SeqCst);
    let _ = socket.0.writer.lock().unwrap_or_else(|e| e.into_inner()).shutdown(Shutdown::Both);
    session.on_close();
}

/// Écrit `bytes` puis ferme : réponse brute (refus d'un upgrade, par exemple).
pub fn write_raw(stream: &mut TcpStream, bytes: &[u8]) {
    let _ = stream.write_all(bytes);
    let _ = stream.shutdown(Shutdown::Both);
}
