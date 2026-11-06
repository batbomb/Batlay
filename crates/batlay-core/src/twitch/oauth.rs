//! OAuth Twitch (« Implicit grant », le seul flux navigateur de Twitch SANS
//! secret client) : adresse d'autorisation, validation du retour, contrôle du
//! jeton. Aucun effet de bord : c'est ici que se joue la sécurité du callback.

use super::model::{twitch_error, ErrorCode, TwitchError, TWITCH_ALERT_SCOPES, TWITCH_CLIENT_ID, TWITCH_REDIRECT_URI, TWITCH_SCOPES};
use crate::json::Value;
use crate::util::encoding::constant_time_eq;
use crate::util::url::build_query;

pub fn generate_state() -> String {
    crate::util::random::random_hex(24)
}

/// Chat (obligatoire) + alertes (facultatif). `force_verify` : permet de changer de compte.
pub fn authorize_url(state: &str) -> String {
    let scopes: Vec<&str> = TWITCH_SCOPES.iter().chain(TWITCH_ALERT_SCOPES.iter()).copied().collect();
    let scope = scopes.join(" ");
    format!(
        "https://id.twitch.tv/oauth2/authorize?{}",
        build_query(&[("response_type", "token"), ("client_id", TWITCH_CLIENT_ID), ("redirect_uri", TWITCH_REDIRECT_URI), ("scope", &scope), ("state", state), ("force_verify", "true")])
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackPayload {
    Token { access_token: String, scopes: Vec<String>, state: String },
    Error { error: String, description: String, state: Option<String> },
}

impl CallbackPayload {
    pub fn state(&self) -> Option<&str> {
        match self {
            CallbackPayload::Token { state, .. } => Some(state),
            CallbackPayload::Error { state, .. } => state.as_deref(),
        }
    }
}

fn str_max(v: &Value, max: usize) -> Option<&str> {
    v.as_str().filter(|s| s.chars().count() <= max)
}

fn charset_ok(s: &str, min: usize, max: usize) -> bool {
    (min..=max).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Corps du POST /auth/callback. `None` = rejeté (entrée non fiable, jamais de panique).
pub fn parse_callback(body: &Value) -> Option<CallbackPayload> {
    if !body.is_object() {
        return None;
    }
    let state = match body.get("state") {
        Value::Null => None,
        v => Some(str_max(v, 200)?.to_string()),
    };
    if state.as_deref().is_some_and(|s| !charset_ok(s, 8, 200)) {
        return None;
    }
    if let Some(token) = str_max(body.get("access_token"), 128) {
        if !charset_ok(token, 16, 128) {
            return None;
        }
        let state = state?;
        if let Some(t) = body.get("token_type").as_str() {
            if !t.eq_ignore_ascii_case("bearer") {
                return None;
            }
        } else if !body.get("token_type").is_null() {
            return None;
        }
        let scope_text = match body.get("scope") {
            Value::Null => "",
            v => str_max(v, 500)?,
        };
        let scopes = scope_text.split_whitespace().map(str::to_string).collect();
        return Some(CallbackPayload::Token { access_token: token.to_string(), scopes, state });
    }
    let error = str_max(body.get("error"), 100).filter(|e| !e.is_empty())?;
    let description = str_max(body.get("error_description"), 300).unwrap_or("");
    Some(CallbackPayload::Error { error: crate::util::text::replace_controls(error), description: crate::util::text::replace_controls(description).trim().to_string(), state })
}

/// Comparaison à temps constant (le `state` protège contre le CSRF de la redirection).
pub fn states_match(expected: &str, received: Option<&str>) -> bool {
    received.is_some_and(|r| constant_time_eq(expected.as_bytes(), r.as_bytes()))
}

pub fn oauth_error(error: &str, description: &str) -> TwitchError {
    if error == "access_denied" {
        return twitch_error(ErrorCode::AccessDenied, None);
    }
    let detail = if description.is_empty() { error.to_string() } else { format!("{error} : {description}") };
    twitch_error(ErrorCode::OauthError, Some(&detail))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenInfo {
    pub client_id: String,
    pub user_id: String,
    pub login: String,
    pub scopes: Vec<String>,
    /// Secondes restantes (0 = sans expiration annoncée).
    pub expires_in: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Validation {
    Ok(TokenInfo),
    /// `unauthorized` : le jeton ne sert plus (à effacer).
    Failed { unauthorized: bool, error: TwitchError },
}

/// Réponse de GET https://id.twitch.tv/oauth2/validate.
pub fn interpret_validation(status: u16, body: &Value) -> Validation {
    if status == 401 {
        return Validation::Failed { unauthorized: true, error: twitch_error(ErrorCode::TokenInvalid, None) };
    }
    if status != 200 {
        return Validation::Failed { unauthorized: false, error: twitch_error(ErrorCode::Network, Some(&format!("HTTP {status}"))) };
    }
    let (Some(client_id), Some(user_id), Some(login), Some(scopes)) = (body.get("client_id").as_str(), body.get("user_id").as_str(), body.get("login").as_str(), body.get("scopes").as_array()) else {
        return Validation::Failed { unauthorized: false, error: twitch_error(ErrorCode::TokenInvalid, Some("réponse /validate inattendue")) };
    };
    if client_id != TWITCH_CLIENT_ID {
        return Validation::Failed { unauthorized: true, error: twitch_error(ErrorCode::WrongClient, None) };
    }
    let scopes: Vec<String> = scopes.iter().filter_map(|s| s.as_str().map(str::to_string)).collect();
    if !TWITCH_SCOPES.iter().all(|s| scopes.iter().any(|x| x == s)) {
        return Validation::Failed { unauthorized: true, error: twitch_error(ErrorCode::MissingScope, None) };
    }
    let expires_in = body.get("expires_in").as_f64().map(|n| n.max(0.0) as u64).unwrap_or(0);
    Validation::Ok(TokenInfo { client_id: client_id.into(), user_id: user_id.into(), login: login.into(), scopes, expires_in })
}

// -- Page de retour (http://localhost:4000/) ------------------------------------------

/// Le jeton arrive dans le *fragment* (`#access_token=…`), jamais envoyé au
/// serveur : cette page le lit, l'efface de la barre d'adresse, puis le relaie
/// en POST à `/auth/callback` (même origine). Rien n'est chargé d'Internet.
pub fn callback_page_csp(nonce: &str) -> String {
    [
        "default-src 'none'".to_string(),
        format!("script-src 'nonce-{nonce}'"),
        format!("style-src 'nonce-{nonce}'"),
        "connect-src 'self'".to_string(),
        "base-uri 'none'".to_string(),
        "form-action 'none'".to_string(),
        "frame-ancestors 'none'".to_string(),
    ]
    .join("; ")
}

pub fn callback_page(nonce: &str) -> String {
    // Identité de l'application (brand_page.rs) ; le nonce est posé en dernier, sur la page complète.
    CALLBACK_PAGE
        .replace("{BRAND_CSS}", crate::brand_page::CSS)
        .replace("{LOGO}", crate::brand_page::LOGO_SVG)
        .replace("{MARK}", crate::brand_page::STATUS_MARK)
        .replace("{NONCE}", nonce)
}

const CALLBACK_PAGE: &str = r#"<!DOCTYPE html>
<html lang="fr">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<meta name="referrer" content="no-referrer" />
<title>Batlay · Twitch</title>
<style nonce="{NONCE}">{BRAND_CSS}</style>
</head>
<body data-state="idle">
<main role="status" aria-live="polite">
  {LOGO}
  <p class="brand">Batlay</p>
  {MARK}
  <h1 id="title">Serveur Twitch de Batlay</h1>
  <p id="message">Il est actif. Pour connecter votre compte, utilisez le bouton « Connecter Twitch » dans Batlay.</p>
</main>
<script nonce="{NONCE}">
(function () {
  var query = new URLSearchParams(location.search);
  var hash = new URLSearchParams(location.hash.replace(/^#/, ''));
  var hasToken = hash.has('access_token');
  if (!hasToken && !query.has('error')) return;

  var body = hasToken
    ? { access_token: hash.get('access_token'), scope: hash.get('scope'), token_type: hash.get('token_type'), state: hash.get('state') }
    : { error: query.get('error'), error_description: query.get('error_description'), state: query.get('state') };

  // Le jeton ne doit ni rester visible ni finir dans l'historique du navigateur.
  history.replaceState(null, '', location.pathname);

  function show(state, title, message) {
    document.body.dataset.state = state;
    document.getElementById('title').textContent = title;
    document.getElementById('message').textContent = message;
  }
  show('working', 'Finalisation de la connexion…', 'Un instant, Batlay vérifie votre autorisation.');

  fetch('/auth/callback', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) })
    .then(function (r) { return r.json(); })
    .then(function (r) {
      if (r && r.ok) show('success', 'Twitch est connecté', String(r.message || 'Vous pouvez fermer cet onglet.'));
      else show('error', 'Connexion échouée', String((r && r.message) || 'Réessayez depuis Batlay.'));
    })
    .catch(function () {
      show('error', 'Batlay ne répond pas', 'Vérifiez que Batlay est ouvert, puis relancez la connexion.');
    });
})();
</script>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn authorize_url_has_every_parameter() {
        let url = authorize_url("abcdef0123456789");
        assert!(url.starts_with("https://id.twitch.tv/oauth2/authorize?response_type=token&client_id=vog5xlub7gikyjfoseohp2uo33di6x&redirect_uri=http%3A%2F%2Flocalhost%3A4000"));
        assert!(url.contains("scope=user%3Aread%3Achat%20moderator%3Aread%3Afollowers%20channel%3Aread%3Asubscriptions"));
        assert!(url.ends_with("&state=abcdef0123456789&force_verify=true"));
    }

    #[test]
    fn callback_payloads() {
        let ok = parse(r#"{"access_token":"abcdefghijklmnop0123","scope":"user:read:chat channel:read:subscriptions","token_type":"bearer","state":"abcdefgh12"}"#).unwrap();
        assert_eq!(
            parse_callback(&ok),
            Some(CallbackPayload::Token { access_token: "abcdefghijklmnop0123".into(), scopes: vec!["user:read:chat".into(), "channel:read:subscriptions".into()], state: "abcdefgh12".into() })
        );
        for bad in [
            r#"{"access_token":"court","state":"abcdefgh12"}"#,
            r#"{"access_token":"abcdefghijklmnop0123"}"#,
            r#"{"access_token":"abcdefghijklmnop0123","state":"abcdefgh12","token_type":"mac"}"#,
            r#"{"access_token":"abcdefghijklmnop0123","state":"<script>"}"#,
            r#"{"state":"abcdefgh12"}"#,
            r#"[]"#,
        ] {
            assert_eq!(parse_callback(&parse(bad).unwrap()), None, "{bad}");
        }
        let err = parse_callback(&parse(r#"{"error":"access_denied","error_description":"The user\ndenied","state":null}"#).unwrap()).unwrap();
        assert_eq!(err, CallbackPayload::Error { error: "access_denied".into(), description: "The user denied".into(), state: None });
        assert!(states_match("abcdefgh12", Some("abcdefgh12")));
        assert!(!states_match("abcdefgh12", Some("abcdefgh13")));
        assert!(!states_match("abcdefgh12", None));
    }

    #[test]
    fn validation() {
        let ok = parse(r#"{"client_id":"vog5xlub7gikyjfoseohp2uo33di6x","login":"adil","user_id":"42","scopes":["user:read:chat"],"expires_in":5000}"#).unwrap();
        assert!(matches!(interpret_validation(200, &ok), Validation::Ok(TokenInfo { expires_in: 5000, .. })));
        assert!(matches!(interpret_validation(401, &Value::Null), Validation::Failed { unauthorized: true, .. }));
        assert!(matches!(interpret_validation(500, &Value::Null), Validation::Failed { unauthorized: false, .. }));
        let other = parse(r#"{"client_id":"zzz","login":"adil","user_id":"42","scopes":["user:read:chat"]}"#).unwrap();
        assert!(matches!(interpret_validation(200, &other), Validation::Failed { unauthorized: true, error: TwitchError { code: ErrorCode::WrongClient, .. } }));
        let no_scope = parse(r#"{"client_id":"vog5xlub7gikyjfoseohp2uo33di6x","login":"adil","user_id":"42","scopes":[]}"#).unwrap();
        assert!(matches!(interpret_validation(200, &no_scope), Validation::Failed { error: TwitchError { code: ErrorCode::MissingScope, .. }, .. }));
        assert_eq!(oauth_error("access_denied", "x").code, ErrorCode::AccessDenied);
    }

    #[test]
    fn page_uses_the_nonce() {
        let page = callback_page("NONCE123");
        assert_eq!(page.matches("nonce=\"NONCE123\"").count(), 2);
        assert!(callback_page_csp("NONCE123").contains("script-src 'nonce-NONCE123'"));
    }
}
