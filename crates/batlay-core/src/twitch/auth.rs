//! Session Twitch : connexion (OAuth Implicit), contrôle du jeton au démarrage
//! puis toutes les heures (exigé par Twitch), déconnexion (révocation +
//! effacement) et expiration. Aucun secret : le flux Implicit n'en a pas.

use super::eventsub::HELIX_USERS_URL;
use super::model::{alert_scopes_from, twitch_error, AuthStatus, ErrorCode, TwitchAccount, TwitchError, TWITCH_AUTH_TIMEOUT_MS, TWITCH_CLIENT_ID};
use super::oauth::{authorize_url, generate_state, interpret_validation, oauth_error, states_match, CallbackPayload, TokenInfo, Validation};
use crate::config::SecretBox;
use crate::json::Value;
use crate::net::{Http, HttpRequest};
use crate::obj;
use crate::runtime::{Rt, TimerId};
use crate::util::encoding::{base64_decode, base64_encode};
use crate::util::url::encode_component;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

const HOUR_MS: u64 = 3_600_000;
const RESTORE_RETRY_MS: u64 = 30_000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub token: String,
    pub account: TwitchAccount,
    pub expires_at: Option<u64>,
    pub scopes: Vec<String>,
}

/// Changement d'état publié par l'authentification (champs `Some` = à modifier).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AuthPatch {
    pub status: Option<AuthStatus>,
    pub account: Option<Option<TwitchAccount>>,
    pub token_expires_at: Option<Option<u64>>,
    pub error: Option<Option<TwitchError>>,
    pub warning: Option<Option<TwitchError>>,
    pub alert_scopes: Option<(bool, bool)>,
}

pub trait AuthEvents: Send + Sync {
    fn patch(&self, patch: AuthPatch);
    /// Une session valide vient de s'ouvrir (connexion ou restauration).
    fn logged_in(&self, session: Session);
    /// La session est terminée (déconnexion, expiration).
    fn logged_out(&self);
    fn open_external(&self, url: &str) -> Result<(), String>;
}

/// Session mémorisée : `batlay-twitch.json` (fichier séparé : le tableau de bord
/// ne le voit jamais), jeton chiffré par DPAPI. Sans chiffrement : rien n'est
/// écrit, la session vit le temps de ce lancement.
pub struct TokenStore {
    path: PathBuf,
    secrets: Arc<dyn SecretBox>,
}

impl TokenStore {
    pub fn new(path: impl Into<PathBuf>, secrets: Arc<dyn SecretBox>) -> TokenStore {
        TokenStore { path: path.into(), secrets }
    }

    fn read(&self) -> Value {
        std::fs::read(&self.path).ok().and_then(|b| crate::json::parse_bytes(&b).ok()).filter(Value::is_object).unwrap_or_else(Value::object)
    }

    fn write(&self, v: &Value) {
        if let Err(e) = crate::config::write_atomic(&self.path, v.to_json_pretty().as_bytes()) {
            crate::log::warn(&format!("[Twitch] session non enregistrée : {e}"));
        }
    }

    /// `false` = la session n'a PAS été mémorisée (chiffrement indisponible).
    pub fn save(&self, s: &Session) -> bool {
        let mut v = self.read();
        let Some(protected) = self.secrets.protect(s.token.as_bytes()) else {
            self.clear();
            return false;
        };
        v.set("tokenDpapi", base64_encode(&protected));
        // Jeton chiffré par Electron (0.4.x) : illisible ici, retiré.
        v.set("encryptedToken", Value::Null);
        v.set("account", s.account.to_json());
        v.set("expiresAt", s.expires_at);
        self.write(&v);
        true
    }

    pub fn load(&self) -> Option<(String, TwitchAccount, Option<u64>)> {
        let v = self.read();
        let blob = base64_decode(v.get("tokenDpapi").as_str()?)?;
        let account = TwitchAccount::from_json(v.get("account"))?;
        let token = String::from_utf8(self.secrets.unprotect(&blob)?).ok().filter(|t| !t.is_empty())?;
        Some((token, account, v.get("expiresAt").as_u64()))
    }

    pub fn clear(&self) {
        let mut v = self.read();
        for k in ["tokenDpapi", "encryptedToken", "account", "expiresAt"] {
            v.set(k, Value::Null);
        }
        self.write(&v);
    }

