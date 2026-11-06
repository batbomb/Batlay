//! Live TikTok : réglages, état publié, pseudo, vérification du compte (oEmbed
//! officiel), recherche du live (API « api-live » et page du live), messages.

use crate::json::{self, Value};
use crate::obj;
use crate::util::url::encode_component;

/// Moyen de connexion au live, choisi par l'utilisateur dans la carte TikTok.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Method {
    /// Page du live dans une fenêtre invisible de Batlay (sans service tiers, compte privé possible).
    #[default]
    Browser,
    /// Signature par le service tiers Euler Stream, puis flux lu directement (comptes publics).
    Euler,
    /// Batlay ne se connecte pas : un autre outil (TikFinity, Streamer.bot…) appelle l'adresse locale.
    Local,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Browser => "browser",
            Method::Euler => "euler",
            Method::Local => "local",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Method::Browser => "navigateur intégré (page du live dans une fenêtre invisible, sans service tiers)",
            Method::Euler => "Euler Stream (service tiers de signature, puis flux lu par Batlay)",
            Method::Local => "adresse locale (TikFinity, Streamer.bot… : Batlay ne se connecte pas lui-même)",
        }
    }
    /// `library` : nom de la méthode Euler en 0.4.x.
    pub fn parse(v: &str) -> Method {
        match v {
            "euler" | "library" => Method::Euler,
            "local" => Method::Local,
            _ => Method::Browser,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub username: Option<String>,
    /// Vrai tant que l'utilisateur n'a pas cliqué « Déconnecter ».
    pub enabled: bool,
    /// Valeur minimale (diamants, quantité comprise) d'un cadeau pour afficher une alerte.
    pub gift_min_diamonds: u64,
    pub method: Method,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { username: None, enabled: false, gift_min_diamonds: 1, method: Method::Browser }
    }
}

impl Settings {
    /// Réglages relus ou reçus : revalidés. Méthode inconnue ou absente = navigateur intégré.
    pub fn normalize(raw: &Value) -> Settings {
        let username = raw.get("username").as_str().and_then(parse_username);
        let min = match raw.get("giftMinDiamonds") {
            Value::String(s) if !s.is_empty() && s.len() <= 9 && s.chars().all(|c| c.is_ascii_digit()) => s.parse::<f64>().ok(),
            v => v.as_f64(),
        };
        Settings {
            enabled: username.is_some() && raw.get("enabled").as_bool() == Some(true),
            username,
            gift_min_diamonds: min.map(|m| crate::alerts::model::js_round(m).clamp(1.0, 100_000.0) as u64).unwrap_or(1),
            method: Method::parse(raw.get("mode").as_str().unwrap_or("browser")),
        }
    }
    pub fn to_json(&self) -> Value {
        obj! { "username" => self.username.clone(), "enabled" => self.enabled, "giftMinDiamonds" => self.gift_min_diamonds, "mode" => self.method.as_str() }
    }
}

/// `pseudo`, `@pseudo` ou `https://www.tiktok.com/@pseudo/live` → pseudo en minuscules.
pub fn parse_username(input: &str) -> Option<String> {
    let mut v = input.trim().to_string();
    let lower = v.to_ascii_lowercase();
    let without_scheme = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    let without_www = without_scheme.strip_prefix("www.").or_else(|| without_scheme.strip_prefix("m.")).unwrap_or(without_scheme);
    if let Some(rest) = without_www.strip_prefix("tiktok.com/@") {
        let offset = v.len() - rest.len();
        let end = rest.find(['/', '?', '#', ' ', '\t']).unwrap_or(rest.len());
        v = v[offset..offset + end].to_string();
    }
    let v = v.strip_prefix('@').unwrap_or(&v);
    let ok = (2..=24).contains(&v.len()) && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    ok.then(|| v.to_ascii_lowercase())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Idle,
    Connecting,
    Live,
    Waiting,
    Hidden,
    Error,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Idle => "idle",
            Status::Connecting => "connecting",
            Status::Live => "live",
            Status::Waiting => "waiting",
            Status::Hidden => "hidden",
            Status::Error => "error",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Status::Idle => "déconnecté",
            Status::Connecting => "connexion en cours",
            Status::Live => "branché sur le live",
            Status::Waiting => "en attente du live",
            Status::Hidden => "live caché",
            Status::Error => "erreur",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub display_name: String,
}

