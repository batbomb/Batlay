//! Alertes (nouveau follower, abonné, réabonnement, abonnements offerts, cadeau
//! TikTok) : modèle et validation. Tout ce qui arrive du réseau passe par
//! `normalize_incoming` ; les réglages par `normalize_config`.

use crate::json::Value;
use crate::obj;
use crate::util::text::{clean_text, is_hex_color};

pub const ALERT_KINDS: [&str; 5] = ["follow", "subscribe", "resub", "gift", "tiktokGift"];

pub const ALERTS_PAGE_PATH: &str = "/alerts";
pub const ALERTS_WS_PATH: &str = "/ws-alerts";
pub const ALERTS_HOOK_PREFIX: &str = "/api/alerts/hook/";
pub const ALERT_MEDIA_PREFIX: &str = "/api/alert-media/";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertKind {
    Follow,
    Subscribe,
    Resub,
    Gift,
    TiktokGift,
}

impl AlertKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AlertKind::Follow => "follow",
            AlertKind::Subscribe => "subscribe",
            AlertKind::Resub => "resub",
            AlertKind::Gift => "gift",
            AlertKind::TiktokGift => "tiktokGift",
        }
    }
    pub fn from_exact(s: &str) -> Option<AlertKind> {
        Some(match s {
            "follow" => AlertKind::Follow,
            "subscribe" => AlertKind::Subscribe,
            "resub" => AlertKind::Resub,
            "gift" => AlertKind::Gift,
            "tiktokGift" => AlertKind::TiktokGift,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            AlertKind::Follow => "Nouveau follower",
            AlertKind::Subscribe => "Nouvel abonné",
            AlertKind::Resub => "Réabonnement",
            AlertKind::Gift => "Abonnements offerts",
            AlertKind::TiktokGift => "Cadeau TikTok",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlertSource {
    Twitch,
    Tiktok,
    Test,
}

impl AlertSource {
    pub fn as_str(self) -> &'static str {
        match self {
            AlertSource::Twitch => "twitch",
            AlertSource::Tiktok => "tiktok",
            AlertSource::Test => "test",
        }
    }
    pub fn parse(s: &str) -> Option<AlertSource> {
        Some(match s {
            "twitch" => AlertSource::Twitch,
            "tiktok" => AlertSource::Tiktok,
            "test" => AlertSource::Test,
            _ => return None,
        })
    }
    pub fn label(self) -> &'static str {
        match self {
            AlertSource::Twitch => "Twitch",
            AlertSource::Tiktok => "TikTok",
            AlertSource::Test => "Test",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertEvent {
    pub id: String,
    pub kind: AlertKind,
    pub source: AlertSource,
    pub user: String,
    pub amount: Option<u32>,
    pub gift: Option<String>,
    pub months: Option<u32>,
    pub tier: Option<String>,
    pub message: Option<String>,
    pub at: u64,
}

impl AlertEvent {
    pub fn to_json(&self) -> Value {
        let mut v = obj! { "id" => self.id.clone(), "kind" => self.kind.as_str(), "source" => self.source.as_str(), "user" => self.user.clone() };
        if let Some(a) = self.amount {
            v.set("amount", a);
        }
        if let Some(g) = &self.gift {
            v.set("gift", g.clone());
        }
        if let Some(m) = self.months {
            v.set("months", m);
        }
        if let Some(t) = &self.tier {
            v.set("tier", t.clone());
        }
        if let Some(m) = &self.message {
            v.set("message", m.clone());
        }
        v.set("at", self.at);
        v
    }

    /// Relit un événement déjà validé (journal de la fenêtre Live).
    pub fn from_json(v: &Value) -> Option<AlertEvent> {
        Some(AlertEvent {
            id: v.get("id").as_str()?.to_string(),
            kind: AlertKind::from_exact(v.get("kind").as_str()?)?,
            source: AlertSource::parse(v.get("source").as_str()?)?,
            user: v.get("user").as_str()?.to_string(),
            amount: v.get("amount").as_u64().map(|n| n as u32),
            gift: v.get("gift").as_str().map(str::to_string),
            months: v.get("months").as_u64().map(|n| n as u32),
            tier: v.get("tier").as_str().map(str::to_string),
            message: v.get("message").as_str().map(str::to_string),
            at: v.get("at").as_u64().unwrap_or(0),
        })
    }
}

/// Noms acceptés pour le type (outils tiers : vocabulaire variable).
pub fn parse_kind(v: &Value) -> Option<AlertKind> {
    let s: String = v.as_str()?.trim().to_lowercase().chars().filter(|c| !matches!(c, ' ' | '\t' | '\n' | '_' | '.' | '-')).collect();
    Some(match s.as_str() {
        "follow" | "follower" => AlertKind::Follow,
        "sub" | "subscribe" | "subscription" => AlertKind::Subscribe,
        "resub" | "resubscribe" => AlertKind::Resub,
        "gift" | "subgift" | "giftsub" => AlertKind::Gift,
        "tiktokgift" | "present" | "cadeau" => AlertKind::TiktokGift,
        _ => return None,
    })
}

/// Texte d'alerte : nombre ou chaîne → texte nettoyé.
fn alert_text(v: &Value, max: usize) -> String {
    match v {
        Value::String(s) => clean_text(s, max),
        Value::Number(n) => clean_text(n.text(), max),
        _ => String::new(),
    }
}

/// Entier strictement positif (nombre entier, ou chaîne de 1 à 6 chiffres), plafonné.
fn positive_int(v: &Value, max: u32) -> Option<u32> {
    let n: Option<f64> = match v {
        Value::String(s) if !s.trim().is_empty() && s.trim().len() <= 6 && s.trim().chars().all(|c| c.is_ascii_digit()) => s.trim().parse().ok(),
        Value::Number(_) => v.as_f64(),
        _ => None,
    };
    let n = n?;
    (n.fract() == 0.0 && n > 0.0).then(|| (n.min(f64::from(max))) as u32)
}

fn first<'a>(raw: &'a Value, keys: &[&str]) -> &'a Value {
    static NULL: Value = Value::Null;
    keys.iter().map(|k| raw.get(k)).find(|v| !v.is_null()).unwrap_or(&NULL)
}

/// Événement reçu (adresse locale, EventSub converti, test) → événement sûr, ou `None`.
pub fn normalize_incoming(raw: &Value, source: AlertSource, id: String, now: u64) -> Option<AlertEvent> {
    if !raw.is_object() {
        return None;
    }
    let kind = parse_kind(first(raw, &["kind", "type", "event"]))?;
    let user = alert_text(first(raw, &["user", "username", "nickname", "name"]), 40);
    let user = if user.is_empty() { "Quelqu'un".to_string() } else { user };
    let tier = alert_text(raw.get("tier"), 10);
    let message = alert_text(first(raw, &["message", "comment"]), 200);
    let gift = alert_text(first(raw, &["gift", "giftName"]), 40);
    Some(AlertEvent {
        id,
        kind,
        source,
        user,
        amount: positive_int(first(raw, &["amount", "count", "total"]), 1000),
        months: positive_int(first(raw, &["months", "cumulative_months"]), 1000),
        tier: (!tier.is_empty()).then_some(tier),
        message: (!message.is_empty()).then_some(message),
        gift: (!gift.is_empty()).then_some(gift),
        at: now,
    })
}

// -- Fichiers importés ---------------------------------------------------------------

/// Identifiant d'un fichier importé : 24 hex + extension.
pub fn is_media_id(v: &str) -> bool {
    let Some((hash, ext)) = v.split_once('.') else { return false };
    hash.len() == 24 && hash.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) && matches!(ext, "png" | "jpg" | "gif" | "webp" | "mp3" | "ogg" | "wav")
}
pub fn is_image_media_id(v: &str) -> bool {
    is_media_id(v) && matches!(v.rsplit('.').next(), Some("png" | "jpg" | "gif" | "webp"))
}
pub fn is_sound_media_id(v: &str) -> bool {
    is_media_id(v) && matches!(v.rsplit('.').next(), Some("mp3" | "ogg" | "wav"))
}

// -- Réglages -------------------------------------------------------------------------

fn base_style(title: &str, subtitle: &str, accent: &str) -> Value {
    obj! {
        "enabled" => true,
        "title" => title,
        "subtitle" => subtitle,
        "imageId" => Value::Null,
        "sound" => obj! { "type" => "chime" },
        "volume" => 0.6,
        "durationMs" => 5000,
        "enter" => "pop",
        "exit" => "fade",
        "animMs" => 500,
        "layout" => "stack",
        "accentColor" => accent,
        "textColor" => "#FFFFFF",
        "fontSize" => 40,
        "card" => true,
    }
}

pub fn default_config() -> Value {
    obj! {
        "version" => 1,
        "kinds" => obj! {
            "follow" => base_style("{user}", "nous suit maintenant !", "#9B6BFF"),
            "subscribe" => base_style("{user}", "vient de s'abonner !", "#37E29A"),
            "resub" => base_style("{user}", "est abonné depuis {months} mois !", "#37E29A"),
            "gift" => base_style("{user}", "offre {amount} abonnement(s) !", "#F5C542"),
            "tiktokGift" => base_style("{user}", "envoie {amount} × {gift} !", "#FF3B6B"),
        },
        "sources" => obj! { "twitch" => true, "tiktok" => true },
        "gapMs" => 600,
    }
}

fn clamp(v: &Value, min: f64, max: f64, fallback: f64) -> f64 {
    v.as_f64().map(|n| n.clamp(min, max)).unwrap_or(fallback)
}

/// Arrondi comme `Math.round` (moitiés vers +∞).
pub fn js_round(n: f64) -> f64 {
    (n + 0.5).floor()
}

fn is_effect(s: &str) -> bool {
    (2..=20).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphabetic())
}

