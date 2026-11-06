//! Spotify : Authorization Code + PKCE (sans secret client), jeton de
//! rafraîchissement chiffré par DPAPI, morceau en cours.
//!
//! Configuration requise : une app sur developer.spotify.com avec l'URI de
//! redirection exacte `http://127.0.0.1:8945/callback`, et son Client ID
//! renseigné dans Batlay. Sans Client ID, la connexion est désactivée.

use crate::config::{SecretBox, Store};
use crate::json::Value;
use crate::net::http::{Request, Response};
use crate::net::server::{Handler, HttpServer};
use crate::net::{Http, HttpRequest};
use crate::obj;
use crate::runtime::Clock;
use crate::util::encoding::{base64_decode, base64_encode, base64url_encode};
use crate::util::hash::sha256;
use crate::util::random::random_bytes;
use crate::util::url::{build_query, query_get};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const CALLBACK_PORT: u16 = 8945;
pub const REDIRECT_URI: &str = "http://127.0.0.1:8945/callback";
const SCOPES: &str = "user-read-currently-playing user-read-playback-state";
const AUTH_TIMEOUT: Duration = Duration::from_secs(120);

pub fn code_verifier() -> String {
    base64url_encode(&random_bytes(64))
}

pub fn code_challenge(verifier: &str) -> String {
    base64url_encode(&sha256(verifier.as_bytes()))
}

pub fn authorize_url(client_id: &str, challenge: &str, state: &str) -> String {
    format!(
        "https://accounts.spotify.com/authorize?{}",
        build_query(&[("client_id", client_id), ("response_type", "code"), ("redirect_uri", REDIRECT_URI), ("code_challenge_method", "S256"), ("code_challenge", challenge), ("scope", SCOPES), ("state", state)])
    )
}

/// Réponse de /me/player/currently-playing → forme attendue par le tableau de bord.
/// `sampledAt` : MILIEU de l'aller-retour (Spotify mesure la progression en traitant la requête).
pub fn normalize_currently_playing(raw: &Value, requested_at: u64, received_at: u64) -> Value {
    let received = received_at.max(requested_at);
    obj! {
        "isPlaying" => raw.get("is_playing").as_bool().unwrap_or(false),
        "progressMs" => raw.get("progress_ms").as_f64().unwrap_or(0.0),
        "sampledAt" => requested_at + (received - requested_at).div_ceil(2),
        "item" => if raw.get("item").is_object() { raw.get("item").clone() } else { Value::Null },
    }
}

struct Tokens {
    access: Option<String>,
    expires_at: u64,
}

pub struct Spotify {
    store: Arc<Store>,
    secrets: Arc<dyn SecretBox>,
    http: Arc<dyn Http>,
    clock: Arc<dyn Clock>,
    open_external: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>,
    tokens: Mutex<Tokens>,
    /// Une seule connexion à la fois.
    connecting: Mutex<()>,
}

impl Spotify {
    pub fn new(store: Arc<Store>, secrets: Arc<dyn SecretBox>, http: Arc<dyn Http>, clock: Arc<dyn Clock>, open_external: Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>) -> Spotify {
        Spotify { store, secrets, http, clock, open_external, tokens: Mutex::new(Tokens { access: None, expires_at: 0 }), connecting: Mutex::new(()) }
    }

    pub fn client_id(&self) -> Option<String> {
        self.store.get("spotify").get("clientId").non_empty_str().map(|s| s.trim().to_string())
    }

    /// Client ID public (32 caractères hexadécimaux chez Spotify) : forme vérifiée.
    pub fn set_client_id(&self, client_id: &str) -> bool {
        let id = client_id.trim();
        if !id.is_empty() && (id.len() > 64 || !id.chars().all(|c| c.is_ascii_alphanumeric())) {
            return false;
        }
        self.store.update("spotify", |m| m.insert("clientId", if id.is_empty() { Value::Null } else { Value::from(id) }));
        true
    }

    pub fn is_configured(&self) -> bool {
        self.client_id().is_some()
    }