impl Account {
    pub fn to_json(&self) -> Value {
        obj! { "username" => self.username.clone(), "displayName" => self.display_name.clone() }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct State {
    pub status: Status,
    pub settings: Settings,
    pub message: Option<String>,
    pub detail: Option<String>,
    pub retry_at: Option<u64>,
    pub since: Option<u64>,
    pub account: Option<Account>,
    pub logged_in: bool,
    /// Clé API d'Euler enregistrée (chiffrée par Windows) : jamais sa valeur.
    pub euler_key: bool,
}

impl State {
    pub fn initial(settings: Settings) -> State {
        State { status: Status::Idle, settings, message: None, detail: None, retry_at: None, since: None, account: None, logged_in: false, euler_key: false }
    }
    pub fn to_json(&self) -> Value {
        obj! {
            "status" => self.status.as_str(),
            "settings" => self.settings.to_json(),
            "message" => self.message.clone(),
            "detail" => self.detail.clone(),
            "retryAt" => self.retry_at,
            "since" => self.since,
            "account" => self.account.as_ref().map(Account::to_json),
            "loggedIn" => self.logged_in,
            "eulerKey" => self.euler_key,
        }
    }
}

/// Nom affiché : caractères de contrôle retirés, 60 caractères au plus.
fn display_name(raw: &str, fallback: &str) -> String {
    let without_controls: String = raw.chars().filter(|c| (*c as u32) > 0x1f && *c as u32 != 0x7f).collect();
    let cleaned: String = without_controls.trim().chars().take(60).collect();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

// -- Vérification du compte (oEmbed officiel, même hors live) --------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountLookup {
    Found(Account),
    NotFound,
    Unknown(String),
}

pub fn oembed_url(username: &str) -> String {
    format!("https://www.tiktok.com/oembed?url={}", encode_component(&format!("https://www.tiktok.com/@{username}")))
}

fn handle_after(text: &str, marker: &str, stop: &[char]) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let rest = &text[start..];
    let end = rest.find(stop).unwrap_or(rest.len());
    let h = &rest[..end];
    (!h.is_empty()).then(|| h.to_string())
}

pub fn parse_oembed(status: u16, body: &Value, username: &str) -> AccountLookup {
    if status == 400 || status == 404 {
        return AccountLookup::NotFound;
    }
    if status != 200 {
        return AccountLookup::Unknown(format!("HTTP {status}"));
    }
    if !body.is_object() {
        return AccountLookup::Unknown("réponse illisible".into());
    }
    // TikTok répond parfois 200 avec un code d'erreur dans le JSON.
    if let Some(code) = body.get("code").as_f64() {
        if code != 0.0 {
            return if code == 400.0 || code == 404.0 { AccountLookup::NotFound } else { AccountLookup::Unknown(format!("code {code}")) };
        }
    }
    let author_url = body.get("author_url").as_str().unwrap_or("");
    let html = body.get("html").as_str().unwrap_or("");
    let handle = handle_after(author_url, "/@", &['/', '?', '#', '"']).or_else(|| handle_after(html, "data-unique-id=\"", &['"']));
    let Some(handle) = handle else { return AccountLookup::Unknown("profil absent de la réponse".into()) };
    if !handle.eq_ignore_ascii_case(username) {
        return AccountLookup::NotFound;
    }
    let name = body.get("author_name").non_empty_str().unwrap_or(username);
    AccountLookup::Found(Account { username: username.to_ascii_lowercase(), display_name: display_name(name, username) })
}

// -- Recherche du live ---------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomLookup {
    Live { room_id: String, account: Option<Account>, info: String },
    Offline { account: Option<Account>, info: String },
    /// En live, mais l'identifiant est caché aux visiteurs non connectés (compte privé).
    Hidden { private: bool, account: Option<Account>, info: String },
    Unknown(String),
}