fn normalize_sound(v: &Value, fallback: &Value) -> Value {
    if !v.is_object() {
        return fallback.clone();
    }
    match v.get("type").as_str() {
        Some("none") => obj! { "type" => "none" },
        Some("chime") => obj! { "type" => "chime" },
        Some("file") => match v.get("mediaId").as_str().filter(|id| is_sound_media_id(id)) {
            Some(id) => obj! { "type" => "file", "mediaId" => id },
            None => fallback.clone(),
        },
        _ => fallback.clone(),
    }
}

fn normalize_style(s: &Value, d: &Value) -> Value {
    let text = |key: &str, max: usize| match s.get(key).as_str() {
        Some(t) => Value::from(t.chars().take(max).collect::<String>()),
        None => d.get(key).clone(),
    };
    let flag = |key: &str| Value::Bool(s.get(key).as_bool().unwrap_or_else(|| d.get(key).as_bool().unwrap_or(true)));
    let color = |key: &str| match s.get(key).as_str() {
        Some(c) if is_hex_color(c) => Value::from(c),
        _ => d.get(key).clone(),
    };
    let effect = |key: &str| match s.get(key).as_str() {
        Some(e) if is_effect(e) => Value::from(e),
        _ => d.get(key).clone(),
    };
    let explicit_null_image = s.as_object().is_some_and(|m| matches!(m.get("imageId"), Some(Value::Null)));
    let image = match s.get("imageId") {
        Value::String(id) if is_image_media_id(id) => Value::from(id.clone()),
        _ if explicit_null_image => Value::Null,
        _ => d.get("imageId").clone(),
    };
    let layout = match s.get("layout").as_str() {
        Some("side") => "side",
        Some("stack") => "stack",
        _ => d.get("layout").as_str().unwrap_or("stack"),
    };
    let fnum = |key: &str| d.get(key).as_f64().unwrap_or(0.0);
    obj! {
        "enabled" => flag("enabled"),
        "title" => text("title", 120),
        "subtitle" => text("subtitle", 200),
        "imageId" => image,
        "sound" => normalize_sound(s.get("sound"), d.get("sound")),
        "volume" => clamp(s.get("volume"), 0.0, 1.0, fnum("volume")),
        "durationMs" => js_round(clamp(s.get("durationMs"), 1000.0, 30000.0, fnum("durationMs"))),
        "enter" => effect("enter"),
        "exit" => effect("exit"),
        "animMs" => js_round(clamp(s.get("animMs"), 50.0, 3000.0, fnum("animMs"))),
        "layout" => layout,
        "accentColor" => color("accentColor"),
        "textColor" => color("textColor"),
        "fontSize" => js_round(clamp(s.get("fontSize"), 16.0, 120.0, fnum("fontSize"))),
        "card" => flag("card"),
    }
}

