//! Module Twitch : constantes, erreurs affichables, état publié au tableau de
//! bord, choix de la chaîne. Aucun secret : le Client ID est public par nature.

use crate::config::TWITCH_PORT;
use crate::json::Value;
use crate::obj;

pub const TWITCH_CLIENT_ID: &str = "vog5xlub7gikyjfoseohp2uo33di6x";
pub const TWITCH_REDIRECT_URI: &str = "http://localhost:4000";
/// Obligatoire : lire le chat. Facultatives : les alertes (refusées, le chat marche quand même).
pub const TWITCH_SCOPES: [&str; 1] = ["user:read:chat"];
pub const TWITCH_ALERT_SCOPES: [&str; 2] = ["moderator:read:followers", "channel:read:subscriptions"];
pub const TWITCH_OVERLAY_PATH: &str = "/overlay/chat";
pub const TWITCH_WS_PATH: &str = "/ws/chat";
pub const TWITCH_AUTH_TIMEOUT_MS: u64 = 5 * 60_000;

pub fn overlay_url(host: &str) -> String {
    format!("http://{host}:{TWITCH_PORT}{TWITCH_OVERLAY_PATH}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    AccessDenied,
    AuthorizationTimeout,
    InvalidState,
    OauthError,
    TokenInvalid,
    TokenExpired,
    MissingScope,
    WrongClient,
    Network,
    PortInUse,
    ServerError,
    SubscriptionFailed,
    InvalidChannel,
    ChannelNotFound,
    StorageUnavailable,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::AccessDenied => "access_denied",
            ErrorCode::AuthorizationTimeout => "authorization_timeout",
            ErrorCode::InvalidState => "invalid_state",
            ErrorCode::OauthError => "oauth_error",
            ErrorCode::TokenInvalid => "token_invalid",
            ErrorCode::TokenExpired => "token_expired",
            ErrorCode::MissingScope => "missing_scope",
            ErrorCode::WrongClient => "wrong_client",
            ErrorCode::Network => "network",
            ErrorCode::PortInUse => "port_in_use",
            ErrorCode::ServerError => "server_error",
            ErrorCode::SubscriptionFailed => "subscription_failed",
            ErrorCode::InvalidChannel => "invalid_channel",
            ErrorCode::ChannelNotFound => "channel_not_found",
            ErrorCode::StorageUnavailable => "storage_unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitchError {
    pub code: ErrorCode,
    pub message: String,
}

impl TwitchError {
    pub fn to_json(&self) -> Value {
        obj! { "code" => self.code.as_str(), "message" => self.message.clone() }
    }
}

/// Message affichable tel quel (français, avec la marche à suivre).
pub fn twitch_error(code: ErrorCode, detail: Option<&str>) -> TwitchError {
    let suffix = detail.map(|d| format!(" ({d})")).unwrap_or_default();
    let message = match code {
        ErrorCode::AccessDenied => "Autorisation refusée sur Twitch. Relancez la connexion et cliquez sur « Autoriser ».".to_string(),
        ErrorCode::AuthorizationTimeout => "Délai dépassé : Twitch n'a pas répondu dans les 5 minutes. Relancez la connexion.".to_string(),
        ErrorCode::InvalidState => "Réponse Twitch rejetée (contrôle de sécurité « state » invalide). Relancez la connexion depuis Batlay.".to_string(),
        ErrorCode::OauthError => format!(
            "Twitch a refusé la connexion{suffix}. Vérifiez sur dev.twitch.tv que l'URI de redirection est exactement {TWITCH_REDIRECT_URI} et que l'application est de type « Confidentiel »."
        ),
        ErrorCode::TokenInvalid => format!("Le jeton Twitch reçu est invalide{suffix}. Relancez la connexion."),
        ErrorCode::TokenExpired => "Votre session Twitch a expiré ou a été révoquée. Reconnectez votre compte.".to_string(),
        ErrorCode::MissingScope => "Batlay n'a pas la permission de lire le chat (user:read:chat). Reconnectez-vous et acceptez la demande.".to_string(),
        ErrorCode::WrongClient => "Ce jeton n'a pas été émis pour l'application Batlay. Relancez la connexion.".to_string(),
        ErrorCode::Network => format!("Impossible de joindre Twitch{suffix}. Vérifiez votre connexion internet."),
        ErrorCode::PortInUse => format!("Le port {TWITCH_PORT} est déjà utilisé par une autre application. Fermez-la puis relancez Batlay (le port Twitch est fixe et ne peut pas être changé)."),
        ErrorCode::ServerError => format!("Le serveur Twitch de Batlay n'a pas pu démarrer{suffix}."),
        ErrorCode::SubscriptionFailed => format!("Twitch a refusé l'abonnement au chat{suffix}."),
        ErrorCode::InvalidChannel => "Pseudo ou lien Twitch non reconnu. Saisissez un pseudo (ex. : ninja) ou un lien comme https://www.twitch.tv/ninja.".to_string(),
        ErrorCode::ChannelNotFound => format!("La chaîne « {} » est introuvable sur Twitch. Vérifiez l'orthographe du pseudo.", detail.unwrap_or("?")),
        ErrorCode::StorageUnavailable => {
            "Le chiffrement sécurisé du système est indisponible : la session Twitch ne sera pas mémorisée (reconnexion nécessaire à chaque lancement).".to_string()
        }
    };
    TwitchError { code, message }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitchAccount {
    pub id: String,
    pub login: String,
    pub display_name: String,
    pub profile_image_url: Option<String>,
}

impl TwitchAccount {
    pub fn to_json(&self) -> Value {
        obj! { "id" => self.id.clone(), "login" => self.login.clone(), "displayName" => self.display_name.clone(), "profileImageUrl" => self.profile_image_url.clone() }
    }
    pub fn from_json(v: &Value) -> Option<TwitchAccount> {
        Some(TwitchAccount {
            id: v.get("id").non_empty_str()?.to_string(),
            login: v.get("login").non_empty_str()?.to_string(),
            display_name: v.get("displayName").as_str().unwrap_or_default().to_string(),
            profile_image_url: v.get("profileImageUrl").as_str().filter(|u| u.starts_with("https://")).map(str::to_string),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    Disconnected,
    Authorizing,
    Validating,
    Connected,
}

impl AuthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthStatus::Disconnected => "disconnected",
            AuthStatus::Authorizing => "authorizing",
            AuthStatus::Validating => "validating",
            AuthStatus::Connected => "connected",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatStatus {
    Off,
    Connecting,
    Live,
    Reconnecting,
}

impl ChatStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ChatStatus::Off => "off",
            ChatStatus::Connecting => "connecting",
            ChatStatus::Live => "live",
            ChatStatus::Reconnecting => "reconnecting",
        }
    }
}

/// État publié au tableau de bord (même forme que `TwitchState` côté TypeScript).
#[derive(Debug, Clone, PartialEq)]
pub struct TwitchState {
    pub status: AuthStatus,
    pub chat: ChatStatus,
    pub account: Option<TwitchAccount>,
    pub channel: Option<TwitchAccount>,
    pub channel_login: Option<String>,
    pub server_running: bool,
    pub overlay_clients: usize,
    pub token_expires_at: Option<u64>,
    pub error: Option<TwitchError>,
    pub warning: Option<TwitchError>,
    pub alert_follows: bool,
    pub alert_subscriptions: bool,
}

impl Default for TwitchState {
    fn default() -> Self {
        TwitchState {
            status: AuthStatus::Disconnected,
            chat: ChatStatus::Off,
            account: None,
            channel: None,
            channel_login: None,
            server_running: false,
            overlay_clients: 0,
            token_expires_at: None,
            error: None,
            warning: None,
            alert_follows: false,
            alert_subscriptions: false,
        }
    }
}

impl TwitchState {
    /// `host` : `localhost` ou `batlay.test` (nom local préféré pour l'URL du chat).
    pub fn to_json(&self, host: &str) -> Value {
        obj! {
            "status" => self.status.as_str(),
            "chat" => self.chat.as_str(),
            "account" => self.account.as_ref().map(TwitchAccount::to_json),
            "channel" => self.channel.as_ref().map(TwitchAccount::to_json),
            "channelLogin" => self.channel_login.clone(),
            "server" => obj! { "running" => self.server_running, "port" => TWITCH_PORT, "overlayUrl" => overlay_url(host) },
            "overlayClients" => self.overlay_clients,
            "tokenExpiresAt" => self.token_expires_at,
            "error" => self.error.as_ref().map(TwitchError::to_json),
            "warning" => self.warning.as_ref().map(TwitchError::to_json),
            "alertScopes" => obj! { "follows" => self.alert_follows, "subscriptions" => self.alert_subscriptions },
        }
    }
}

pub fn alert_scopes_from(scopes: &[String]) -> (bool, bool) {
    (scopes.iter().any(|s| s == "moderator:read:followers"), scopes.iter().any(|s| s == "channel:read:subscriptions"))
}

const NON_CHANNEL_PATHS: [&str; 21] = [
    "directory", "videos", "settings", "downloads", "jobs", "turbo", "subscriptions", "wallet", "friends", "messages", "search", "store", "prime", "drops",
    "inventory", "following", "team", "p", "u", "login", "signup",
];

fn is_login(s: &str) -> bool {
    (1..=25).contains(&s.len()) && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// Pseudo (login) d'un pseudo saisi ou d'un lien Twitch, en minuscules ; `None` sinon.
/// Accepte `ninja`, `@Ninja`, `twitch.tv/ninja`, `https://www.twitch.tv/popout/ninja/chat`…
pub fn parse_channel(input: &str) -> Option<String> {
    let text = input.trim();
    if text.is_empty() || text.chars().count() > 300 {
        return None;
    }
    let lower = text.to_ascii_lowercase();
    let without_scheme = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    let host_end = without_scheme.find(['/', '?', '#']).unwrap_or(without_scheme.len());
    let host = &without_scheme[..host_end];
    let is_twitch_url = host == "twitch.tv" || (host.ends_with(".twitch.tv") && host[..host.len() - ".twitch.tv".len()].chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
    let candidate = if is_twitch_url {
        let path = without_scheme[host_end..].split(['?', '#']).next().unwrap_or("");
        let mut segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        if segments.first().is_some_and(|s| matches!(*s, "popout" | "moderator" | "embed")) {
            segments.remove(0);
        }
        segments.first()?.to_string()
    } else {
        if text.contains('/') || text.chars().any(char::is_whitespace) {
            return None;
        }
        text.trim_start_matches(['@', '#']).to_ascii_lowercase()
    };
    let login = crate::util::url::decode(&candidate, false).to_ascii_lowercase();
    if NON_CHANNEL_PATHS.contains(&login.as_str()) || !is_login(&login) {
        return None;
    }
    Some(login)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_inputs() {
        for (input, expected) in [
            ("ninja", Some("ninja")),
            ("@Ninja", Some("ninja")),
            ("#ninja_2", Some("ninja_2")),
            ("twitch.tv/ninja", Some("ninja")),
            ("https://www.twitch.tv/ninja/videos", Some("ninja")),
            ("https://www.twitch.tv/popout/ninja/chat?popout=", Some("ninja")),
            ("https://m.twitch.tv/Ninja", Some("ninja")),
            ("https://www.twitch.tv/directory", None),
            ("https://www.twitch.tv/", None),
            ("https://evil.example/ninja", None),
            ("ni nja", None),
            ("a/b", None),
            ("", None),
            ("x".repeat(26).as_str(), None),
        ] {
            assert_eq!(parse_channel(input).as_deref(), expected, "{input}");
        }
    }

    #[test]
    fn state_json_shape() {
        let s = TwitchState { error: Some(twitch_error(ErrorCode::ChannelNotFound, Some("zzz"))), ..Default::default() };
        let v = s.to_json("batlay.test");
        assert_eq!(v.path(&["server", "overlayUrl"]).as_str(), Some("http://batlay.test:4000/overlay/chat"));
        assert_eq!(v.path(&["error", "message"]).as_str(), Some("La chaîne « zzz » est introuvable sur Twitch. Vérifiez l'orthographe du pseudo."));
        assert_eq!(v.path(&["alertScopes", "follows"]).as_bool(), Some(false));
    }
}