impl RoomLookup {
    pub fn account(&self) -> Option<&Account> {
        match self {
            RoomLookup::Live { account, .. } | RoomLookup::Offline { account, .. } | RoomLookup::Hidden { account, .. } => account.as_ref(),
            RoomLookup::Unknown(_) => None,
        }
    }
}

const LIVE_STATUS: f64 = 2.0;

fn num(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) if !s.is_empty() && s.len() <= 9 && s.chars().all(|c| c.is_ascii_digit()) => s.parse().ok(),
        _ => v.as_f64(),
    }
}

/// `{ user: { roomId, status, uniqueId, nickname, secret }, liveRoom: { status } }`.
pub fn read_live_room_user_info(info: &Value) -> Option<RoomLookup> {
    if !info.is_object() {
        return None;
    }
    let user = info.get("user");
    let live_room = info.get("liveRoom");
    let room_raw = [user.get("roomId"), live_room.get("roomId"), info.get("roomId")].into_iter().find(|v| !v.is_null()).cloned().unwrap_or(Value::Null);
    let room_id = match &room_raw {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.text().to_string(),
        _ => String::new(),
    };
    let status = num(live_room.get("status")).or_else(|| num(user.get("status")));
    let account = user.get("uniqueId").non_empty_str().map(|u| Account { username: u.to_ascii_lowercase(), display_name: display_name(user.get("nickname").non_empty_str().unwrap_or(u), u) });
    let is_private = user.get("secret").as_bool() == Some(true) || user.get("privateAccount").as_bool() == Some(true);
    let info = format!(
        "statut {}, room {}{}",
        status.map(|s| (s as i64).to_string()).unwrap_or_else(|| "?".into()),
        if room_id.is_empty() { "aucune".to_string() } else { room_id.clone() },
        if is_private { ", compte privé" } else { "" }
    );
    let valid_room = (6..=25).contains(&room_id.len()) && room_id.chars().all(|c| c.is_ascii_digit()) && room_id != "0";
    if status.is_some_and(|s| s != LIVE_STATUS) {
        return Some(RoomLookup::Offline { account, info });
    }
    if valid_room {
        return Some(RoomLookup::Live { room_id, account, info });
    }
    if status == Some(LIVE_STATUS) {
        return Some(RoomLookup::Hidden { private: is_private, account, info });
    }
    if room_id.is_empty() || room_id == "0" {
        return Some(RoomLookup::Offline { account, info });
    }
    None
}

/// Réponse de `api-live/user/room`.
pub fn parse_live_room_api(status: u16, body: &Value) -> RoomLookup {
    if status != 200 {
        return RoomLookup::Unknown(format!("API live : HTTP {status}"));
    }
    if !body.is_object() {
        return RoomLookup::Unknown("API live : réponse illisible".into());
    }
    if let Some(code) = num(body.get("statusCode")) {
        if code != 0.0 {
            let msg = body.get("message").as_str().map(|m| format!(" ({})", m.chars().take(80).collect::<String>())).unwrap_or_default();
            return RoomLookup::Unknown(format!("API live : statusCode {}{msg}", code as i64));
        }
    }
    read_live_room_user_info(body.get("data")).unwrap_or_else(|| RoomLookup::Unknown("API live : identifiant de live absent".into()))
}

/// Contenu du bloc `<script id="SIGI_STATE" …>…</script>` d'une page.
pub fn extract_sigi(html: &str) -> Option<&str> {
    let start = html.find("<script id=\"SIGI_STATE\"")?;
    let open_end = start + html[start..].find('>')? + 1;
    let close = open_end + html[open_end..].find("</script>")?;
    Some(&html[open_end..close])
}