/// Réglages relus ou reçus du tableau de bord → réglages complets et valides.
pub fn normalize_config(raw: &Value) -> Value {
    let d = default_config();
    let mut kinds = Value::object();
    for k in ALERT_KINDS {
        kinds.set(k, normalize_style(raw.get("kinds").get(k), d.get("kinds").get(k)));
    }
    let src = raw.get("sources");
    obj! {
        "version" => 1,
        "kinds" => kinds,
        "sources" => obj! {
            "twitch" => src.get("twitch").as_bool().unwrap_or(true),
            "tiktok" => src.get("tiktok").as_bool().unwrap_or(true),
        },
        "gapMs" => js_round(clamp(raw.get("gapMs"), 0.0, 5000.0, 600.0)),
    }
}

/// L'alerte doit-elle s'afficher ? (type activé, source acceptée ; un test passe si le type est activé).
pub fn should_show(config: &Value, event: &AlertEvent) -> bool {
    if !config.get("kinds").get(event.kind.as_str()).get("enabled").as_bool().unwrap_or(true) {
        return false;
    }
    event.source == AlertSource::Test || config.get("sources").get(event.source.as_str()).as_bool().unwrap_or(true)
}

/// Événement d'exemple (boutons « Tester »).
pub fn sample(kind: AlertKind, id: String, now: u64) -> AlertEvent {
    let mut e = AlertEvent { id, kind, source: AlertSource::Test, user: String::new(), amount: None, gift: None, months: None, tier: None, message: None, at: now };
    match kind {
        AlertKind::Follow => e.user = "Nyx_Live".into(),
        AlertKind::Subscribe => {
            e.user = "Kaya".into();
            e.tier = Some("1".into());
        }
        AlertKind::Resub => {
            e.user = "Rémi_Été".into();
            e.months = Some(14);
            e.tier = Some("1".into());
            e.message = Some("Toujours là, merci pour les lives !".into());
        }
        AlertKind::Gift => {
            e.user = "Batlay_Fan".into();
            e.amount = Some(5);
            e.tier = Some("1".into());
        }
        AlertKind::TiktokGift => {
            e.user = "Lina.mp4".into();
            e.amount = Some(3);
            e.gift = Some("Rose".into());
        }
    }
    e
}