    /// Chaîne choisie (donnée non sensible), revalidée à la lecture.
    pub fn channel_login(&self) -> Option<String> {
        self.read().get("channelLogin").as_str().and_then(super::model::parse_channel)
    }

    pub fn save_channel_login(&self, login: Option<&str>) {
        let mut v = self.read();
        v.set("channelLogin", login);
        self.write(&v);
    }
}

struct Pending {
    state: String,
    timer: TimerId,
    keep_session: bool,
}

#[derive(Default)]
struct State {
    session: Option<Session>,
    pending: Option<Pending>,
    revalidate_timer: Option<TimerId>,
    restore_timer: Option<TimerId>,
}

pub struct TwitchAuth {
    http: Arc<dyn Http>,
    rt: Rt,
    store: TokenStore,
    events: Arc<dyn AuthEvents>,
    state: Mutex<State>,
}

impl TwitchAuth {
    pub fn new(http: Arc<dyn Http>, rt: Rt, store: TokenStore, events: Arc<dyn AuthEvents>) -> Arc<TwitchAuth> {
        Arc::new(TwitchAuth { http, rt, store, events, state: Mutex::new(State::default()) })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn store(&self) -> &TokenStore {
        &self.store
    }

    pub fn token(&self) -> Option<String> {
        self.lock().session.as_ref().map(|s| s.token.clone())
    }

    pub fn session(&self) -> Option<Session> {
        self.lock().session.clone()
    }

    pub fn has_pending(&self) -> bool {
        self.lock().pending.is_some()
    }

    /// Ouvre l'autorisation Twitch. `keep_session` : déjà connecté, on redemande
    /// les permissions d'alertes ; la session en cours continue jusqu'à la réponse.
    pub fn begin(self: &Arc<Self>, keep_session: bool) {
        let state = generate_state();
        let upgrading = {
            let mut st = self.lock();
            if st.session.is_some() && !keep_session {
                return;
            }
            let upgrading = keep_session && st.session.is_some();
            let restore = st.restore_timer.take();
            let old = st.pending.take().map(|p| p.timer);
            self.rt.cancel(restore);
            self.rt.cancel(old);
            let weak = Arc::downgrade(self);
            let expected = state.clone();
            let timer = self.rt.after(TWITCH_AUTH_TIMEOUT_MS, move || {
                if let Some(me) = weak.upgrade() {
                    let expired = {
                        let mut st = me.lock();
                        match &st.pending {
                            Some(p) if p.state == expected => {
                                st.pending = None;
                                true
                            }
                            _ => false,
                        }
                    };
                    if expired {
                        me.fail(upgrading, twitch_error(ErrorCode::AuthorizationTimeout, None));
                    }
                }
            });
            st.pending = Some(Pending { state: state.clone(), timer, keep_session: upgrading });
            upgrading
        };
        if !upgrading {
            self.events.patch(AuthPatch { status: Some(AuthStatus::Authorizing), error: Some(None), warning: Some(None), ..Default::default() });
        }
        if let Err(e) = self.events.open_external(&authorize_url(&state)) {
            self.clear_pending();
            self.fail(upgrading, twitch_error(ErrorCode::OauthError, Some(&format!("impossible d'ouvrir le navigateur : {e}"))));
        }
    }

    /// Échec d'une autorisation : déconnecté, sauf si une session était active (elle est gardée).
    fn fail(&self, upgrading: bool, error: TwitchError) {
        let has_session = self.lock().session.is_some();
        let status = if upgrading && has_session { AuthStatus::Connected } else { AuthStatus::Disconnected };
        self.events.patch(AuthPatch { status: Some(status), error: Some(Some(error)), ..Default::default() });
    }

    fn clear_pending(&self) {
        let timer = self.lock().pending.take().map(|p| p.timer);
        self.rt.cancel(timer);
    }

    pub fn cancel(&self) {
        if !self.has_pending() {
            return;
        }
        self.clear_pending();
        self.events.patch(AuthPatch { status: Some(AuthStatus::Disconnected), error: Some(None), ..Default::default() });
    }

    /// Retour de Twitch relayé par la page locale. Renvoie (ok, message affiché dans le navigateur).
    pub fn handle_callback(self: &Arc<Self>, payload: CallbackPayload) -> (bool, String) {
        let pending_info = {
            let st = self.lock();
            st.pending.as_ref().map(|p| (p.state.clone(), p.keep_session))
        };
        let Some((expected, keep)) = pending_info else {
            return (false, "Aucune connexion Twitch n'est en attente. Relancez la connexion depuis Batlay.".into());
        };
        // Un `state` erroné (ancien onglet, requête forgée) est ignoré SANS annuler la connexion en cours.
        if !states_match(&expected, payload.state()) {
            crate::log::warn("[Twitch] callback ignoré : state invalide");
            return (false, twitch_error(ErrorCode::InvalidState, None).message);
        }
        self.clear_pending();
        let upgrading = keep && self.lock().session.is_some();
        let (token, _scopes) = match payload {
            CallbackPayload::Error { error, description, .. } => {
                let e = oauth_error(&error, &description);
                self.fail(upgrading, e.clone());
                return (false, e.message);
            }
            CallbackPayload::Token { access_token, scopes, .. } => (access_token, scopes),
        };
        if !upgrading {
            self.events.patch(AuthPatch { status: Some(AuthStatus::Validating), error: Some(None), ..Default::default() });
        }
        let info = match self.validate(&token) {
            Validation::Ok(info) => info,
            Validation::Failed { error, .. } => {
                self.fail(upgrading, error.clone());
                return (false, error.message);
            }
        };
        let account = self.fetch_account(&token, &info);
        let display = account.display_name.clone();
        let session = Session { token: token.clone(), account, expires_at: (info.expires_in > 0).then(|| self.rt.now() + info.expires_in * 1000), scopes: info.scopes };
        let previous = self.lock().session.as_ref().map(|s| s.token.clone());
        self.open(session, true);
        // Nouvelle autorisation par-dessus l'ancienne : l'ancien jeton est révoqué (au mieux).
        if let Some(prev) = previous.filter(|p| *p != token) {
            let me = self.clone();
            self.rt.spawn("twitch-revoke", move || me.revoke(&prev));
        }
        (true, format!("Compte {display} connecté. Vous pouvez fermer cet onglet."))
    }

    /// Restauration au démarrage (sans navigateur).
    pub fn restore(self: &Arc<Self>) {
        let Some((token, account, expires_at)) = self.store.load() else { return };
        self.events.patch(AuthPatch { status: Some(AuthStatus::Validating), account: Some(Some(account.clone())), token_expires_at: Some(expires_at), error: Some(None), ..Default::default() });
        self.restore_with(token, account);
    }

    fn restore_with(self: &Arc<Self>, token: String, account: TwitchAccount) {
        match self.validate(&token) {
            Validation::Ok(info) => {
                let session = Session { token, account, expires_at: (info.expires_in > 0).then(|| self.rt.now() + info.expires_in * 1000), scopes: info.scopes };
                self.open(session, true);
            }
            Validation::Failed { unauthorized: true, .. } => {
                self.store.clear();
                self.events.patch(AuthPatch {
                    status: Some(AuthStatus::Disconnected),
                    account: Some(None),
                    token_expires_at: Some(None),
                    error: Some(Some(twitch_error(ErrorCode::TokenExpired, None))),
                    ..Default::default()
                });
            }
            Validation::Failed { error, .. } => {
                // Réseau indisponible : on GARDE le jeton et on réessaie.
                self.events.patch(AuthPatch { status: Some(AuthStatus::Disconnected), account: Some(None), token_expires_at: Some(None), error: Some(Some(error)), ..Default::default() });
                let weak = Arc::downgrade(self);
                let mut st = self.lock();
                st.restore_timer = Some(self.rt.after(RESTORE_RETRY_MS, move || {
                    if let Some(me) = weak.upgrade() {
                        let go = {
                            let mut st = me.lock();
                            st.restore_timer = None;
                            st.session.is_none() && st.pending.is_none()
                        };
                        if go {
                            me.events.patch(AuthPatch { status: Some(AuthStatus::Validating), error: Some(None), ..Default::default() });
                            let m2 = me.clone();
                            me.rt.spawn("twitch-restore", move || m2.restore_with(token, account));
                        }
                    }
                }));
            }
        }
    }

    fn open(self: &Arc<Self>, session: Session, persist: bool) {
        let mut warning = None;
        if persist && !self.store.save(&session) {
            warning = Some(twitch_error(ErrorCode::StorageUnavailable, None));
        }
        self.lock().session = Some(session.clone());
        self.events.patch(AuthPatch {
            status: Some(AuthStatus::Connected),
            account: Some(Some(session.account.clone())),
            token_expires_at: Some(session.expires_at),
            error: Some(None),
            warning: Some(warning),
            alert_scopes: Some(alert_scopes_from(&session.scopes)),
        });
        self.start_revalidation();
        self.events.logged_in(session);
    }

    /// Twitch exige `/validate` au moins toutes les heures.
    fn start_revalidation(self: &Arc<Self>) {
        let weak = Arc::downgrade(self);
        let mut st = self.lock();
        let old = st.revalidate_timer.take();
        self.rt.cancel(old);
        st.revalidate_timer = Some(self.rt.after(HOUR_MS, move || {
            if let Some(me) = weak.upgrade() {
                let m2 = me.clone();
                me.rt.spawn("twitch-validate", move || m2.revalidate());
            }
        }));
    }

    pub fn revalidate(self: &Arc<Self>) {
        let Some(session) = self.session() else { return };
        let result = self.validate(&session.token);
        if self.token().as_deref() != Some(session.token.as_str()) {
            return; // déconnecté pendant la requête
        }
        match result {
            Validation::Ok(info) => {
                let expires_at = (info.expires_in > 0).then(|| self.rt.now() + info.expires_in * 1000);
                if let Some(s) = self.lock().session.as_mut() {
                    s.expires_at = expires_at;
                }
                self.events.patch(AuthPatch { token_expires_at: Some(expires_at), ..Default::default() });
                self.start_revalidation();
            }
            Validation::Failed { unauthorized: true, .. } => self.expire(twitch_error(ErrorCode::TokenExpired, None)),
            Validation::Failed { error, .. } => {
                crate::log::warn(&format!("[Twitch] contrôle du jeton impossible ({}) : nouvel essai au prochain cycle", error.message));
                self.start_revalidation();
            }
        }
    }

    /// Session perdue côté Twitch : on efface et on demande de se reconnecter.
    pub fn expire(&self, error: TwitchError) {
        let timer = {
            let mut st = self.lock();
            st.session = None;
            st.revalidate_timer.take()
        };
        self.rt.cancel(timer);
        self.store.clear();
        self.events.patch(AuthPatch { status: Some(AuthStatus::Disconnected), account: Some(None), token_expires_at: Some(None), error: Some(Some(error)), ..Default::default() });
        self.events.logged_out();
    }

    pub fn disconnect(&self) {
        let (session, timers) = {
            let mut st = self.lock();
            let timers = [st.pending.take().map(|p| p.timer), st.restore_timer.take(), st.revalidate_timer.take()];
            (st.session.take(), timers)
        };
        for t in timers {
            self.rt.cancel(t);
        }
        let token = session.map(|s| s.token).or_else(|| self.store.load().map(|(t, _, _)| t));
        self.store.clear();
        self.events.patch(AuthPatch {
            status: Some(AuthStatus::Disconnected),
            account: Some(None),
            token_expires_at: Some(None),
            error: Some(None),
            warning: Some(None),
            ..Default::default()
        });
        self.events.logged_out();
        // Révocation côté Twitch : au mieux, l'utilisateur est déjà déconnecté localement.
        if let Some(t) = token {
            self.revoke(&t);
        }
    }

    pub fn dispose(&self) {
        let timers = {
            let mut st = self.lock();
            [st.pending.take().map(|p| p.timer), st.restore_timer.take(), st.revalidate_timer.take()]
        };
        for t in timers {
            self.rt.cancel(t);
        }
    }

    fn validate(&self, token: &str) -> Validation {
        let request = HttpRequest::get("https://id.twitch.tv/oauth2/validate").header("Authorization", format!("OAuth {token}")).timeout(REQUEST_TIMEOUT);
        match self.http.send(&request) {
            Ok(r) => interpret_validation(r.status, &r.json().unwrap_or(Value::Null)),
            Err(e) => Validation::Failed { unauthorized: false, error: twitch_error(ErrorCode::Network, Some(&e.to_string())) },
        }
    }

    fn fetch_account(&self, token: &str, info: &TokenInfo) -> TwitchAccount {
        let fallback = TwitchAccount { id: info.user_id.clone(), login: info.login.clone(), display_name: info.login.clone(), profile_image_url: None };
        let request = HttpRequest::get(format!("{HELIX_USERS_URL}?id={}", encode_component(&info.user_id)))
            .header("Authorization", format!("Bearer {token}"))
            .header("Client-Id", TWITCH_CLIENT_ID)
            .timeout(REQUEST_TIMEOUT);
        match self.http.send(&request) {
            Ok(r) if r.ok() => {
                let body = r.json().unwrap_or(Value::Null);
                let user = body.get("data").at(0);
                TwitchAccount {
                    display_name: user.get("display_name").non_empty_str().map(str::to_string).unwrap_or(fallback.display_name.clone()),
                    profile_image_url: user.get("profile_image_url").as_str().filter(|u| u.starts_with("https://")).map(str::to_string),
                    ..fallback
                }
            }
            _ => fallback, // le profil est cosmétique : ne bloque jamais la connexion
        }
    }

    fn revoke(&self, token: &str) {
        let request = HttpRequest::post_form("https://id.twitch.tv/oauth2/revoke", &[("client_id", TWITCH_CLIENT_ID), ("token", token)]).timeout(REQUEST_TIMEOUT);
        if let Err(e) = self.http.send(&request) {
            crate::log::warn(&format!("[Twitch] révocation du jeton impossible : {e}"));
        }
    }
}

/// Recherche d'une chaîne par pseudo (GET /helix/users?login=).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelLookup {
    Found(TwitchAccount),
    NotFound,
    Unauthorized,
    Network,
}