/// `"roomId":"123456…"` dans le HTML brut.
fn raw_room_id(html: &str) -> Option<String> {
    let mut search = html;
    while let Some(i) = search.find("\"roomId\":\"") {
        let rest = &search[i + 10..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if (6..=25).contains(&digits.len()) && rest[digits.len()..].starts_with('"') {
            return Some(digits);
        }
        search = &search[i + 10..];
    }
    None
}

/// `"liveRoom":{ … "status":N` (sans objet imbriqué entre les deux).
fn raw_live_status(html: &str) -> Option<u32> {
    let mut search = html;
    while let Some(i) = search.find("\"liveRoom\":{") {
        let rest = &search[i + 12..];
        let stop = rest.find(['{', '}']).unwrap_or(rest.len());
        if let Some(j) = rest[..stop].find("\"status\":") {
            if let Some(d) = rest[j + 9..].chars().next().and_then(|c| c.to_digit(10)) {
                return Some(d);
            }
        }
        search = rest;
    }
    None
}

/// Page `https://www.tiktok.com/@pseudo/live` : bloc SIGI_STATE, sinon recherche brute.
pub fn parse_live_page_html(status: u16, html: &str) -> RoomLookup {
    if status != 200 {
        return RoomLookup::Unknown(format!("page du live : HTTP {status}"));
    }
    let sigi = extract_sigi(html);
    if let Some(s) = sigi {
        if let Ok(parsed) = json::parse(s) {
            if let Some(found) = read_live_room_user_info(parsed.path(&["LiveRoom", "liveRoomUserInfo"])) {
                return found;
            }
        }
    }
    let room_id = raw_room_id(html);
    let live_status = raw_live_status(html);
    if let Some(room_id) = room_id {
        if live_status.is_none() || live_status == Some(2) {
            return RoomLookup::Live { room_id, account: None, info: String::new() };
        }
    }
    if live_status.is_some() {
        return RoomLookup::Offline { account: None, info: String::new() };
    }
    let lower = html.to_ascii_lowercase();
    if lower.contains("captcha") || lower.contains("verify-bar") || lower.contains("tiktok-verify-page") {
        return RoomLookup::Unknown("page du live : vérification anti-robot (captcha) de TikTok".into());
    }
    RoomLookup::Unknown(if sigi.is_some() { "page du live : informations du live absentes".into() } else { "page du live : bloc SIGI_STATE absent".into() })
}

pub fn live_api_url(username: &str) -> String {
    format!(
        "https://www.tiktok.com/api-live/user/room/?aid=1988&app_language=fr-FR&app_name=tiktok_web&device_platform=web_pc&uniqueId={}&sourceType=54",
        encode_component(username)
    )
}

pub fn live_page_url(username: &str) -> String {
    format!("https://www.tiktok.com/@{}/live", encode_component(username))
}

// -- Délais et messages ------------------------------------------------------------------------

pub const RETRY_OFFLINE_MS: u64 = 90_000;
pub const RETRY_AFTER_END_MS: u64 = 60_000;
pub const RETRY_OTHER_MS: [u64; 4] = [30_000, 60_000, 120_000, 300_000];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Offline,
    NoRoom,
    NoStream,
    NotFound,
    Other,
}

pub fn retry_delay(kind: ErrorKind, failures: u32) -> Option<u64> {
    match kind {
        ErrorKind::Offline | ErrorKind::NoRoom | ErrorKind::NoStream => Some(RETRY_OFFLINE_MS),
        ErrorKind::NotFound => None,
        ErrorKind::Other => Some(RETRY_OTHER_MS[(failures.max(1) as usize - 1).min(RETRY_OTHER_MS.len() - 1)]),
    }
}

