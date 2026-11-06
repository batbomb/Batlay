//! Méthode « Euler Stream » (au choix, jamais par défaut) : un service tiers,
//! Euler Stream, signe l'adresse du flux du live ; Batlay s'y connecte ensuite
//! lui-même (WebSocket vers TikTok) et décode les trames avec `webcast.rs`.
//!
//! Ce qui part chez Euler : l'identifiant du live (trouvé par Batlay sur
//! tiktok.com), le User-Agent, et la clé API si l'utilisateur en a mis une.
//! JAMAIS la session TikTok de l'utilisateur : Euler ne lit donc que les lives
//! visibles sans compte (pas un compte privé).
//!
//! Déroulé (le même que la bibliothèque communautaire tiktok-live-connector) :
//!   1. GET https://api.eulerstream.com/webcast/rooms/{room}/connect → première
//!      réponse du flux (`ProtoMessageFetchResult`, protobuf) + cookie TikTok
//!      (`x-set-tt-cookie`) ;
//!   2. WebSocket vers `wsUrl` (serveur TikTok) avec `cursor`, `internal_ext`
//!      et `wsParams` de cette réponse ;
//!   3. trame « im_enter_room », puis un « hb » toutes les 10 s ; chaque trame
//!      qui le demande reçoit son accusé (« ack » avec `internalExt`).

use super::browser::Outcome;
use super::proto::encode::{field_bytes, field_varint};
use super::proto::Msg;
use super::webcast::{decode_push, is_webcast_socket_url, LiveEvent};
use crate::net::{HttpRequest, HttpResponse, WsConn, WsEvent};
use crate::util::url::encode_component;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const EULER_API: &str = "https://api.eulerstream.com";
/// Page où créer une clé API gratuite (affichée à l'utilisateur).
pub const EULER_SITE: &str = "https://www.eulerstream.com";
/// Taille maximale d'un message du flux.
pub const MAX_WS_MESSAGE: usize = 8 * 1024 * 1024;

/// Demande de signature pour un live (`room_id` : chiffres seulement).
pub fn sign_request(room_id: &str, user_agent: &str, api_key: Option<&str>, version: &str) -> Option<HttpRequest> {
    if room_id.is_empty() || room_id.len() > 30 || !room_id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let url = format!("{EULER_API}/webcast/rooms/{room_id}/connect?client_enter=true&user_agent={}", encode_component(user_agent));
    let mut request = HttpRequest::get(url)
        .header("Accept", "application/protobuf,application/json")
        .header("User-Agent", format!("Batlay/{version}"))
        .timeout(Duration::from_secs(15))
        .max_body(4 * 1024 * 1024);
    request.follow_redirects = 0;
    if let Some(key) = api_key.map(str::trim).filter(|k| is_valid_key(k)) {
        // En-tête plutôt que paramètre d'adresse : la clé n'apparaît dans aucune adresse.
        request = request.header("x-api-key", key);
    }
    Some(request)
}

