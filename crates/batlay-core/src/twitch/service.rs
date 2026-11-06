//! Point d'entrée du module Twitch : serveur local (port 4000), session,
//! chat EventSub, choix de la chaîne. Indépendant du serveur des overlays :
//! une panne de l'un ne touche pas l'autre.

use super::auth::{fetch_badge_map, fetch_channel_by_login, AuthEvents, AuthPatch, ChannelLookup, Session, TokenStore, TwitchAuth};
use super::chat::{ChatClient, ChatDeps, ChatSink};
use super::model::{parse_channel, twitch_error, ChatStatus, ErrorCode, TwitchAccount, TwitchError, TwitchState};
use super::oauth::CallbackPayload;
use super::server::{TwitchServer, TwitchServerEvents};
use crate::json::Value;
use crate::net::{Http, WsConnect};
use crate::obj;
use crate::runtime::{Rt, TimerId};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, Weak};

const CHANNEL_RETRY_MS: u64 = 15_000;

/// Ce que le service publie vers le reste de Batlay.
pub trait TwitchHost: Send + Sync {
    fn state_changed(&self, state: &TwitchState);
    /// Alerte de SA chaîne (follow, abonnement…), données brutes à valider.
    fn alert(&self, payload: Value);
    /// Copie de chaque message envoyé aux overlays du chat (fenêtre Live).
    fn overlay_message(&self, message: &Value);
    fn open_external(&self, url: &str) -> Result<(), String>;
}

pub struct TwitchConfig {
    pub port: u16,
    pub static_dir: PathBuf,
    pub session_file: PathBuf,
}

struct ServiceState {
    state: TwitchState,
    chat_generation: u64,
    channel_retry: Option<TimerId>,
    test_counter: u64,
}

pub struct TwitchService {
    me: Weak<TwitchService>,
    state: Mutex<ServiceState>,
    server: TwitchServer,
    auth: Arc<TwitchAuth>,
    chat: Arc<ChatClient>,
    http: Arc<dyn Http>,
    host: Arc<dyn TwitchHost>,
    rt: Rt,
}

/// Petits adaptateurs : les sous-modules parlent au service sans le posséder.
struct Bridge(Weak<TwitchService>);

impl AuthEvents for Bridge {
    fn patch(&self, patch: AuthPatch) {
        if let Some(s) = self.0.upgrade() {
            s.apply_auth_patch(patch);
        }
    }
    fn logged_in(&self, session: Session) {
        if let Some(s) = self.0.upgrade() {
            let me = s.clone();
            s.rt.spawn("twitch-chat-start", move || me.start_chat(session));
        }
    }
    fn logged_out(&self) {
        if let Some(s) = self.0.upgrade() {
            s.on_logged_out();
        }
    }
    fn open_external(&self, url: &str) -> Result<(), String> {
        match self.0.upgrade() {
            Some(s) => s.host.open_external(url),
            None => Err("Batlay se ferme".into()),
        }
    }
}

impl ChatSink for Bridge {
    fn emit(&self, message: Value) {
        if let Some(s) = self.0.upgrade() {
            s.server.broadcast(&message, true);
            s.host.overlay_message(&message);
        }
    }
    fn alert(&self, payload: Value) {
        if let Some(s) = self.0.upgrade() {
            s.host.alert(payload);
        }
    }
    fn status(&self, status: ChatStatus) {
        if let Some(s) = self.0.upgrade() {
            s.patch(|st| st.chat = status);
        }
    }
    fn fatal(&self, error: TwitchError) {
        if let Some(s) = self.0.upgrade() {
            s.auth.expire(error);
        }
    }
    fn transient(&self, error: Option<TwitchError>) {
        if let Some(s) = self.0.upgrade() {
            s.patch(|st| {
                if st.status == super::model::AuthStatus::Connected {
                    st.error = error;
                }
            });
        }
    }
}

impl TwitchServerEvents for Bridge {
    fn auth_callback(&self, payload: CallbackPayload) -> (bool, String) {
        match self.0.upgrade() {
            Some(s) => s.auth.handle_callback(payload),
            None => (false, "Batlay se ferme.".into()),
        }
    }
    fn clients_changed(&self, count: usize) {
        if let Some(s) = self.0.upgrade() {
            s.patch(|st| st.overlay_clients = count);
        }
    }
}