pub fn error_message(kind: ErrorKind, username: &str) -> String {
    match kind {
        ErrorKind::Offline => format!("@{username} n'est pas en live. Batlay se branchera tout seul dès le début du live."),
        ErrorKind::NoStream => format!(
            "La page du live de @{username} s'est ouverte, mais TikTok n'y a pas envoyé le flux du live (pas encore en live, ou page bloquée par une vérification). Batlay réessaie tout seul. Si ça dure, cliquez sur « Afficher la page TikTok » pour voir ce que TikTok affiche."
        ),
        ErrorKind::NoRoom => format!("Live de @{username} introuvable pour le moment : le compte n'est probablement pas en live (ou TikTok bloque la recherche). Batlay réessaie tout seul."),
        ErrorKind::NotFound => format!("Compte @{username} introuvable : vérifiez le pseudo (celui de l'adresse tiktok.com/@…)."),
        ErrorKind::Other => "Connexion au live TikTok impossible pour le moment. Nouvel essai automatique.".into(),
    }
}

/// Rapport texte du diagnostic (à copier pour demander de l'aide).
pub fn format_diagnostic(at: u64, steps: &[(String, &str, String)], version: &str) -> String {
    let mut lines = vec![format!("Diagnostic TikTok · Batlay {version} · {}", crate::log::iso_time(at))];
    for (label, status, detail) in steps {
        let mark = match *status {
            "ok" => "OK",
            "warn" => "ATTENTION",
            "fail" => "ÉCHEC",
            _ => "—",
        };
        lines.push(format!("[{mark}] {label} : {detail}"));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn usernames() {
        for (input, expected) in [
            ("Mon.Pseudo_1", Some("mon.pseudo_1")),
            ("  @adil ", Some("adil")),
            ("https://www.tiktok.com/@adil.live/live", Some("adil.live")),
            ("tiktok.com/@adil?lang=fr", Some("adil")),
            ("m.tiktok.com/@Adil", Some("adil")),
            ("", None),
            ("a", None),
            ("nom avec espace", None),
            ("<script>", None),
            ("https://evil.example/@adil", None),
        ] {
            assert_eq!(parse_username(input).as_deref(), expected, "{input}");
        }
        assert_eq!(parse_username(&"x".repeat(25)), None);
    }

    #[test]
    fn settings() {
        let s = Settings::normalize(&parse(r#"{"username":"@Adil","enabled":true,"giftMinDiamonds":50,"mode":"library"}"#).unwrap());
        assert_eq!(s.to_json().to_json(), r#"{"username":"adil","enabled":true,"giftMinDiamonds":50,"mode":"euler"}"#, "« library » (0.4.x) = Euler");
        for (raw, method) in [("euler", Method::Euler), ("local", Method::Local), ("browser", Method::Browser), ("autre", Method::Browser)] {
            assert_eq!(Settings::normalize(&parse(&format!(r#"{{"mode":"{raw}"}}"#)).unwrap()).method, method);
        }
        assert_eq!(Settings::normalize(&parse("{}").unwrap()).method, Method::Browser);
        let s = Settings::normalize(&parse(r#"{"username":"pas valable !","enabled":true,"giftMinDiamonds":-3}"#).unwrap());
        assert_eq!((s.username, s.enabled, s.gift_min_diamonds), (None, false, 1));
        assert_eq!(Settings::normalize(&parse(r#"{"giftMinDiamonds":10000000}"#).unwrap()).gift_min_diamonds, 100_000);
    }

    #[test]
    fn oembed() {
        let ok = parse(r#"{"author_url":"https://www.tiktok.com/@adil.bli","author_name":"𝐚𝐝𝐢𝐥_𝐛𝐥𝐢","html":""}"#).unwrap();
        assert_eq!(parse_oembed(200, &ok, "adil.bli"), AccountLookup::Found(Account { username: "adil.bli".into(), display_name: "𝐚𝐝𝐢𝐥_𝐛𝐥𝐢".into() }));
        assert_eq!(parse_oembed(400, &Value::Null, "x"), AccountLookup::NotFound);
        assert_eq!(parse_oembed(200, &parse(r#"{"code":404}"#).unwrap(), "x"), AccountLookup::NotFound);
        assert!(matches!(parse_oembed(500, &Value::Null, "x"), AccountLookup::Unknown(_)));
        let other = parse(r#"{"html":"<blockquote data-unique-id=\"someone\">"}"#).unwrap();
        assert_eq!(parse_oembed(200, &other, "adil"), AccountLookup::NotFound);
        assert!(oembed_url("adil.bli").ends_with("url=https%3A%2F%2Fwww.tiktok.com%2F%40adil.bli"));
    }

    #[test]
    fn real_private_account_answer() {
        // Réponse réelle de TikTok (octobre 2026) pour un compte privé en live.
        let body = parse(r#"{"data":{"liveRoom":{"status":2},"user":{"roomId":"","secret":true,"uniqueId":"adil.bli","nickname":"𝐚𝐝𝐢𝐥_𝐛𝐥𝐢","status":2}},"extra":{},"message":"","statusCode":0}"#).unwrap();
        match parse_live_room_api(200, &body) {
            RoomLookup::Hidden { private, account, info } => {
                assert!(private);
                assert_eq!(account.unwrap().username, "adil.bli");
                assert_eq!(info, "statut 2, room aucune, compte privé");
            }
            other => panic!("{other:?}"),
        }
        let live = parse(r#"{"data":{"liveRoom":{"status":2},"user":{"roomId":"7351234567890123456"}},"statusCode":0}"#).unwrap();
        assert!(matches!(parse_live_room_api(200, &live), RoomLookup::Live { room_id, .. } if room_id == "7351234567890123456"));
        let off = parse(r#"{"data":{"liveRoom":{"status":4},"user":{"roomId":"7351234567890123456"}},"statusCode":0}"#).unwrap();
        assert!(matches!(parse_live_room_api(200, &off), RoomLookup::Offline { .. }));
        assert!(matches!(parse_live_room_api(200, &parse(r#"{"statusCode":19881007,"message":"user_not_found"}"#).unwrap()), RoomLookup::Unknown(r) if r.contains("19881007")));
    }

    #[test]
    fn live_page_html() {
        let html = r#"<html><script id="SIGI_STATE" type="application/json">{"LiveRoom":{"liveRoomUserInfo":{"user":{"roomId":"7351234567890123456","uniqueId":"adil"},"liveRoom":{"status":2}}}}</script></html>"#;
        assert!(matches!(parse_live_page_html(200, html), RoomLookup::Live { .. }));
        assert!(matches!(parse_live_page_html(200, r#"x "roomId":"73512345678" y "liveRoom":{"a":1,"status":2}"#), RoomLookup::Live { room_id, .. } if room_id == "73512345678"));
        assert!(matches!(parse_live_page_html(200, r#""liveRoom":{"status":4}"#), RoomLookup::Offline { .. }));
        assert_eq!(parse_live_page_html(200, "<div id=\"captcha-verify\">"), RoomLookup::Unknown("page du live : vérification anti-robot (captcha) de TikTok".into()));
        assert_eq!(parse_live_page_html(403, ""), RoomLookup::Unknown("page du live : HTTP 403".into()));
    }

    #[test]
    fn delays_and_report() {
        assert_eq!(retry_delay(ErrorKind::NotFound, 0), None);
        assert_eq!([1, 2, 3, 4, 9].map(|n| retry_delay(ErrorKind::Other, n).unwrap()), [30_000, 60_000, 120_000, 300_000, 300_000]);
        let r = format_diagnostic(0, &[("Live".into(), "fail", "en cours mais identifiant caché".into())], "0.5.0");
        assert_eq!(r, "Diagnostic TikTok · Batlay 0.5.0 · 1970-01-01T00:00:00.000Z\n[ÉCHEC] Live : en cours mais identifiant caché");
    }
}