pub fn fetch_channel_by_login(http: &dyn Http, token: &str, login: &str) -> ChannelLookup {
    let request = HttpRequest::get(format!("{HELIX_USERS_URL}?login={}", encode_component(login)))
        .header("Authorization", format!("Bearer {token}"))
        .header("Client-Id", TWITCH_CLIENT_ID)
        .timeout(REQUEST_TIMEOUT);
    match http.send(&request) {
        Ok(r) if r.status == 401 => ChannelLookup::Unauthorized,
        Ok(r) if r.status == 400 => ChannelLookup::NotFound,
        Ok(r) if !r.ok() => ChannelLookup::Network,
        Ok(r) => {
            let body = r.json().unwrap_or(Value::Null);
            let user = body.get("data").at(0);
            match (user.get("id").as_str(), user.get("login").as_str()) {
                (Some(id), Some(l)) => ChannelLookup::Found(TwitchAccount {
                    id: id.into(),
                    login: l.into(),
                    display_name: user.get("display_name").non_empty_str().unwrap_or(l).into(),
                    profile_image_url: user.get("profile_image_url").as_str().filter(|u| u.starts_with("https://")).map(str::to_string),
                }),
                _ => ChannelLookup::NotFound,
            }
        }
        Err(_) => ChannelLookup::Network,
    }
}