impl TwitchService {
    pub fn new(config: TwitchConfig, http: Arc<dyn Http>, ws: Arc<dyn WsConnect>, secrets: Arc<dyn crate::config::SecretBox>, host: Arc<dyn TwitchHost>, rt: Rt) -> Arc<TwitchService> {
        Arc::new_cyclic(|me: &Weak<TwitchService>| {
            let store = TokenStore::new(&config.session_file, secrets);
            let channel_login = store.channel_login();
            let auth = TwitchAuth::new(http.clone(), rt.clone(), store, Arc::new(Bridge(me.clone())));
            let token_source = Arc::downgrade(&auth);
            let chat = ChatClient::new(ChatDeps {
                http: http.clone(),
                ws,
                rt: rt.clone(),
                sink: Arc::new(Bridge(me.clone())),
                token: Arc::new(move || token_source.upgrade().and_then(|a| a.token())),
            });
            TwitchService {
                me: me.clone(),
                state: Mutex::new(ServiceState { state: TwitchState { channel_login, ..Default::default() }, chat_generation: 0, channel_retry: None, test_counter: 0 }),
                server: TwitchServer::new(config.port, config.static_dir, Arc::new(Bridge(me.clone()))),
                auth,
                chat,
                http,
                host,
                rt,
            }
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, ServiceState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> TwitchState {
        self.lock().state.clone()
    }

    fn patch(&self, f: impl FnOnce(&mut TwitchState)) {
        let snapshot = {
            let mut st = self.lock();
            f(&mut st.state);
            st.state.clone()
        };
        self.host.state_changed(&snapshot);
    }

    fn apply_auth_patch(&self, p: AuthPatch) {
        self.patch(|st| {
            if let Some(v) = p.status {
                st.status = v;
            }
            if let Some(v) = p.account {
                st.account = v;
            }
            if let Some(v) = p.token_expires_at {
                st.token_expires_at = v;
            }
            if let Some(v) = p.error {
                st.error = v;
            }
            if let Some(v) = p.warning {
                st.warning = v;
            }
            if let Some((f, s)) = p.alert_scopes {
                st.alert_follows = f;
                st.alert_subscriptions = s;
            }
        });
    }

    /// Démarre le serveur (port 4000) puis restaure la session en arrière-plan.
    pub fn start(self: &Arc<Self>) {
        if self.start_server() {
            let auth = self.auth.clone();
            self.rt.spawn("twitch-restore", move || auth.restore());
        }
    }

    fn start_server(&self) -> bool {
        if self.server.is_running() {
            return true;
        }
        match self.server.start() {
            Ok(()) => {
                self.patch(|st| {
                    st.server_running = true;
                    if matches!(st.error.as_ref().map(|e| e.code), Some(ErrorCode::PortInUse | ErrorCode::ServerError)) {
                        st.error = None;
                    }
                });
                true
            }
            Err(e) => {
                crate::log::error(&format!("Serveur Twitch indisponible : {e}"));
                let error = if e.kind() == std::io::ErrorKind::AddrInUse { twitch_error(ErrorCode::PortInUse, None) } else { twitch_error(ErrorCode::ServerError, Some(&e.to_string())) };
                self.patch(|st| {
                    st.server_running = false;
                    st.error = Some(error);
                });
                false
            }
        }
    }

    pub fn connect(self: &Arc<Self>) {
        // Cas typique : le port était occupé au lancement et vient d'être libéré.
        if self.start_server() {
            self.auth.begin(false);
        }
    }

    /// Déjà connecté : redemande les permissions d'alertes (le chat continue entre-temps).
    pub fn reauthorize(self: &Arc<Self>) {
        if self.start_server() {
            self.auth.begin(true);
        }
    }

    pub fn cancel(&self) {
        self.auth.cancel();
    }

    pub fn disconnect(&self) {
        self.auth.disconnect();
    }

    /// Chaîne à afficher : pseudo ou lien ; vide = sa propre chaîne. Faux si la saisie est invalide.
    pub fn set_channel(self: &Arc<Self>, input: Option<&str>) -> bool {
        let raw = input.unwrap_or("").trim();
        let mut login = None;
        if !raw.is_empty() {
            match parse_channel(raw) {
                Some(l) => login = Some(l),
                None => {
                    self.patch(|st| st.error = Some(twitch_error(ErrorCode::InvalidChannel, None)));
                    return false;
                }
            }
        }
        let session = self.auth.session();
        // Saisir son propre pseudo revient au réglage par défaut.
        if login.is_some() && login.as_deref() == session.as_ref().map(|s| s.account.login.as_str()) {
            login = None;
        }
        self.auth.store().save_channel_login(login.as_deref());
        self.patch(|st| {
            st.channel_login = login.clone();
            st.error = without_channel_error(&st.error);
        });
        if let Some(session) = session {
            let me = self.clone();
            self.rt.spawn("twitch-chat-start", move || me.start_chat(session));
        }
        true
    }

    /// Faux message dans OBS pour vérifier la source.
    pub fn send_test_message(&self) {
        let n = {
            let mut st = self.lock();
            st.test_counter += 1;
            st.test_counter
        };
        let now = crate::util::now_ms();
        let name = self.state().account.map(|a| a.display_name).unwrap_or_else(|| "Batlay".into());
        let text = "Test réussi : le chat Twitch s'affichera ici en temps réel.";
        let message = obj! {
            "id" => format!("batlay-test-{now}-{n}"), "userId" => "batlay-test", "login" => "batlay", "displayName" => name, "color" => "#9b6bff",
            "badges" => Value::Array(Vec::new()), "fragments" => Value::Array(vec![obj! { "type" => "text", "text" => text }]), "text" => text,
            "timestamp" => now, "highlight" => "none", "replyTo" => Value::Null,
        };
        self.server.broadcast(&obj! { "type" => "chat", "message" => message }, false);
    }

    /// Message d'une autre plateforme (chat TikTok) dans les overlays du chat (`src=` pour filtrer).
    pub fn inject_chat(&self, message: Value) {
        self.server.broadcast(&obj! { "type" => "chat", "message" => message }, true);
    }

    pub fn restart_overlay(&self) -> usize {
        self.server.reload_clients()
    }

    pub fn stop(&self) {
        let timer = {
            let mut st = self.lock();
            st.chat_generation += 1;
            st.channel_retry.take()
        };
        self.rt.cancel(timer);
        self.auth.dispose();
        self.chat.stop();
        self.server.stop();
    }

    fn on_logged_out(&self) {
        let timer = {
            let mut st = self.lock();
            st.chat_generation += 1;
            st.channel_retry.take()
        };
        self.rt.cancel(timer);
        self.chat.stop();
        self.server.reset();
        self.patch(|st| {
            st.channel = None;
            st.alert_follows = false;
            st.alert_subscriptions = false;
        });
    }

    /// (Re)démarre la lecture sur la chaîne choisie. En cas d'échec le chat reste éteint :
    /// jamais de repli silencieux sur une autre chaîne que celle choisie.
    fn start_chat(self: &Arc<Self>, session: Session) {
        let (generation, old_timer) = {
            let mut st = self.lock();
            st.chat_generation += 1;
            (st.chat_generation, st.channel_retry.take())
        };
        self.rt.cancel(old_timer);
        self.chat.stop();
        self.server.reset();
        let login = self.state().channel_login;
        let mut channel: TwitchAccount = session.account.clone();
        if let Some(login) = login.filter(|l| *l != session.account.login) {
            self.patch(|st| {
                st.channel = None;
                st.chat = ChatStatus::Connecting;
            });
            let found = fetch_channel_by_login(self.http.as_ref(), &session.token, &login);
            if self.lock().chat_generation != generation || self.auth.token().as_deref() != Some(session.token.as_str()) {
                return; // choix changé entre-temps
            }
            match found {
                ChannelLookup::Found(account) => channel = account,
                ChannelLookup::Unauthorized => {
                    self.auth.expire(twitch_error(ErrorCode::TokenExpired, None));
                    return;
                }
                ChannelLookup::NotFound => {
                    self.patch(|st| {
                        st.chat = ChatStatus::Off;
                        st.error = Some(twitch_error(ErrorCode::ChannelNotFound, Some(&login)));
                    });
                    return;
                }
                ChannelLookup::Network => {
                    self.patch(|st| {
                        st.chat = ChatStatus::Off;
                        st.error = Some(twitch_error(ErrorCode::Network, None));
                    });
                    let weak = self.me.clone();
                    let mut st = self.lock();
                    st.channel_retry = Some(self.rt.after(CHANNEL_RETRY_MS, move || {
                        if let Some(me) = weak.upgrade() {
                            let ok = {
                                let mut st = me.lock();
                                st.channel_retry = None;
                                st.chat_generation == generation
                            };
                            if ok {
                                let m2 = me.clone();
                                me.rt.spawn("twitch-chat-start", move || m2.start_chat(session));
                            }
                        }
                    }));
                    return;
                }
            }
        }
        let channel_id = channel.id.clone();
        self.patch(|st| {
            st.channel = Some(channel);
            st.error = without_channel_error(&st.error);
        });
        self.chat.start(&session.account.id, &channel_id);
        let badges = fetch_badge_map(self.http.as_ref(), &session.token, &channel_id);
        if self.lock().chat_generation == generation {
            self.server.broadcast(&obj! { "type" => "badges", "badges" => badges }, true);
        }
    }
}

/// L'erreur courante, sauf si elle concerne le choix de chaîne.
fn without_channel_error(error: &Option<TwitchError>) -> Option<TwitchError> {
    match error {
        Some(e) if matches!(e.code, ErrorCode::InvalidChannel | ErrorCode::ChannelNotFound) => None,
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch::chat::tests::{wait_until, welcome, FakeHttp, FakeWs};
    use crate::twitch::model::AuthStatus;
    use crate::runtime::{ManualClock, ManualTimers, ThreadSpawner};

    struct Reverse;
    impl crate::config::SecretBox for Reverse {
        fn protect(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.iter().rev().copied().collect())
        }
        fn unprotect(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.iter().rev().copied().collect())
        }
    }

    #[derive(Default)]
    struct Host {
        states: Mutex<Vec<TwitchState>>,
        alerts: Mutex<Vec<Value>>,
        messages: Mutex<Vec<Value>>,
        opened: Mutex<Vec<String>>,
    }
    impl TwitchHost for Host {
        fn state_changed(&self, s: &TwitchState) {
            self.states.lock().unwrap().push(s.clone());
        }
        fn alert(&self, p: Value) {
            self.alerts.lock().unwrap().push(p);
        }
        fn overlay_message(&self, m: &Value) {
            self.messages.lock().unwrap().push(m.clone());
        }
        fn open_external(&self, url: &str) -> Result<(), String> {
            self.opened.lock().unwrap().push(url.into());
            Ok(())
        }
    }

    #[test]
    fn login_starts_chat_on_chosen_channel_and_alerts_flow() {
        let dir = std::env::temp_dir().join(format!("batlay-twsvc-{}", crate::util::random::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let http = Arc::new(FakeHttp::default());
        http.route("oauth2/validate", 200, r#"{"client_id":"vog5xlub7gikyjfoseohp2uo33di6x","login":"adil","user_id":"42","scopes":["user:read:chat","moderator:read:followers","channel:read:subscriptions"],"expires_in":5000}"#);
        http.route("helix/users?id=42", 200, r#"{"data":[{"display_name":"Adil"}]}"#);
        let ws = Arc::new(FakeWs::default());
        let host = Arc::new(Host::default());
        let rt = Rt { clock: ManualClock::new(1), timers: ManualTimers::new(), spawner: Arc::new(ThreadSpawner) };
        let svc = TwitchService::new(TwitchConfig { port: 0, static_dir: dir.clone(), session_file: dir.join("batlay-twitch.json") }, http.clone(), ws.clone(), Arc::new(Reverse), host.clone(), rt);
        svc.start();
        wait_until("serveur démarré", || svc.state().server_running);
        svc.connect();
        assert_eq!(svc.state().status, AuthStatus::Authorizing);
        let url = host.opened.lock().unwrap()[0].clone();
        let state = url.split("state=").nth(1).unwrap().split('&').next().unwrap().to_string();
        let (ok, _) = svc.auth.handle_callback(CallbackPayload::Token { access_token: "abcdefghijklmnop0123".into(), scopes: vec![], state });
        assert!(ok);
        wait_until("socket ouverte", || ws.count() == 1);
        ws.feed(0, welcome("S1"));
        wait_until("chat en direct", || svc.state().chat == ChatStatus::Live);
        let st = svc.state();
        assert_eq!((st.status, st.alert_follows, st.channel.unwrap().login), (AuthStatus::Connected, true, "adil".to_string()));
        ws.feed(0, crate::twitch::chat::tests::Feed::Text(r#"{"metadata":{"message_type":"notification","message_id":"f1","subscription_type":"channel.follow"},"payload":{"event":{"user_name":"Lina"}}}"#.into()));
        wait_until("alerte reçue", || host.alerts.lock().unwrap().len() == 1);
        // Pseudo invalide : refusé, rien ne change.
        assert!(!svc.set_channel(Some("pas valable !")));
        assert_eq!(svc.state().error.unwrap().code, ErrorCode::InvalidChannel);
        // Chaîne d'un autre streamer.
        http.route("login=ninja", 200, r#"{"data":[{"id":"9","login":"ninja","display_name":"Ninja"}]}"#);
        assert!(svc.set_channel(Some("https://www.twitch.tv/ninja")));
        wait_until("nouvelle socket", || ws.count() == 2);
        ws.feed(1, welcome("S2"));
        wait_until("chat de ninja", || svc.state().chat == ChatStatus::Live && svc.state().channel.map(|c| c.login) == Some("ninja".into()));
        assert!(svc.state().error.is_none(), "erreur de chaîne effacée");
        svc.disconnect();
        wait_until("déconnecté", || svc.state().status == AuthStatus::Disconnected && svc.state().chat == ChatStatus::Off);
        assert_eq!(svc.state().channel_login.as_deref(), Some("ninja"), "le choix de chaîne survit à la déconnexion");
        svc.stop();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