/// Résumé lisible (journal de la fenêtre Live), identique à `describeAlert`.
pub fn describe(e: &AlertEvent) -> String {
    match e.kind {
        AlertKind::Follow => "suit la chaîne".into(),
        AlertKind::Subscribe => match e.tier.as_deref() {
            Some(t) if !t.is_empty() && t != "1" => format!("abonnement niveau {t}"),
            _ => "nouvel abonnement".into(),
        },
        AlertKind::Resub => {
            let months = format!("{} mois", e.months.unwrap_or(1));
            match &e.message {
                Some(m) => format!("{months} · « {m} »"),
                None => months,
            }
        }
        AlertKind::Gift => {
            let n = e.amount.unwrap_or(1);
            let s = if n > 1 { "s" } else { "" };
            format!("{n} abonnement{s} offert{s}")
        }
        AlertKind::TiktokGift => format!("{} × {}", e.amount.unwrap_or(1), e.gift.clone().unwrap_or_else(|| "cadeau".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn incoming_events_are_cleaned() {
        let raw = parse(r#"{"type":"Tiktok_Gift","user":"  Ro\u200bse \n","amount":"5","gift":"Rose","evil":"<script>","months":-3}"#).unwrap();
        let e = normalize_incoming(&raw, AlertSource::Tiktok, "1".into(), 42).unwrap();
        assert_eq!((e.kind, e.user.as_str(), e.amount, e.gift.as_deref(), e.months), (AlertKind::TiktokGift, "Ro se", Some(5), Some("Rose"), None));
        assert_eq!(e.to_json().to_json(), r#"{"id":"1","kind":"tiktokGift","source":"tiktok","user":"Ro se","amount":5,"gift":"Rose","at":42}"#);
        assert!(normalize_incoming(&parse(r#"{"type":"raid"}"#).unwrap(), AlertSource::Test, "1".into(), 0).is_none());
        assert!(normalize_incoming(&parse("[1]").unwrap(), AlertSource::Test, "1".into(), 0).is_none());
        let e = normalize_incoming(&parse(r#"{"event":"sub","amount":2.5}"#).unwrap(), AlertSource::Twitch, "2".into(), 0).unwrap();
        assert_eq!((e.user.as_str(), e.amount), ("Quelqu'un", None));
        let e = normalize_incoming(&parse(r#"{"kind":"gift","count":99999999}"#).unwrap(), AlertSource::Twitch, "3".into(), 0).unwrap();
        assert_eq!(e.amount, Some(1000));
        assert_eq!(AlertEvent::from_json(&e.to_json()).unwrap(), e);
    }

    #[test]
    fn config_is_completed_and_bounded() {
        let raw = parse(r##"{"kinds":{"follow":{"title":"Merci {user}","durationMs":999999,"accentColor":"red","sound":{"type":"file","mediaId":"../../etc/passwd"},"imageId":null,"enter":"<x>"}},"sources":{"tiktok":false},"gapMs":-5}"##).unwrap();
        let c = normalize_config(&raw);
        let f = c.get("kinds").get("follow");
        assert_eq!(f.get("title").as_str(), Some("Merci {user}"));
        assert_eq!(f.get("durationMs").as_f64(), Some(30000.0));
        assert_eq!(f.get("accentColor").as_str(), Some("#9B6BFF"));
        assert_eq!(f.get("sound").get("type").as_str(), Some("chime"));
        assert_eq!(f.get("enter").as_str(), Some("pop"));
        assert!(f.get("imageId").is_null());
        assert_eq!(c.get("kinds").get("tiktokGift").get("subtitle").as_str(), Some("envoie {amount} × {gift} !"));
        assert_eq!((c.get("sources").get("tiktok").as_bool(), c.get("gapMs").as_f64()), (Some(false), Some(0.0)));
        assert_eq!(normalize_config(&c), c, "stable");
        assert_eq!(normalize_config(&Value::Null), normalize_config(&default_config()));
        let e = sample(AlertKind::TiktokGift, "t".into(), 0);
        assert!(!should_show(&c, &AlertEvent { source: AlertSource::Tiktok, ..e.clone() }));
        assert!(should_show(&c, &e));
        let with_image = normalize_config(&parse(r#"{"kinds":{"gift":{"imageId":"0123456789abcdef01234567.png"}}}"#).unwrap());
        assert_eq!(with_image.get("kinds").get("gift").get("imageId").as_str(), Some("0123456789abcdef01234567.png"));
    }

    #[test]
    fn media_ids() {
        assert!(is_image_media_id("0123456789abcdef01234567.png"));
        assert!(is_sound_media_id("0123456789abcdef01234567.ogg"));
        assert!(!is_media_id("0123456789ABCDEF01234567.png"));
        assert!(!is_media_id("../0123456789abcdef0123.png"));
        assert!(!is_image_media_id("0123456789abcdef01234567.mp3"));
    }

    #[test]
    fn descriptions_match_typescript() {
        assert_eq!(describe(&sample(AlertKind::Resub, "1".into(), 0)), "14 mois · « Toujours là, merci pour les lives ! »");
        assert_eq!(describe(&sample(AlertKind::Gift, "1".into(), 0)), "5 abonnements offerts");
        assert_eq!(describe(&sample(AlertKind::TiktokGift, "1".into(), 0)), "3 × Rose");
        assert_eq!(describe(&sample(AlertKind::Subscribe, "1".into(), 0)), "nouvel abonnement");
    }
}