/// Images des badges, globaux + chaîne (ceux de la chaîne priment). Décoratif : erreur = table vide.
pub fn fetch_badge_map(http: &dyn Http, token: &str, broadcaster_id: &str) -> Value {
    let get = |url: String| -> Value {
        let request = HttpRequest::get(url).header("Authorization", format!("Bearer {token}")).header("Client-Id", TWITCH_CLIENT_ID).timeout(REQUEST_TIMEOUT);
        match http.send(&request) {
            Ok(r) if r.ok() => super::eventsub::parse_badge_sets(&r.json().unwrap_or(Value::Null)),
            _ => Value::object(),
        }
    };
    let mut all = get(super::eventsub::HELIX_GLOBAL_BADGES_URL.to_string());
    let channel = get(format!("{}?broadcaster_id={}", super::eventsub::HELIX_CHANNEL_BADGES_URL, encode_component(broadcaster_id)));
    if let (Some(a), Some(c)) = (all.as_object_mut(), channel.as_object()) {
        a.merge(c);
    }
    all
}

/// Vue JSON de la session (pour les tests et le diagnostic, jamais le jeton).
pub fn session_summary(s: &Session) -> Value {
    obj! { "account" => s.account.to_json(), "expiresAt" => s.expires_at, "scopes" => Value::from(s.scopes.clone()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch::chat::tests::FakeHttp;
    use crate::runtime::test_rt;

    /// « Chiffrement » de test : inversion des octets (jamais en production).
    pub struct Reverse;
    impl SecretBox for Reverse {
        fn protect(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.iter().rev().copied().collect())
        }
        fn unprotect(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.iter().rev().copied().collect())
        }
    }

    #[derive(Default)]
    struct Events {
        patches: Mutex<Vec<AuthPatch>>,
        logins: Mutex<Vec<Session>>,
        logouts: Mutex<u32>,
        opened: Mutex<Vec<String>>,
    }
    impl AuthEvents for Events {
        fn patch(&self, p: AuthPatch) {
            self.patches.lock().unwrap().push(p);
        }
        fn logged_in(&self, s: Session) {
            self.logins.lock().unwrap().push(s);
        }
        fn logged_out(&self) {
            *self.logouts.lock().unwrap() += 1;
        }
        fn open_external(&self, url: &str) -> Result<(), String> {
            self.opened.lock().unwrap().push(url.to_string());
            Ok(())
        }
    }

    const VALID: &str = r#"{"client_id":"vog5xlub7gikyjfoseohp2uo33di6x","login":"adil","user_id":"42","scopes":["user:read:chat","moderator:read:followers"],"expires_in":5000}"#;

    fn setup(secrets: Arc<dyn SecretBox>) -> (Arc<TwitchAuth>, Arc<FakeHttp>, Arc<Events>, Arc<crate::runtime::ManualTimers>, PathBuf) {
        let (rt, _clock, timers) = test_rt(1_000_000);
        let http = Arc::new(FakeHttp::default());
        http.route("oauth2/validate", 200, VALID);
        http.route("helix/users?id=42", 200, r#"{"data":[{"display_name":"Adil ✨","profile_image_url":"https://x/y.png"}]}"#);
        let events = Arc::new(Events::default());
        let path = std::env::temp_dir().join(format!("batlay-twitch-{}.json", crate::util::random::random_hex(4)));
        let auth = TwitchAuth::new(http.clone(), rt, TokenStore::new(&path, secrets), events.clone());
        (auth, http, events, timers, path)
    }

    fn state_of(url: &str) -> String {
        url.split("state=").nth(1).unwrap().split('&').next().unwrap().to_string()
    }

    #[test]
    fn full_login_then_restore_then_disconnect() {
        let (auth, http, events, _timers, path) = setup(Arc::new(Reverse));
        auth.begin(false);
        let url = events.opened.lock().unwrap()[0].clone();
        let state = state_of(&url);
        // Mauvais state : ignoré, la connexion reste en attente.
        let (ok, _) = auth.handle_callback(CallbackPayload::Token { access_token: "abcdefghijklmnop0123".into(), scopes: vec![], state: "zzzzzzzzzz".into() });
        assert!(!ok && auth.has_pending());
        let (ok, message) = auth.handle_callback(CallbackPayload::Token { access_token: "abcdefghijklmnop0123".into(), scopes: vec![], state });
        assert!(ok, "{message}");
        assert_eq!(message, "Compte Adil ✨ connecté. Vous pouvez fermer cet onglet.");
        let session = events.logins.lock().unwrap()[0].clone();
        assert_eq!((session.account.display_name.as_str(), session.expires_at), ("Adil ✨", Some(1_000_000 + 5_000_000)));
        let last = events.patches.lock().unwrap().last().cloned().unwrap();
        assert_eq!((last.status, last.alert_scopes), (Some(AuthStatus::Connected), Some((true, false))));
        // Le fichier ne contient jamais le jeton en clair.
        let file = std::fs::read_to_string(&path).unwrap();
        assert!(!file.contains("abcdefghijklmnop0123") && file.contains("tokenDpapi"));
        // Relance de Batlay : session restaurée sans navigateur.
        let (rt2, _c, _t) = test_rt(2_000_000);
        let events2 = Arc::new(Events::default());
        let auth2 = TwitchAuth::new(http.clone(), rt2, TokenStore::new(&path, Arc::new(Reverse)), events2.clone());
        auth2.restore();
        assert_eq!(auth2.token().as_deref(), Some("abcdefghijklmnop0123"));
        auth2.disconnect();
        assert_eq!(http.bodies_matching("token=abcdefghijklmnop0123"), 1, "jeton révoqué");
        assert!(TokenStore::new(&path, Arc::new(Reverse)).load().is_none());
        assert_eq!(*events2.logouts.lock().unwrap(), 1);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn denied_timeout_and_storage_unavailable() {
        let (auth, _http, events, timers, path) = setup(Arc::new(crate::config::NoSecretBox));
        auth.begin(false);
        let state = state_of(&events.opened.lock().unwrap()[0]);
        let (ok, msg) = auth.handle_callback(CallbackPayload::Error { error: "access_denied".into(), description: String::new(), state: Some(state) });
        assert!(!ok && msg.contains("Autorisation refusée"));
        auth.begin(false);
        assert!(timers.fire_delay(TWITCH_AUTH_TIMEOUT_MS));
        let last = events.patches.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.error.flatten().unwrap().code, ErrorCode::AuthorizationTimeout);
        auth.begin(false);
        let state = state_of(&events.opened.lock().unwrap()[2]);
        assert!(auth.handle_callback(CallbackPayload::Token { access_token: "abcdefghijklmnop0123".into(), scopes: vec![], state }).0);
        let last = events.patches.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.warning.flatten().unwrap().code, ErrorCode::StorageUnavailable, "sans chiffrement : prévenu, rien d'écrit");
        assert!(!path.exists() || !std::fs::read_to_string(&path).unwrap().contains("abcdefghijklmnop0123"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn restore_keeps_token_when_offline_and_clears_when_revoked() {
        let (auth, http, events, timers, path) = setup(Arc::new(Reverse));
        let account = TwitchAccount { id: "42".into(), login: "adil".into(), display_name: "Adil".into(), profile_image_url: None };
        auth.store().save(&Session { token: "abcdefghijklmnop0123".into(), account, expires_at: None, scopes: vec![] });
        http.route("oauth2/validate", 0, "réseau coupé");
        auth.restore();
        assert!(auth.token().is_none());
        assert_eq!(timers.pending(), vec![RESTORE_RETRY_MS]);
        assert!(auth.store().load().is_some(), "jeton gardé");
        http.route("oauth2/validate", 401, "{}");
        timers.fire_delay(RESTORE_RETRY_MS);
        assert!(auth.store().load().is_none(), "jeton révoqué : effacé");
        let last = events.patches.lock().unwrap().last().cloned().unwrap();
        assert_eq!(last.error.flatten().unwrap().code, ErrorCode::TokenExpired);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn channel_lookup_and_badges() {
        let http = FakeHttp::default();
        http.route("login=ninja", 200, r#"{"data":[{"id":"9","login":"ninja","display_name":"Ninja"}]}"#);
        http.route("login=nobody", 200, r#"{"data":[]}"#);
        http.route("login=expired", 401, "{}");
        assert!(matches!(fetch_channel_by_login(&http, "t", "ninja"), ChannelLookup::Found(a) if a.display_name == "Ninja"));
        assert_eq!(fetch_channel_by_login(&http, "t", "nobody"), ChannelLookup::NotFound);
        assert_eq!(fetch_channel_by_login(&http, "t", "expired"), ChannelLookup::Unauthorized);
        http.route("badges/global", 200, r#"{"data":[{"set_id":"vip","versions":[{"id":"1","image_url_2x":"https://static-cdn.jtvnw.net/g"}]}]}"#);
        http.route("broadcaster_id=9", 200, r#"{"data":[{"set_id":"vip","versions":[{"id":"1","image_url_2x":"https://static-cdn.jtvnw.net/c"}]}]}"#);
        assert_eq!(fetch_badge_map(&http, "t", "9").to_json(), r#"{"vip/1":"https://static-cdn.jtvnw.net/c"}"#);
    }
}