    fn save_refresh_token(&self, token: &str) -> Result<(), String> {
        let protected = self.secrets.protect(token.as_bytes()).ok_or_else(|| {
            "Le chiffrement sécurisé (DPAPI) n'est pas disponible sur ce système : la session Spotify ne peut pas être sauvegardée en toute sécurité.".to_string()
        })?;
        self.store.update("spotify", |m| {
            m.insert("refreshTokenDpapi", base64_encode(&protected));
            // Jeton chiffré par Electron (0.4.x) : illisible ici.
            m.remove("encryptedRefreshToken");
        });
        Ok(())
    }

    fn load_refresh_token(&self) -> Option<String> {
        let blob = base64_decode(self.store.get("spotify").get("refreshTokenDpapi").as_str()?)?;
        String::from_utf8(self.secrets.unprotect(&blob)?).ok().filter(|t| !t.is_empty())
    }

    fn clear(&self) {
        *self.tokens.lock().unwrap_or_else(|e| e.into_inner()) = Tokens { access: None, expires_at: 0 };
        self.store.update("spotify", |m| {
            m.remove("refreshTokenDpapi");
            m.remove("encryptedRefreshToken");
        });
    }

    pub fn is_session_restorable(&self) -> bool {
        self.load_refresh_token().is_some()
    }

    pub fn disconnect(&self) {
        self.clear();
    }

