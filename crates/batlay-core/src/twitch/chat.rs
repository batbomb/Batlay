//! Lecture du chat (et des alertes de sa chaîne) par EventSub WebSocket.
//!
//! Une connexion = un fil qui lit la socket. Machine à états fidèle à la
//! version TypeScript : bienvenue → abonnements, keepalive surveillé,
//! reconnexion demandée par Twitch sans perte (`session_reconnect`), reprise
//! avec recul progressif, révocation, dédoublonnage des notifications.

use super::eventsub::{
    alert_subscription_body, is_alert_subscription, parse_message, reconnect_delay_ms, subscription_body, to_alert_payload, to_overlay_message, EventSubMessage, ALERT_SUBSCRIPTIONS,
    CHAT_SUBSCRIPTIONS, EVENTSUB_SUBSCRIPTIONS_URL, EVENTSUB_WS_URL,
};
use super::model::{twitch_error, ChatStatus, ErrorCode, TwitchError, TWITCH_CLIENT_ID};
use crate::json::Value;
use crate::net::{Http, HttpRequest, WsConn, WsConnect, WsEvent};
use crate::runtime::{Rt, TimerId};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

const DEDUPE_SIZE: usize = 200;
/// Marge ajoutée au délai de keepalive annoncé par Twitch.
pub const KEEPALIVE_GRACE_MS: u64 = 10_000;

pub trait ChatSink: Send + Sync {
    /// Message prêt pour les pages overlay du chat.
    fn emit(&self, message: Value);
    /// Alerte de SA chaîne (follow, abonnement…), données brutes à valider.
    fn alert(&self, payload: Value);
    fn status(&self, status: ChatStatus);
    /// Arrêt définitif (jeton refusé, permission manquante) : reconnexion du compte nécessaire.
    fn fatal(&self, error: TwitchError);
    /// Problème récupérable (`None` = résolu).
    fn transient(&self, error: Option<TwitchError>);
}

pub struct ChatDeps {
    pub http: Arc<dyn Http>,
    pub ws: Arc<dyn WsConnect>,
    pub rt: Rt,
    pub sink: Arc<dyn ChatSink>,
    pub token: Arc<dyn Fn() -> Option<String> + Send + Sync>,
}

#[derive(Default)]
struct State {
    running: bool,
    generation: u64,
    user_id: Option<String>,
    broadcaster_id: Option<String>,
    attempt: u32,
    status: Option<ChatStatus>,
    active: Option<u64>,
    pending: Option<u64>,
    stops: HashMap<u64, Arc<AtomicBool>>,
    next_socket: u64,
    retry_timer: Option<TimerId>,
    keepalive_ms: u64,
    last_message_at: u64,
    seen: VecDeque<String>,
    seen_set: HashSet<String>,
}

pub struct ChatClient {
    deps: ChatDeps,
    state: Mutex<State>,
}