/// Clé API d'Euler : caractères imprimables simples, 8 à 200.
pub fn is_valid_key(key: &str) -> bool {
    (8..=200).contains(&key.len()) && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '=' | '+' | '/'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub ws_url: String,
    /// Cookie à envoyer à TikTok avec la WebSocket (`nom=valeur`).
    pub cookie: Option<String>,
    pub room_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignError {
    /// Trop de connexions (limites gratuites) : réessayer plus tard.
    RateLimited(String),
    /// Clé absente, refusée ou expirée.
    KeyRefused(String),
    /// Fonction réservée aux offres payantes.
    Premium(String),
    Other(String),
}

impl SignError {
    pub fn message(&self) -> String {
        match self {
            SignError::RateLimited(m) => format!("Euler Stream limite le nombre de connexions{}. Batlay réessaie tout seul ; une clé API gratuite (eulerstream.com) relève cette limite.", detail(m)),
            SignError::KeyRefused(m) => format!("Euler Stream refuse la connexion{} : ajoutez ou vérifiez votre clé API (gratuite sur eulerstream.com) dans la carte TikTok.", detail(m)),
            SignError::Premium(m) => format!("Euler Stream réserve cette connexion à ses offres payantes{}. Utilisez plutôt « Navigateur intégré ».", detail(m)),
            SignError::Other(m) => format!("Euler Stream n'a pas pu signer la connexion ({m}). Nouvel essai automatique."),
        }
    }
}

fn detail(m: &str) -> String {
    if m.is_empty() {
        String::new()
    } else {
        format!(" (« {} »)", m.chars().take(160).collect::<String>())
    }
}

/// Message d'erreur JSON d'Euler (`message`, `limit_label`).
fn error_text(resp: &HttpResponse) -> String {
    let json = resp.json().unwrap_or(crate::json::Value::Null);
    let label = json.get("limit_label").as_str().map(|l| format!("{l} : ")).unwrap_or_default();
    let message = json.get("message").as_str().unwrap_or("").chars().filter(|c| !c.is_control()).collect::<String>();
    format!("{label}{message}").trim().trim_end_matches(':').trim().to_string()
}

/// `nom=valeur` d'un en-tête du type Set-Cookie, s'il est sûr à renvoyer.
fn cookie_pair(header: &str) -> Option<String> {
    let pair = header.split(';').next()?.trim();
    let (name, value) = pair.split_once('=')?;
    let ok_name = !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    let ok_value = value.len() <= 4096 && value.chars().all(|c| c.is_ascii_graphic() && c != ';' && c != ',' && c != '"' && c != '\\');
    (ok_name && ok_value).then(|| format!("{name}={value}"))
}

/// Paramètres de la WebSocket, dans l'ordre de la page TikTok (repris de la bibliothèque).
fn ws_params(user_agent: &str, room_id: &str, cursor: &str, internal_ext: &str, extra: &[(String, String)]) -> Vec<(String, String)> {
    let browser_version = user_agent.strip_prefix("Mozilla/").unwrap_or(user_agent);
    let rtt = 100 + u64::from(crate::util::random::random_bytes(1)[0]) * 100 / 256;
    let mut params: Vec<(String, String)> = [
        ("version_code", "180800"),
        ("aid", "1988"),
        ("app_language", "fr"),
        ("app_name", "tiktok_web"),
        ("browser_platform", "Win32"),
        ("browser_language", "fr-FR"),
        ("browser_name", "Mozilla"),
        ("browser_version", browser_version),
        ("browser_online", "true"),
        ("cookie_enabled", "true"),
        ("tz_name", "Europe/Paris"),
        ("device_platform", "web"),
        ("identity", "audience"),
        ("live_id", "12"),
        ("webcast_language", "fr"),
        ("ws_direct", "0"),
        ("sup_ws_ds_opt", "1"),
        ("update_version_code", "2.0.0"),
        ("did_rule", "3"),
        ("screen_height", "1080"),
        ("screen_width", "1920"),
        ("heartbeat_duration", "0"),
        ("resp_content_type", "protobuf"),
        ("history_comment_count", "6"),
        ("client_enter", "1"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    params.push(("last_rtt".into(), rtt.to_string()));
    let mut set = |k: &str, v: &str| match params.iter_mut().find(|(key, _)| key == k) {
        Some(slot) => slot.1 = v.to_string(),
        None => params.push((k.to_string(), v.to_string())),
    };
    set("compress", "gzip");
    set("room_id", room_id);
    set("internal_ext", internal_ext);
    set("cursor", cursor);
    for (k, v) in extra {
        if !k.is_empty() && !v.is_empty() {
            set(k, v);
        }
    }
    params
}

/// Réponse d'Euler → adresse de la WebSocket TikTok (refusée si elle ne pointe pas vers TikTok).
pub fn parse_signed(resp: &HttpResponse, room_id: &str, user_agent: &str) -> Result<Signed, SignError> {
    match resp.status {
        200 => {}
        429 => return Err(SignError::RateLimited(error_text(resp))),
        401 | 403 => return Err(SignError::KeyRefused(error_text(resp))),
        402 => return Err(SignError::Premium(error_text(resp))),
        s => {
            let text = error_text(resp);
            return Err(SignError::Other(if text.is_empty() { format!("HTTP {s}") } else { format!("HTTP {s} : {text}") }));
        }
    }
    let room_id = resp.headers.get("x-room-id").map(str::trim).filter(|r| !r.is_empty() && r.len() <= 30 && r.chars().all(|c| c.is_ascii_digit())).unwrap_or(room_id).to_string();
    let Some(cookie) = resp.headers.get("x-set-tt-cookie").and_then(cookie_pair) else {
        let handshake = resp.headers.get("handshake-msg").map(|m| format!(" : {}", m.chars().take(120).collect::<String>())).unwrap_or_default();
        return Err(SignError::Other(format!("aucun cookie TikTok reçu{handshake}")));
    };
    let fetch = Msg::parse(&resp.body).map_err(|_| SignError::Other("réponse illisible".into()))?;
    let cursor = fetch.string(2).filter(|c| !c.is_empty()).ok_or_else(|| SignError::Other("réponse incomplète (cursor absent)".into()))?;
    let internal_ext = fetch.string(5).unwrap_or_default();
    let base = fetch.string(10).unwrap_or_default();
    if !is_webcast_socket_url(&base) || base.contains(['#', ' ', '\r', '\n']) {
        return Err(SignError::Other("adresse du flux absente ou hors de tiktok.com".into()));
    }
    let query: Vec<String> = ws_params(user_agent, &room_id, &cursor, &internal_ext, &fetch.string_map(7)).iter().map(|(k, v)| format!("{}={}", encode_component(k), encode_component(v))).collect();
    let sep = if base.contains('?') { '&' } else { '?' };
    Ok(Signed { ws_url: format!("{base}{sep}{}&version_code=270000", query.join("&")), cookie: Some(cookie), room_id })
}

// -- Trames envoyées à TikTok ----------------------------------------------------------

fn push_frame(log_id: Option<u64>, kind: &str, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    if let Some(id) = log_id {
        field_varint(2, id, &mut out);
    }
    field_bytes(6, b"pb", &mut out);
    field_bytes(7, kind.as_bytes(), &mut out);
    field_bytes(8, payload, &mut out);
    out
}

/// Entrée dans le live (`WebcastImEnterRoomMessage`).
pub fn enter_room_frame(room_id: u64, enter_unique_id: u64) -> Vec<u8> {
    let mut m = Vec::new();
    field_varint(1, room_id, &mut m);
    field_varint(4, 12, &mut m);
    field_bytes(5, b"audience", &mut m);
    field_varint(8, enter_unique_id, &mut m);
    field_bytes(9, b"0", &mut m);
    push_frame(None, "im_enter_room", &m)
}

/// Maintien de la connexion (`HeartbeatMessage`).
pub fn heartbeat_frame(room_id: u64, seq: u64) -> Vec<u8> {
    let mut m = Vec::new();
    field_varint(1, room_id, &mut m);
    field_varint(2, seq, &mut m);
    push_frame(None, "hb", &m)
}

/// Accusé de réception d'une trame.
pub fn ack_frame(log_id: u64, internal_ext: &str) -> Vec<u8> {
    push_frame(Some(log_id), "ack", internal_ext.as_bytes())
}

fn enter_unique_id() -> u64 {
    let b = crate::util::random::random_bytes(8);
    let v = u64::from_be_bytes(b.try_into().unwrap_or([0, 0, 0, 0, 0, 0, 0, 1])) & 0x7fff_ffff_ffff_ffff;
    v.max(1)
}

// -- Connexion au flux -------------------------------------------------------------------

pub struct EulerTiming {
    /// Délai pour recevoir la première trame après l'ouverture.
    pub first_frame: Duration,
    pub heartbeat: Duration,
}

impl Default for EulerTiming {
    fn default() -> Self {
        EulerTiming { first_frame: Duration::from_secs(20), heartbeat: Duration::from_secs(10) }
    }
}

/// Fait vivre la WebSocket (sur le fil appelant) jusqu'à la fin du live, une coupure ou l'arrêt.
pub fn run(mut conn: Box<dyn WsConn>, room_id: u64, stop: &AtomicBool, timing: EulerTiming, mut on_connected: impl FnMut(), mut on_event: impl FnMut(LiveEvent)) -> Outcome {
    let outcome = drive(conn.as_mut(), room_id, stop, &timing, &mut on_connected, &mut on_event);
    conn.close();
    outcome
}

fn drive(conn: &mut dyn WsConn, room_id: u64, stop: &AtomicBool, timing: &EulerTiming, on_connected: &mut impl FnMut(), on_event: &mut impl FnMut(LiveEvent)) -> Outcome {
    let lost = |connected: bool, why: String| if connected { Outcome::Lost(why) } else { Outcome::NoStream(why) };
    if let Err(e) = conn.send_binary(&enter_room_frame(room_id, enter_unique_id())) {
        return Outcome::NoStream(format!("entrée dans le live impossible ({e})"));
    }
    let opened = Instant::now();
    let mut next_heartbeat = opened + timing.heartbeat;
    let mut seq = 1u64;
    let mut connected = false;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Outcome::Stopped;
        }
        let now = Instant::now();
        if now >= next_heartbeat {
            if let Err(e) = conn.send_binary(&heartbeat_frame(room_id, seq)) {
                return lost(connected, format!("flux du live coupé ({e})"));
            }
            seq += 1;
            next_heartbeat = now + timing.heartbeat;
        }
        if !connected && now.duration_since(opened) >= timing.first_frame {
            return Outcome::NoStream("Flux du live ouvert, mais TikTok n'a rien envoyé.".into());
        }
        match conn.recv() {
            Ok(WsEvent::Binary(data)) => {
                if !connected {
                    connected = true;
                    on_connected();
                }
                match decode_push(&data) {
                    Ok(frame) => {
                        if let (true, Some(log_id), Some(ext)) = (frame.needs_ack, frame.log_id, frame.internal_ext.as_deref()) {
                            let _ = conn.send_binary(&ack_frame(log_id, ext));
                        }
                        for e in frame.events {
                            let end = e == LiveEvent::StreamEnd;
                            on_event(e);
                            if end {
                                return Outcome::Ended;
                            }
                        }
                    }
                    Err(e) => crate::log::warn(&format!("[TikTok/Euler] {e}")),
                }
            }
            Ok(WsEvent::Text(_)) | Ok(WsEvent::Idle) => {}
            Ok(WsEvent::Closed(code)) => return lost(connected, format!("flux du live fermé par TikTok{}", code.map(|c| format!(" (code {c})")).unwrap_or_default())),
            Err(e) => return lost(connected, format!("flux du live coupé ({e})")),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::net::{http::Headers, NetError};
    use crate::util::encoding::base64_decode;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    const UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

    /// Première réponse du flux telle qu'Euler la renvoie (fabriquée d'après le schéma).
    pub fn fetch_result(ws_url: &str) -> Vec<u8> {
        let mut out = Vec::new();
        field_bytes(2, b"1730000000000_cursor", &mut out);
        field_bytes(5, b"internal_ext:abc|wrds_v:1", &mut out);
        for (k, v) in [("wrss", "XyZ-123"), ("empty", "")] {
            let mut entry = Vec::new();
            field_bytes(1, k.as_bytes(), &mut entry);
            field_bytes(2, v.as_bytes(), &mut entry);
            field_bytes(7, &entry, &mut out);
        }
        field_bytes(10, ws_url.as_bytes(), &mut out);
        out
    }

    pub fn signed_response(ws_url: &str) -> HttpResponse {
        let mut headers = Headers::new();
        headers.set("x-set-tt-cookie", "ttwid=1%7CabcDEF; Path=/; Domain=tiktok.com; HttpOnly");
        headers.set("x-room-id", "7701654510762380054");
        HttpResponse { status: 200, headers, body: fetch_result(ws_url), url: String::new() }
    }

    fn json_response(status: u16, body: &str) -> HttpResponse {
        crate::net::fake_response(status, body)
    }

    #[test]
    fn sign_request_never_puts_the_key_in_the_address() {
        let r = sign_request("7701654510762380054", UA, Some("  euler_ABC123def  "), "0.5.1").unwrap();
        assert!(r.url.starts_with("https://api.eulerstream.com/webcast/rooms/7701654510762380054/connect?client_enter=true&user_agent=Mozilla%2F5.0"));
        assert!(!r.url.contains("ABC123"));
        assert!(r.headers.iter().any(|(k, v)| k == "x-api-key" && v == "euler_ABC123def"));
        assert_eq!(r.follow_redirects, 0);
        assert!(sign_request("12a", UA, None, "x").is_none());
        assert!(sign_request("../1", UA, None, "x").is_none());
        let no_key = sign_request("1", UA, Some("court"), "x").unwrap();
        assert!(!no_key.headers.iter().any(|(k, _)| k == "x-api-key"), "clé invalide : pas envoyée");
    }

    #[test]
    fn signed_websocket_address() {
        let s = parse_signed(&signed_response("wss://webcast-ws.tiktok.com/webcast/im/push/v2/"), "1", UA).unwrap();
        assert_eq!(s.cookie.as_deref(), Some("ttwid=1%7CabcDEF"));
        assert_eq!(s.room_id, "7701654510762380054", "x-room-id d'Euler");
        assert!(s.ws_url.starts_with("wss://webcast-ws.tiktok.com/webcast/im/push/v2/?version_code=180800&aid=1988&"));
        for part in ["&room_id=7701654510762380054&", "&cursor=1730000000000_cursor", "&internal_ext=internal_ext%3Aabc%7Cwrds_v%3A1&", "&compress=gzip&", "&wrss=XyZ-123", "browser_version=5.0%20(Windows"] {
            assert!(s.ws_url.contains(part), "{part} dans {}", s.ws_url);
        }
        assert!(!s.ws_url.contains("empty="), "paramètre vide ignoré");
        assert!(s.ws_url.ends_with("&version_code=270000"));
        // Une adresse hors de TikTok est refusée.
        for bad in ["wss://evil.example/webcast/im/push", "ws://webcast-ws.tiktok.com/webcast", ""] {
            assert!(matches!(parse_signed(&signed_response(bad), "1", UA), Err(SignError::Other(_))), "{bad}");
        }
    }

    #[test]
    fn sign_errors() {
        assert_eq!(parse_signed(&json_response(429, r#"{"code":429,"message":"Rate limit exceeded.","limit_label":"minute"}"#), "1", UA), Err(SignError::RateLimited("minute : Rate limit exceeded.".into())));
        assert!(matches!(parse_signed(&json_response(401, r#"{"message":"Invalid API key"}"#), "1", UA), Err(SignError::KeyRefused(m)) if m == "Invalid API key"));
        assert!(matches!(parse_signed(&json_response(402, "{}"), "1", UA), Err(SignError::Premium(_))));
        assert_eq!(parse_signed(&json_response(503, "pas du json"), "1", UA), Err(SignError::Other("HTTP 503".into())));
        // 200 sans cookie : erreur claire.
        let mut r = signed_response("wss://webcast-ws.tiktok.com/webcast/im/push/v2/");
        r.headers = Headers::new();
        assert!(matches!(parse_signed(&r, "1", UA), Err(SignError::Other(m)) if m.contains("cookie")));
        // Cookie dangereux (retour à la ligne, guillemets) : refusé.
        assert_eq!(cookie_pair("a=b\r\nX: y"), None);
        assert_eq!(cookie_pair("ttwid=\"x\""), None);
        assert_eq!(cookie_pair("tt_csrf=abc; Secure").as_deref(), Some("tt_csrf=abc"));
        for e in [SignError::RateLimited(String::new()), SignError::KeyRefused("x".into()), SignError::Premium(String::new()), SignError::Other("y".into())] {
            assert!(!e.message().is_empty());
        }
    }

    #[test]
    fn outgoing_frames_follow_the_schema() {
        let f = enter_room_frame(7701654510762380054, 99);
        let push = Msg::parse(&f).unwrap();
        assert_eq!((push.string(6).as_deref(), push.string(7).as_deref()), (Some("pb"), Some("im_enter_room")));
        let m = push.msg(8).unwrap();
        assert_eq!((m.uint(1), m.uint(4), m.string(5).as_deref(), m.uint(8), m.string(9).as_deref()), (Some(7701654510762380054), Some(12), Some("audience"), Some(99), Some("0")));
        let hb_raw = heartbeat_frame(5, 3);
        let hb = Msg::parse(&hb_raw).unwrap();
        assert_eq!(hb.string(7).as_deref(), Some("hb"));
        assert_eq!((hb.msg(8).unwrap().uint(1), hb.msg(8).unwrap().uint(2)), (Some(5), Some(3)));
        let ack_raw = ack_frame(42, "ext");
        let ack = Msg::parse(&ack_raw).unwrap();
        assert_eq!((ack.uint(2), ack.string(7).as_deref(), ack.string(8).as_deref()), (Some(42), Some("ack"), Some("ext")));
        // Nos propres trames de service ne donnent aucun événement.
        assert!(crate::tiktok::webcast::decode_frame(&hb_bytes()).unwrap().is_empty());
        assert!(enter_unique_id() >= 1);
    }

    fn hb_bytes() -> Vec<u8> {
        heartbeat_frame(1, 1)
    }

    /// Trame « msg » qui demande un accusé de réception.
    fn frame_needing_ack() -> Vec<u8> {
        let mut result = Vec::new();
        field_bytes(5, b"ext-1", &mut result);
        field_varint(9, 1, &mut result);
        let mut out = Vec::new();
        field_varint(2, 777, &mut out);
        field_bytes(6, b"pb", &mut out);
        field_bytes(7, b"msg", &mut out);
        field_bytes(8, &result, &mut out);
        out
    }

    pub struct FakeConn {
        pub incoming: VecDeque<Result<WsEvent, NetError>>,
        pub sent: Arc<Mutex<Vec<Vec<u8>>>>,
        pub closed: Arc<Mutex<bool>>,
    }
    impl WsConn for FakeConn {
        fn recv(&mut self) -> Result<WsEvent, NetError> {
            std::thread::sleep(Duration::from_millis(2));
            self.incoming.pop_front().unwrap_or(Ok(WsEvent::Idle))
        }
        fn close(&mut self) {
            *self.closed.lock().unwrap() = true;
        }
        fn send_binary(&mut self, data: &[u8]) -> Result<(), NetError> {
            self.sent.lock().unwrap().push(data.to_vec());
            Ok(())
        }
    }

    pub type Sent = Arc<Mutex<Vec<Vec<u8>>>>;

    pub fn fake(incoming: Vec<Result<WsEvent, NetError>>) -> (Box<dyn WsConn>, Sent, Arc<Mutex<bool>>) {
        let sent = Arc::new(Mutex::new(Vec::new()));
        let closed = Arc::new(Mutex::new(false));
        (Box::new(FakeConn { incoming: incoming.into(), sent: sent.clone(), closed: closed.clone() }), sent, closed)
    }

    fn kinds(sent: &[Vec<u8>]) -> Vec<String> {
        sent.iter().map(|f| Msg::parse(f).unwrap().string(7).unwrap()).collect()
    }

    fn fast() -> EulerTiming {
        EulerTiming { first_frame: Duration::from_millis(150), heartbeat: Duration::from_millis(40) }
    }

    #[test]
    fn enters_acks_decodes_and_ends() {
        let events = base64_decode(crate::tiktok::webcast::tests_fixtures::FRAME_EVENTS).unwrap();
        let end = base64_decode(crate::tiktok::webcast::tests_fixtures::FRAME_END).unwrap();
        let (conn, sent, closed) = fake(vec![Ok(WsEvent::Binary(frame_needing_ack())), Ok(WsEvent::Idle), Ok(WsEvent::Binary(events)), Ok(WsEvent::Binary(end))]);
        let mut connected = 0;
        let mut got = Vec::new();
        let out = run(conn, 7, &AtomicBool::new(false), fast(), || connected += 1, |e| got.push(e));
        assert_eq!((out, connected, got.len()), (Outcome::Ended, 1, 9));
        let sent = sent.lock().unwrap();
        assert_eq!(kinds(&sent)[..2], ["im_enter_room".to_string(), "ack".to_string()]);
        let ack = Msg::parse(&sent[1]).unwrap();
        assert_eq!((ack.uint(2), ack.string(8).as_deref()), (Some(777), Some("ext-1")));
        assert!(*closed.lock().unwrap());
    }

    #[test]
    fn heartbeats_no_data_and_closing() {
        // Rien reçu : « pas de flux » au bout du délai, après quelques heartbeats.
        let (conn, sent, _) = fake(vec![]);
        assert!(matches!(run(conn, 7, &AtomicBool::new(false), fast(), || {}, |_| {}), Outcome::NoStream(_)));
        let k = kinds(&sent.lock().unwrap());
        assert!(k.iter().filter(|x| *x == "hb").count() >= 2, "{k:?}");
        // Branché puis fermé par TikTok : live perdu.
        let (conn, _, _) = fake(vec![Ok(WsEvent::Binary(hb_bytes())), Ok(WsEvent::Closed(Some(1000)))]);
        assert_eq!(run(conn, 7, &AtomicBool::new(false), fast(), || {}, |_| {}), Outcome::Lost("flux du live fermé par TikTok (code 1000)".into()));
        // Fermé avant toute trame : pas de flux.
        let (conn, _, _) = fake(vec![Err(NetError::Protocol("x".into()))]);
        assert!(matches!(run(conn, 7, &AtomicBool::new(false), fast(), || {}, |_| {}), Outcome::NoStream(_)));
        // Arrêt demandé.
        let (conn, _, closed) = fake(vec![]);
        assert_eq!(run(conn, 7, &AtomicBool::new(true), fast(), || {}, |_| {}), Outcome::Stopped);
        assert!(*closed.lock().unwrap());
    }
}