    /// Ouvre le navigateur, attend le retour (2 min max), échange le code, mémorise le jeton.
    pub fn connect(&self) -> Result<(), String> {
        let _guard = self.connecting.try_lock().map_err(|_| "Une connexion Spotify est déjà en cours.".to_string())?;
        let client_id = self.client_id().ok_or("Aucun Client ID Spotify configuré. Renseignez-le dans Connexions avant de vous connecter.")?;
        let verifier = code_verifier();
        let state = base64url_encode(&random_bytes(16));
        let code = self.wait_for_code(&authorize_url(&client_id, &code_challenge(&verifier), &state), &state)?;
        let tokens = self.token_request(&[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", REDIRECT_URI), ("client_id", &client_id), ("code_verifier", &verifier)])?;
        self.accept_tokens(&tokens)
    }

    fn wait_for_code(&self, auth_url: &str, expected_state: &str) -> Result<String, String> {
        let (tx, rx) = channel();
        let handler = Arc::new(CallbackHandler { tx: Mutex::new(Some(tx)), expected_state: expected_state.to_string() });
        let mut server = HttpServer::start(CALLBACK_PORT, handler).map_err(|e| {
            format!("Impossible de démarrer le serveur de retour local sur le port {CALLBACK_PORT} ({e}). Vérifiez qu'aucune autre application ne l'utilise.")
        })?;
        (self.open_external)(auth_url)?;
        let result = rx.recv_timeout(AUTH_TIMEOUT).unwrap_or_else(|_| Err("Délai d'attente dépassé pour l'autorisation Spotify.".into()));
        // Laisse le temps à la page de confirmation de partir.
        std::thread::sleep(Duration::from_millis(200));
        server.stop();
        result
    }

    fn token_request(&self, form: &[(&str, &str)]) -> Result<Value, String> {
        let r = self.http.send(&HttpRequest::post_form("https://accounts.spotify.com/api/token", form)).map_err(|e| format!("Impossible de joindre Spotify ({e})."))?;
        if !r.ok() {
            let detail = r.json().and_then(|j| j.get("error_description").as_str().or(j.get("error").as_str()).map(str::to_string)).unwrap_or_default();
            return Err(format!("Spotify a refusé la connexion ({}){}", r.status, if detail.is_empty() { ".".to_string() } else { format!(" : {detail}") }));
        }
        r.json().filter(|j| j.get("access_token").is_string()).ok_or_else(|| "Réponse de Spotify illisible.".to_string())
    }

    fn accept_tokens(&self, tokens: &Value) -> Result<(), String> {
        let access = tokens.get("access_token").as_str().ok_or("Réponse de Spotify illisible.")?.to_string();
        let expires_in = tokens.get("expires_in").as_f64().unwrap_or(3600.0).max(0.0) as u64;
        *self.tokens.lock().unwrap_or_else(|e| e.into_inner()) = Tokens { access: Some(access), expires_at: self.clock.now() + expires_in * 1000 };
        if let Some(refresh) = tokens.get("refresh_token").as_str() {
            self.save_refresh_token(refresh)?;
        }
        Ok(())
    }

    fn refresh(&self) -> Result<(), String> {
        let (Some(client_id), Some(refresh)) = (self.client_id(), self.load_refresh_token()) else {
            return Err("Aucune session Spotify à rafraîchir. Reconnectez-vous.".into());
        };
        let tokens = self.token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client_id)])?;
        self.accept_tokens(&tokens)
    }

    /// Session existante restaurée sans navigateur. Jeton refusé : effacé.
    pub fn restore_session(&self) -> bool {
        if !self.is_session_restorable() {
            return false;
        }
        match self.refresh() {
            Ok(()) => true,
            Err(_) => {
                self.clear();
                false
            }
        }
    }

    fn valid_access_token(&self) -> Result<String, String> {
        {
            let t = self.tokens.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(a) = &t.access {
                if self.clock.now() + 5000 < t.expires_at {
                    return Ok(a.clone());
                }
            }
        }
        self.refresh()?;
        self.tokens.lock().unwrap_or_else(|e| e.into_inner()).access.clone().ok_or_else(|| "Impossible d'obtenir un jeton Spotify valide.".to_string())
    }

    /// Morceau en cours, `null` si rien ne joue.
    pub fn currently_playing(&self) -> Result<Value, String> {
        let token = self.valid_access_token()?;
        let requested_at = self.clock.now();
        let r = self
            .http
            .send(&HttpRequest::get("https://api.spotify.com/v1/me/player/currently-playing").header("Authorization", format!("Bearer {token}")))
            .map_err(|e| format!("Impossible de joindre Spotify ({e})."))?;
        let received_at = self.clock.now();
        if r.status == 204 {
            return Ok(Value::Null);
        }
        if !r.ok() {
            // Spotify donne presque toujours une raison exploitable (« Premium required »…).
            let reason = r.json().and_then(|j| j.path(&["error", "message"]).as_str().map(str::to_string));
            let retry = (r.status == 429).then(|| r.headers.get("retry-after").map(str::to_string)).flatten();
            return Err(format!(
                "Échec de la récupération du morceau en cours ({}){}{}",
                r.status,
                reason.map(|m| format!(" : {m}")).unwrap_or_else(|| ".".into()),
                retry.map(|s| format!(" Nouvelle tentative possible dans {s}s.")).unwrap_or_default()
            ));
        }
        let raw = r.json().ok_or("Réponse de Spotify illisible.")?;
        Ok(normalize_currently_playing(&raw, requested_at, received_at))
    }
}

struct CallbackHandler {
    tx: Mutex<Option<Sender<Result<String, String>>>>,
    expected_state: String,
}

fn callback_result_page(ok: bool) -> String {
    if ok {
        crate::brand_page::status_page("Batlay · Spotify", "Spotify connecté", "Vous pouvez fermer cet onglet et revenir à Batlay.", true)
    } else {
        crate::brand_page::status_page("Batlay · Spotify", "Connexion Spotify échouée", "Vous pouvez fermer cet onglet et réessayer dans Batlay.", false)
    }
}

impl Handler for CallbackHandler {
    fn handle(&self, req: &Request) -> Response {
        if req.path() != "/callback" {
            return Response::new(404);
        }
        let q = req.query();
        let code = query_get(&q, "code").filter(|c| !c.is_empty() && c.len() < 2048);
        let state_ok = query_get(&q, "state").is_some_and(|s| crate::util::encoding::constant_time_eq(s.as_bytes(), self.expected_state.as_bytes()));
        let error = query_get(&q, "error");
        let result = match (code, state_ok, error) {
            (Some(code), true, None) => Ok(code.to_string()),
            (_, _, Some(e)) => Err(format!("Spotify a refusé l'autorisation ({}).", crate::util::text::clean_text(e, 100))),
            _ => Err("Réponse d'autorisation invalide (state incorrect).".into()),
        };
        let ok = result.is_ok();
        if let Some(tx) = self.tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(result);
        }
        Response::html(200, &callback_result_page(ok)).header("Cache-Control", "no-store")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::twitch::chat::tests::FakeHttp;