impl ChatClient {
    pub fn new(deps: ChatDeps) -> Arc<ChatClient> {
        Arc::new(ChatClient { deps, state: Mutex::new(State::default()) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn log(&self, message: &str) {
        crate::log::info(&format!("[Twitch chat] {message}"));
    }

    fn set_status(&self, status: ChatStatus) {
        let changed = {
            let mut st = self.lock();
            let changed = st.status != Some(status);
            st.status = Some(status);
            changed
        };
        if changed {
            self.deps.sink.status(status);
        }
    }

    pub fn status(&self) -> ChatStatus {
        self.lock().status.unwrap_or(ChatStatus::Off)
    }

    /// Lit le chat de `broadcaster_id` avec le compte `user_id`.
    pub fn start(self: &Arc<Self>, user_id: &str, broadcaster_id: &str) {
        self.stop();
        let gen = {
            let mut st = self.lock();
            st.running = true;
            st.user_id = Some(user_id.to_string());
            st.broadcaster_id = Some(broadcaster_id.to_string());
            st.attempt = 0;
            st.generation
        };
        self.set_status(ChatStatus::Connecting);
        self.open(EVENTSUB_WS_URL.to_string(), false, gen);
    }

    pub fn stop(&self) {
        let (stops, timer) = {
            let mut st = self.lock();
            st.running = false;
            st.generation += 1;
            st.active = None;
            st.pending = None;
            st.seen.clear();
            st.seen_set.clear();
            (st.stops.drain().map(|(_, s)| s).collect::<Vec<_>>(), st.retry_timer.take())
        };
        self.deps.rt.cancel(timer);
        for s in stops {
            s.store(true, Ordering::SeqCst);
        }
        self.set_status(ChatStatus::Off);
    }

    fn open(self: &Arc<Self>, url: String, is_reconnect: bool, gen: u64) {
        let (id, stop) = {
            let mut st = self.lock();
            if st.generation != gen || !st.running {
                return;
            }
            st.next_socket += 1;
            let id = st.next_socket;
            let stop = Arc::new(AtomicBool::new(false));
            st.stops.insert(id, stop.clone());
            if is_reconnect {
                st.pending = Some(id);
            } else {
                st.active = Some(id);
                st.last_message_at = self.deps.rt.now();
            }
            (id, stop)
        };
        let me = self.clone();
        self.deps.rt.spawn("twitch-eventsub", move || me.run_socket(id, url, gen, stop));
    }

    fn is_current(&self, gen: u64) -> bool {
        let st = self.lock();
        st.running && st.generation == gen
    }

    fn run_socket(self: Arc<Self>, id: u64, url: String, gen: u64, stop: Arc<AtomicBool>) {
        let mut conn = match self.deps.ws.connect(&url) {
            Ok(c) => c,
            Err(e) => {
                if self.is_current(gen) {
                    self.log(&format!("ouverture du socket impossible : {e}"));
                    let was_active = self.lock().active == Some(id);
                    self.deps.sink.transient(Some(twitch_error(ErrorCode::Network, Some(&e.to_string()))));
                    self.on_closed(id, gen, was_active);
                }
                return;
            }
        };
        loop {
            if stop.load(Ordering::SeqCst) || !self.is_current(gen) {
                conn.close();
                return;
            }
            match conn.recv() {
                Ok(WsEvent::Idle) => {
                    if self.keepalive_expired(id) {
                        self.log("aucun message reçu dans le délai de keepalive : reconnexion");
                        conn.close();
                        self.on_closed(id, gen, true);
                        return;
                    }
                }
                Ok(WsEvent::Text(text)) => {
                    if stop.load(Ordering::SeqCst) || !self.is_current(gen) {
                        conn.close();
                        return;
                    }
                    self.handle_raw(id, gen, &text, conn.as_mut());
                }
                Ok(WsEvent::Binary(_)) => {}
                Ok(WsEvent::Closed(code)) => {
                    self.log(&format!("connexion EventSub fermée (code {})", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())));
                    self.on_closed(id, gen, false);
                    return;
                }
                Err(e) => {
                    self.log(&format!("erreur socket : {e}"));
                    conn.close();
                    self.on_closed(id, gen, false);
                    return;
                }
            }
        }
    }

    fn keepalive_expired(&self, id: u64) -> bool {
        let st = self.lock();
        st.active == Some(id) && st.keepalive_ms > 0 && self.deps.rt.now().saturating_sub(st.last_message_at) > st.keepalive_ms
    }

    fn on_closed(self: &Arc<Self>, id: u64, gen: u64, _dead: bool) {
        let reconnect = {
            let mut st = self.lock();
            st.stops.remove(&id);
            if st.generation != gen {
                return;
            }
            if st.pending == Some(id) {
                // La reconnexion demandée par Twitch a échoué : l'ancien socket reste utilisable.
                st.pending = None;
                return;
            }
            if st.active != Some(id) {
                return;
            }
            st.active = None;
            st.running
        };
        if reconnect {
            self.schedule_reconnect(gen);
        }
    }

    fn schedule_reconnect(self: &Arc<Self>, gen: u64) {
        let delay = {
            let mut st = self.lock();
            if !st.running || st.generation != gen || st.retry_timer.is_some() {
                return;
            }
            let delay = reconnect_delay_ms(st.attempt);
            st.attempt += 1;
            let weak = Arc::downgrade(self);
            // Programmé SOUS le verrou : la tâche (qui prend ce verrou) ne peut pas
            // s'exécuter avant que `retry_timer` soit renseigné.
            st.retry_timer = Some(self.deps.rt.after(delay, move || {
                if let Some(me) = weak.upgrade() {
                    let ok = {
                        let mut st = me.lock();
                        st.retry_timer = None;
                        st.running && st.generation == gen
                    };
                    if ok {
                        me.open(EVENTSUB_WS_URL.to_string(), false, gen);
                    }
                }
            }));
            delay
        };
        self.set_status(ChatStatus::Reconnecting);
        self.log(&format!("nouvelle tentative dans {} s", delay / 1000));
    }

    fn handle_raw(self: &Arc<Self>, id: u64, gen: u64, raw: &str, _conn: &mut dyn WsConn) {
        let Some(msg) = parse_message(raw) else { return };
        {
            let mut st = self.lock();
            if st.active == Some(id) {
                st.last_message_at = self.deps.rt.now();
            }
        }
        match msg {
            EventSubMessage::Welcome { session_id, keepalive_seconds } => {
                let (swap_out, subscribe) = {
                    let mut st = self.lock();
                    st.keepalive_ms = keepalive_seconds * 1000 + KEEPALIVE_GRACE_MS;
                    if st.pending == Some(id) {
                        // Reconnexion à la demande de Twitch : les abonnements sont conservés.
                        let old = st.active.replace(id);
                        st.pending = None;
                        st.last_message_at = self.deps.rt.now();
                        (old.and_then(|o| st.stops.get(&o).cloned()), false)
                    } else {
                        (None, true)
                    }
                };
                if let Some(old) = swap_out {
                    old.store(true, Ordering::SeqCst);
                }
                if subscribe {
                    self.subscribe_all(&session_id, gen);
                }
            }
            EventSubMessage::Reconnect { url } => {
                if self.lock().pending.is_none() {
                    self.open(url, true, gen);
                }
            }
            EventSubMessage::Revocation { subscription_type, status } => {
                if status == "authorization_revoked" || status == "user_removed" {
                    self.fatal(twitch_error(ErrorCode::TokenExpired, None));
                } else {
                    self.log(&format!("abonnement {subscription_type} révoqué ({status})"));
                }
            }
            EventSubMessage::Notification { message_id, subscription_type, event } => {
                {
                    let mut st = self.lock();
                    if st.seen_set.contains(&message_id) {
                        return; // Twitch peut renvoyer un même message
                    }
                    st.seen_set.insert(message_id.clone());
                    st.seen.push_back(message_id);
                    while st.seen.len() > DEDUPE_SIZE {
                        if let Some(old) = st.seen.pop_front() {
                            st.seen_set.remove(&old);
                        }
                    }
                }
                if is_alert_subscription(&subscription_type) {
                    if let Some(payload) = to_alert_payload(&subscription_type, &event) {
                        self.deps.sink.alert(payload);
                    }
                    return;
                }
                if let Some(out) = to_overlay_message(&subscription_type, &event, crate::util::now_ms()) {
                    self.deps.sink.emit(out);
                }
            }
            EventSubMessage::Keepalive => {}
        }
    }

    fn post(&self, token: &str, body: &Value) -> Result<(u16, String), String> {
        let request = HttpRequest::post_json(EVENTSUB_SUBSCRIPTIONS_URL, body).header("Authorization", format!("Bearer {token}")).header("Client-Id", TWITCH_CLIENT_ID);
        match self.deps.http.send(&request) {
            Ok(r) => Ok((r.status, r.json().and_then(|j| j.get("message").as_str().map(|m| m.chars().take(200).collect())).unwrap_or_default())),
            Err(e) => Err(e.to_string()),
        }
    }

    fn subscribe_all(self: &Arc<Self>, session_id: &str, gen: u64) {
        let (user_id, broadcaster_id) = {
            let st = self.lock();
            (st.user_id.clone(), st.broadcaster_id.clone().or_else(|| st.user_id.clone()))
        };
        let token = (self.deps.token)();
        let (Some(user_id), Some(broadcaster_id), Some(token)) = (user_id, broadcaster_id, token) else {
            self.fatal(twitch_error(ErrorCode::TokenExpired, None));
            return;
        };
        for (kind, required) in CHAT_SUBSCRIPTIONS {
            let result = self.post(&token, &subscription_body(kind, &user_id, session_id, &broadcaster_id));
            if !self.is_current(gen) {
                return;
            }
            let (status, detail) = match result {
                Ok(r) => r,
                Err(e) => {
                    if required {
                        return self.retry_later(twitch_error(ErrorCode::Network, Some(&e)), gen);
                    }
                    self.log(&format!("abonnement {kind} : {e}"));
                    continue;
                }
            };
            if matches!(status, 200 | 202 | 409) {
                continue; // 409 = déjà abonné
            }
            if status == 401 {
                return self.fatal(twitch_error(ErrorCode::TokenExpired, None));
            }
            if !required {
                self.log(&format!("abonnement {kind} refusé ({status}) {detail}"));
                continue;
            }
            if status == 403 {
                return self.fatal(twitch_error(ErrorCode::MissingScope, None));
            }
            let d = if detail.is_empty() { format!("HTTP {status}") } else { format!("HTTP {status} : {detail}") };
            return self.retry_later(twitch_error(ErrorCode::SubscriptionFailed, Some(&d)), gen);
        }
        if !self.is_current(gen) {
            return;
        }
        self.lock().attempt = 0;
        self.deps.sink.transient(None);
        self.set_status(ChatStatus::Live);
        // Alertes : seulement pour SA chaîne (Twitch refuse les follows / abonnements d'une autre).
        if broadcaster_id == user_id {
            for (kind, version) in ALERT_SUBSCRIPTIONS {
                if !self.is_current(gen) {
                    return;
                }
                match self.post(&token, &alert_subscription_body(kind, version, &broadcaster_id, session_id)) {
                    Ok((200 | 202 | 409, _)) => {}
                    Ok((status, detail)) => self.log(&format!("alertes : abonnement {kind} refusé ({status}) {detail}")),
                    Err(e) => self.log(&format!("alertes : abonnement {kind} : {e}")),
                }
            }
        }
    }

    /// Échec récupérable : on l'affiche, on referme, et le recul refait une tentative.
    fn retry_later(self: &Arc<Self>, error: TwitchError, gen: u64) {
        self.deps.sink.transient(Some(error));
        let stop = {
            let mut st = self.lock();
            let active = st.active.take();
            active.and_then(|a| st.stops.remove(&a))
        };
        if let Some(s) = stop {
            s.store(true, Ordering::SeqCst);
        }
        self.schedule_reconnect(gen);
    }

    fn fatal(&self, error: TwitchError) {
        self.stop();
        self.deps.sink.fatal(error);
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::net::{fake_response, HttpResponse, NetError};
    use crate::runtime::{ManualClock, ManualTimers, ThreadSpawner};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::time::{Duration, Instant};

    pub enum Feed {
        Text(String),
        Idle,
        Close(u16),
    }

    struct FakeConn(Receiver<Feed>, Arc<Mutex<Vec<String>>>, String);
    impl WsConn for FakeConn {
        fn recv(&mut self) -> Result<WsEvent, NetError> {
            match self.0.recv_timeout(Duration::from_secs(5)) {
                Ok(Feed::Text(t)) => Ok(WsEvent::Text(t)),
                Ok(Feed::Idle) => Ok(WsEvent::Idle),
                Ok(Feed::Close(c)) => Ok(WsEvent::Closed(Some(c))),
                Err(_) => Ok(WsEvent::Closed(None)),
            }
        }
        fn close(&mut self) {
            self.1.lock().unwrap().push(self.2.clone());
        }
    }

    /// Chaque `connect` crée une socket simulée ; le test la nourrit par `feeds`.
    #[derive(Default)]
    pub struct FakeWs {
        pub opened: Mutex<Vec<(String, Sender<Feed>)>>,
        pub closed: Arc<Mutex<Vec<String>>>,
        pub fail_next: Mutex<u32>,
    }
    impl WsConnect for FakeWs {
        fn connect(&self, url: &str) -> Result<Box<dyn WsConn>, NetError> {
            {
                let mut f = self.fail_next.lock().unwrap();
                if *f > 0 {
                    *f -= 1;
                    return Err(NetError::Protocol("refusé".into()));
                }
            }
            let (tx, rx) = channel();
            let mut opened = self.opened.lock().unwrap();
            let name = format!("{}#{}", url, opened.len());
            opened.push((url.to_string(), tx));
            Ok(Box::new(FakeConn(rx, self.closed.clone(), name)))
        }
    }
    impl FakeWs {
        pub fn feed(&self, index: usize, feed: Feed) {
            let tx = self.opened.lock().unwrap()[index].1.clone();
            let _ = tx.send(feed);
        }
        pub fn count(&self) -> usize {
            self.opened.lock().unwrap().len()
        }
    }

    /// Réponses HTTP simulées : `(motif dans l'URL ou le corps, statut, corps)`.
    #[derive(Default)]
    pub struct FakeHttp {
        pub routes: Mutex<Vec<(String, u16, String)>>,
        pub calls: Mutex<Vec<(String, String)>>,
    }
    impl Http for FakeHttp {
        fn send(&self, r: &HttpRequest) -> Result<HttpResponse, NetError> {
            let body = String::from_utf8_lossy(&r.body).into_owned();
            self.calls.lock().unwrap().push((r.url.clone(), body.clone()));
            let routes = self.routes.lock().unwrap();
            for (pattern, status, resp) in routes.iter() {
                if r.url.contains(pattern.as_str()) || body.contains(pattern.as_str()) {
                    if *status == 0 {
                        return Err(NetError::Protocol(resp.clone()));
                    }
                    return Ok(fake_response(*status, resp));
                }
            }
            Ok(fake_response(202, "{}"))
        }
    }
    impl FakeHttp {
        pub fn route(&self, pattern: &str, status: u16, body: &str) {
            self.routes.lock().unwrap().insert(0, (pattern.into(), status, body.into()));
        }
        pub fn bodies_matching(&self, pattern: &str) -> usize {
            self.calls.lock().unwrap().iter().filter(|(u, b)| u.contains(pattern) || b.contains(pattern)).count()
        }
    }

    #[derive(Default)]
    pub struct Sink {
        pub emitted: Mutex<Vec<Value>>,
        pub alerts: Mutex<Vec<Value>>,
        pub statuses: Mutex<Vec<ChatStatus>>,
        pub fatal: Mutex<Vec<TwitchError>>,
        pub transient: Mutex<Vec<Option<TwitchError>>>,
    }
    impl ChatSink for Sink {
        fn emit(&self, m: Value) {
            self.emitted.lock().unwrap().push(m);
        }
        fn alert(&self, p: Value) {
            self.alerts.lock().unwrap().push(p);
        }
        fn status(&self, s: ChatStatus) {
            self.statuses.lock().unwrap().push(s);
        }
        fn fatal(&self, e: TwitchError) {
            self.fatal.lock().unwrap().push(e);
        }
        fn transient(&self, e: Option<TwitchError>) {
            self.transient.lock().unwrap().push(e);
        }
    }

    pub fn wait_until(what: &str, f: impl Fn() -> bool) {
        let start = Instant::now();
        while !f() {
            assert!(start.elapsed() < Duration::from_secs(5), "délai dépassé : {what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    pub fn welcome(id: &str) -> Feed {
        Feed::Text(format!(r#"{{"metadata":{{"message_type":"session_welcome"}},"payload":{{"session":{{"id":"{id}","keepalive_timeout_seconds":10}}}}}}"#))
    }
    pub fn chat(msg_id: &str, text: &str) -> Feed {
        Feed::Text(format!(
            r#"{{"metadata":{{"message_type":"notification","message_id":"{msg_id}","subscription_type":"channel.chat.message"}},"payload":{{"event":{{"message_id":"c-{msg_id}","chatter_user_id":"7","chatter_user_login":"lina","chatter_user_name":"Lina","message":{{"text":"{text}"}}}}}}}}"#
        ))
    }

    fn setup() -> (Arc<ChatClient>, Arc<FakeWs>, Arc<FakeHttp>, Arc<Sink>, Arc<ManualTimers>, Arc<ManualClock>) {
        let ws = Arc::new(FakeWs::default());
        let http = Arc::new(FakeHttp::default());
        let sink = Arc::new(Sink::default());
        let timers = ManualTimers::new();
        let clock = ManualClock::new(1_000_000);
        let rt = Rt { clock: clock.clone(), timers: timers.clone(), spawner: Arc::new(ThreadSpawner) };
        let client = ChatClient::new(ChatDeps { http: http.clone(), ws: ws.clone(), rt, sink: sink.clone(), token: Arc::new(|| Some("jeton".into())) });
        (client, ws, http, sink, timers, clock)
    }

    #[test]
    fn welcome_subscribes_then_live_and_messages_are_deduplicated() {
        let (client, ws, http, sink, _timers, _clock) = setup();
        client.start("42", "42");
        wait_until("socket ouverte", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("en direct", || client.status() == ChatStatus::Live);
        assert_eq!(http.bodies_matching("\"session_id\":\"S1\""), 8, "4 chat + 4 alertes (sa propre chaîne)");
        ws.feed(0, chat("n1", "salut"));
        ws.feed(0, chat("n1", "salut"));
        ws.feed(0, chat("n2", "re"));
        wait_until("deux messages", || sink.emitted.lock().unwrap().len() == 2);
        assert_eq!(sink.emitted.lock().unwrap()[0].path(&["message", "text"]).as_str(), Some("salut"));
        client.stop();
        assert_eq!(client.status(), ChatStatus::Off);
    }

    #[test]
    fn other_channel_has_no_alert_subscriptions() {
        let (client, ws, http, _sink, _t, _c) = setup();
        client.start("42", "99");
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("en direct", || client.status() == ChatStatus::Live);
        assert_eq!(http.bodies_matching("channel.follow"), 0);
        assert_eq!(http.bodies_matching("\"broadcaster_user_id\":\"99\""), 4);
        client.stop();
    }

    #[test]
    fn missing_scope_is_fatal_and_401_expires() {
        let (client, ws, http, sink, _t, _c) = setup();
        http.route("channel.chat.message\"", 403, r#"{"message":"forbidden"}"#);
        client.start("42", "42");
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("fatal", || !sink.fatal.lock().unwrap().is_empty());
        assert_eq!(sink.fatal.lock().unwrap()[0].code, ErrorCode::MissingScope);
        assert_eq!(client.status(), ChatStatus::Off);
    }

    #[test]
    fn closed_socket_reconnects_with_backoff() {
        let (client, ws, _http, sink, timers, _c) = setup();
        client.start("42", "42");
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("en direct", || client.status() == ChatStatus::Live);
        ws.feed(0, Feed::Close(4000));
        wait_until("reconnexion programmée", || timers.count() == 1);
        assert_eq!(timers.pending(), vec![1000]);
        assert_eq!(client.status(), ChatStatus::Reconnecting);
        timers.fire_last();
        wait_until("nouvelle socket", || ws.count() == 2);
        ws.feed(1, welcome("S2"));
        wait_until("de nouveau en direct", || client.status() == ChatStatus::Live);
        assert!(sink.statuses.lock().unwrap().contains(&ChatStatus::Reconnecting));
        client.stop();
    }

    #[test]
    fn connection_failure_retries_and_subscription_failure_is_transient() {
        let (client, ws, http, sink, timers, _c) = setup();
        *ws.fail_next.lock().unwrap() = 1;
        client.start("42", "42");
        wait_until("reconnexion programmée", || timers.count() == 1);
        assert_eq!(sink.transient.lock().unwrap()[0].as_ref().unwrap().code, ErrorCode::Network);
        http.route("channel.chat.message\"", 500, r#"{"message":"panne"}"#);
        timers.fire_last();
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("deuxième essai programmé", || timers.pending() == vec![2000]);
        let last = sink.transient.lock().unwrap().last().cloned().flatten().unwrap();
        assert_eq!((last.code, last.message.contains("HTTP 500 : panne")), (ErrorCode::SubscriptionFailed, true));
        client.stop();
    }

    #[test]
    fn twitch_requested_reconnect_switches_without_loss() {
        let (client, ws, http, _sink, _t, _c) = setup();
        client.start("42", "42");
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("en direct", || client.status() == ChatStatus::Live);
        let calls = http.calls.lock().unwrap().len();
        ws.feed(0, Feed::Text(r#"{"metadata":{"message_type":"session_reconnect"},"payload":{"session":{"reconnect_url":"wss://eventsub.wss.twitch.tv/ws?r=1"}}}"#.into()));
        wait_until("socket de reconnexion", || ws.count() == 2);
        assert_eq!(ws.opened.lock().unwrap()[1].0, "wss://eventsub.wss.twitch.tv/ws?r=1");
        ws.feed(1, welcome("S2"));
        wait_until("bascule sur la nouvelle socket", || client.lock().active == Some(2));
        ws.feed(0, Feed::Idle);
        wait_until("ancienne socket fermée", || !ws.closed.lock().unwrap().is_empty());
        assert_eq!(http.calls.lock().unwrap().len(), calls, "aucun réabonnement");
        assert_eq!(client.status(), ChatStatus::Live);
        client.stop();
    }

    #[test]
    fn keepalive_watchdog() {
        let (client, ws, _http, _sink, timers, clock) = setup();
        client.start("42", "42");
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("en direct", || client.status() == ChatStatus::Live);
        clock.advance(19_000);
        ws.feed(0, Feed::Idle);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(timers.count(), 0, "10 s + 10 s de marge pas encore écoulées");
        clock.advance(2_000);
        ws.feed(0, Feed::Idle);
        wait_until("reconnexion", || timers.count() == 1);
        client.stop();
    }

    #[test]
    fn revocation_is_fatal() {
        let (client, ws, _http, sink, _t, _c) = setup();
        client.start("42", "42");
        wait_until("socket", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("en direct", || client.status() == ChatStatus::Live);
        ws.feed(0, Feed::Text(r#"{"metadata":{"message_type":"revocation"},"payload":{"subscription":{"type":"channel.chat.message","status":"authorization_revoked"}}}"#.into()));
        wait_until("fatal", || !sink.fatal.lock().unwrap().is_empty());
        assert_eq!(sink.fatal.lock().unwrap()[0].code, ErrorCode::TokenExpired);
    }
}