    struct Reverse;
    impl SecretBox for Reverse {
        fn protect(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.iter().rev().copied().collect())
        }
        fn unprotect(&self, d: &[u8]) -> Option<Vec<u8>> {
            Some(d.iter().rev().copied().collect())
        }
    }

    #[test]
    fn pkce_vector_from_rfc7636() {
        assert_eq!(code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(authorize_url("abc", "ch", "st").contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8945%2Fcallback"));
    }

    #[test]
    fn normalizes_and_samples_mid_flight() {
        let raw = crate::json::parse(r#"{"is_playing":true,"progress_ms":1234,"item":{"name":"Starboy","artists":[{"name":"The Weeknd"},{"name":"Daft Punk"}]}}"#).unwrap();
        let v = normalize_currently_playing(&raw, 1000, 1200);
        assert_eq!((v.get("isPlaying").as_bool(), v.get("progressMs").as_f64(), v.get("sampledAt").as_u64()), (Some(true), Some(1234.0), Some(1100)));
        assert_eq!(v.path(&["item", "artists"]).items().len(), 2, "tous les artistes transmis");
    }

    #[test]
    fn full_connect_flow_with_local_callback() {
        let http = Arc::new(FakeHttp::default());
        http.route("api/token", 200, r#"{"access_token":"AT1","expires_in":3600,"refresh_token":"RT1"}"#);
        http.route("currently-playing", 200, r#"{"is_playing":false,"progress_ms":5,"item":null}"#);
        let store = Arc::new(Store::memory(crate::config::default_config()));
        let opened: Arc<Mutex<Vec<String>>> = Arc::default();
        let o2 = opened.clone();
        let spotify = Arc::new(Spotify::new(store.clone(), Arc::new(Reverse), http.clone(), Arc::new(crate::runtime::SystemClock), Arc::new(move |u: &str| {
            o2.lock().unwrap().push(u.to_string());
            Ok(())
        })));
        assert!(spotify.connect().is_err(), "sans Client ID");
        assert!(!spotify.set_client_id("pas valable !"));
        assert!(spotify.set_client_id("0123456789abcdef0123456789abcdef"));
        let s2 = spotify.clone();
        let worker = std::thread::spawn(move || s2.connect());
        // Le « navigateur » : attend l'ouverture puis suit la redirection de Spotify.
        let url = loop {
            if let Some(u) = opened.lock().unwrap().first().cloned() {
                break u;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let state = url.split("state=").nth(1).unwrap().to_string();
        let client = crate::net::HttpClient::new(Arc::new(crate::net::PlainConnector), "navigateur");
        let page = client.send(&HttpRequest::get(format!("http://127.0.0.1:{CALLBACK_PORT}/callback?code=CODE42&state={state}"))).unwrap();
        assert!(page.text().contains("Spotify connecté"));
        worker.join().unwrap().unwrap();
        assert!(http.bodies_matching("code=CODE42") == 1 && http.bodies_matching("code_verifier=") == 1);
        assert!(spotify.is_session_restorable());
        assert!(!store.snapshot().to_json().contains("RT1"), "jeton de rafraîchissement jamais en clair");
        assert_eq!(spotify.currently_playing().unwrap().get("progressMs").as_f64(), Some(5.0));
        http.route("currently-playing", 403, r#"{"error":{"status":403,"message":"Premium required"}}"#);
        assert_eq!(spotify.currently_playing().unwrap_err(), "Échec de la récupération du morceau en cours (403) : Premium required");
        spotify.disconnect();
        assert!(!spotify.is_session_restorable());
    }
}
