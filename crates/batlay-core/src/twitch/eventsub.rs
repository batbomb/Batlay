//! Protocole EventSub (WebSocket) de Twitch : analyse des messages entrants et
//! conversion vers le modèle de l'overlay. Tout ce qui vient de Twitch est une
//! entrée non fiable (types vérifiés, tailles bornées) : l'overlay ne reçoit
//! jamais le JSON brut.

use crate::json::Value;
use crate::obj;
use crate::util::url::Url;

pub const EVENTSUB_WS_URL: &str = "wss://eventsub.wss.twitch.tv/ws";
pub const EVENTSUB_SUBSCRIPTIONS_URL: &str = "https://api.twitch.tv/helix/eventsub/subscriptions";
pub const HELIX_USERS_URL: &str = "https://api.twitch.tv/helix/users";
pub const HELIX_GLOBAL_BADGES_URL: &str = "https://api.twitch.tv/helix/chat/badges/global";
pub const HELIX_CHANNEL_BADGES_URL: &str = "https://api.twitch.tv/helix/chat/badges";

/// (type, obligatoire) : sans les abonnements de modération, un message supprimé
/// resterait affiché à l'antenne, mais le chat fonctionne.
pub const CHAT_SUBSCRIPTIONS: [(&str, bool); 4] = [
    ("channel.chat.message", true),
    ("channel.chat.message_delete", false),
    ("channel.chat.clear_user_messages", false),
    ("channel.chat.clear", false),
];

/// Alertes de SA chaîne (permissions facultatives) : (type, version).
pub const ALERT_SUBSCRIPTIONS: [(&str, &str); 4] = [("channel.follow", "2"), ("channel.subscribe", "1"), ("channel.subscription.message", "1"), ("channel.subscription.gift", "1")];

pub fn is_alert_subscription(t: &str) -> bool {
    ALERT_SUBSCRIPTIONS.iter().any(|(s, _)| *s == t)
}

pub fn subscription_body(kind: &str, user_id: &str, session_id: &str, broadcaster_id: &str) -> Value {
    obj! {
        "type" => kind,
        "version" => "1",
        "condition" => obj! { "broadcaster_user_id" => broadcaster_id, "user_id" => user_id },
        "transport" => obj! { "method" => "websocket", "session_id" => session_id },
    }
}

pub fn alert_subscription_body(kind: &str, version: &str, broadcaster_id: &str, session_id: &str) -> Value {
    let condition = if kind == "channel.follow" {
        obj! { "broadcaster_user_id" => broadcaster_id, "moderator_user_id" => broadcaster_id }
    } else {
        obj! { "broadcaster_user_id" => broadcaster_id }
    };
    obj! { "type" => kind, "version" => version, "condition" => condition, "transport" => obj! { "method" => "websocket", "session_id" => session_id } }
}

fn tier_label(v: &Value) -> Option<&'static str> {
    match v.as_str()? {
        "1000" => Some("1"),
        "2000" => Some("2"),
        "3000" => Some("3"),
        _ => None,
    }
}

/// Événement EventSub → données brutes d'alerte (revalidées par le hub). `None` = rien à afficher.
pub fn to_alert_payload(kind: &str, event: &Value) -> Option<Value> {
    if !event.is_object() {
        return None;
    }
    let user = event.get("user_name").as_str().or_else(|| event.get("user_login").as_str());
    let tier = tier_label(event.get("tier"));
    match kind {
        "channel.follow" => Some(obj! { "type" => "follow", "user" => user? }),
        "channel.subscribe" => {
            // Un abonnement offert produit un channel.subscribe PAR destinataire : seule l'alerte « offert » est montrée.
            if event.get("is_gift").as_bool() == Some(true) {
                return None;
            }
            Some(obj! { "type" => "subscribe", "user" => user?, "tier" => tier })
        }
        "channel.subscription.message" => {
            let mut v = obj! { "type" => "resub", "user" => user?, "tier" => tier, "months" => event.get("cumulative_months").clone() };
            if let Some(m) = event.path(&["message", "text"]).as_str() {
                v.set("message", m);
            }
            Some(v)
        }
        "channel.subscription.gift" => {
            let who = if event.get("is_anonymous").as_bool() == Some(true) || user.is_none() { "Un donateur anonyme" } else { user? };
            Some(obj! { "type" => "gift", "user" => who, "tier" => tier, "amount" => event.get("total").clone() })
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventSubMessage {
    Welcome { session_id: String, keepalive_seconds: u64 },
    Keepalive,
    Reconnect { url: String },
    Revocation { subscription_type: String, status: String },
    Notification { message_id: String, subscription_type: String, event: Value },
}

/// L'URL de reconnexion vient de Twitch : rien d'autre qu'un wss:// *.twitch.tv.
pub fn is_trusted_reconnect_url(raw: &str) -> bool {
    match Url::parse(raw) {
        Some(u) => u.scheme == "wss" && (u.host == "twitch.tv" || u.host.ends_with(".twitch.tv")),
        None => false,
    }
}

pub fn parse_message(raw: &str) -> Option<EventSubMessage> {
    let json = crate::json::parse(raw).ok()?;
    let metadata = json.get("metadata");
    if !metadata.is_object() {
        return None;
    }
    let payload = json.get("payload");
    match metadata.get("message_type").as_str()? {
        "session_welcome" => {
            let session = payload.get("session");
            let id = session.get("id").as_str()?;
            let keepalive = session.get("keepalive_timeout_seconds").as_f64().map(|n| n.max(0.0) as u64).unwrap_or(10);
            Some(EventSubMessage::Welcome { session_id: id.to_string(), keepalive_seconds: keepalive })
        }
        "session_keepalive" => Some(EventSubMessage::Keepalive),
        "session_reconnect" => {
            let url = payload.path(&["session", "reconnect_url"]).as_str()?;
            is_trusted_reconnect_url(url).then(|| EventSubMessage::Reconnect { url: url.to_string() })
        }
        "revocation" => {
            let sub = payload.get("subscription");
            Some(EventSubMessage::Revocation { subscription_type: sub.get("type").as_str().unwrap_or("").into(), status: sub.get("status").as_str().unwrap_or("").into() })
        }
        "notification" => {
            let message_id = metadata.get("message_id").as_str()?;
            let subscription_type = metadata.get("subscription_type").as_str()?;
            Some(EventSubMessage::Notification { message_id: message_id.into(), subscription_type: subscription_type.into(), event: payload.get("event").clone() })
        }
        _ => None,
    }
}

fn is_emote_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn normalize_badges(value: &Value) -> Value {
    Value::Array(
        value
            .items()
            .iter()
            .take(20)
            .filter_map(|b| Some(obj! { "setId" => b.get("set_id").as_str()?, "id" => b.get("id").as_str()? }))
            .collect(),
    )
}

fn normalize_fragments(value: &Value) -> Vec<Value> {
    value
        .items()
        .iter()
        .take(200)
        .filter_map(|f| {
            let text = f.get("text").as_str()?;
            let kind = f.get("type").as_str();
            Some(match kind {
                Some("emote") if f.path(&["emote", "id"]).as_str().is_some_and(is_emote_id) => obj! { "type" => "emote", "text" => text, "emoteId" => f.path(&["emote", "id"]).as_str().unwrap_or_default() },
                Some(k @ ("mention" | "cheermote")) => obj! { "type" => k, "text" => text },
                _ => obj! { "type" => "text", "text" => text },
            })
        })
        .collect()
}

/// Message de chat EventSub → modèle de l'overlay.
pub fn normalize_chat_message(event: &Value, now: u64) -> Option<Value> {
    let message = event.get("message");
    if !event.is_object() || !message.is_object() {
        return None;
    }
    let id = event.get("message_id").as_str()?;
    let user_id = event.get("chatter_user_id").as_str()?;
    let login = event.get("chatter_user_login").as_str()?;
    let display = event.get("chatter_user_name").as_str()?;
    let text = message.get("text").as_str()?;
    // Chat partagé : un message venu d'une AUTRE chaîne porte les badges de celle-ci.
    let from_other = event.get("source_broadcaster_user_id").as_str().is_some_and(|s| Some(s) != event.get("broadcaster_user_id").as_str());
    let badges = normalize_badges(if from_other && event.get("source_badges").is_array() { event.get("source_badges") } else { event.get("badges") });
    let highlight = if event.get("message_type").as_str() == Some("channel_points_highlighted") {
        "channel_points"
    } else if event.get("cheer").is_object() {
        "cheer"
    } else if event.get("message_type").as_str() == Some("user_intro") {
        "first"
    } else {
        "none"
    };
    let reply_to = event.path(&["reply", "parent_user_name"]).as_str();
    let color = event.get("color").as_str().filter(|c| crate::util::text::is_hex_color(c)).map(|c| c.to_ascii_lowercase());
    let mut fragments = normalize_fragments(message.get("fragments"));
    if fragments.is_empty() {
        fragments.push(obj! { "type" => "text", "text" => text });
    }
    Some(obj! {
        "id" => id, "userId" => user_id, "login" => login, "displayName" => display, "color" => color, "badges" => badges,
        "fragments" => Value::Array(fragments), "text" => text, "timestamp" => now, "highlight" => highlight, "replyTo" => reply_to,
    })
}

/// Notification EventSub → message pour l'overlay (`None` = à ignorer).
pub fn to_overlay_message(kind: &str, event: &Value, now: u64) -> Option<Value> {
    match kind {
        "channel.chat.message" => Some(obj! { "type" => "chat", "message" => normalize_chat_message(event, now)? }),
        "channel.chat.message_delete" => Some(obj! { "type" => "delete", "messageId" => event.get("message_id").as_str()? }),
        "channel.chat.clear_user_messages" => Some(obj! { "type" => "clear_user", "userId" => event.get("target_user_id").as_str()? }),
        "channel.chat.clear" => Some(obj! { "type" => "clear" }),
        _ => None,
    }
}

/// `{ data: [{ set_id, versions: [{ id, image_url_2x }] }] }` → `{ "set_id/id": url }`.
pub fn parse_badge_sets(json: &Value) -> Value {
    let mut out = Value::object();
    for set in json.get("data").items() {
        let Some(set_id) = set.get("set_id").as_str() else { continue };
        for v in set.get("versions").items() {
            let Some(id) = v.get("id").as_str() else { continue };
            let url = v.get("image_url_2x").as_str().or_else(|| v.get("image_url_1x").as_str());
            if let Some(url) = url.filter(|u| u.starts_with("https://static-cdn.jtvnw.net/")) {
                out.set(format!("{set_id}/{id}"), url);
            }
        }
    }
    out
}

/// Délai avant la tentative n° `attempt` : 1 s, 2 s, 4 s… plafonné à 30 s.
pub fn reconnect_delay_ms(attempt: u32) -> u64 {
    (1000u64 << attempt.min(5)).min(30_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;

    #[test]
    fn parses_session_messages() {
        let w = parse_message(r#"{"metadata":{"message_type":"session_welcome"},"payload":{"session":{"id":"S1","keepalive_timeout_seconds":10}}}"#).unwrap();
        assert_eq!(w, EventSubMessage::Welcome { session_id: "S1".into(), keepalive_seconds: 10 });
        let r = parse_message(r#"{"metadata":{"message_type":"session_reconnect"},"payload":{"session":{"reconnect_url":"wss://eventsub.wss.twitch.tv/ws?x=1"}}}"#).unwrap();
        assert!(matches!(r, EventSubMessage::Reconnect { .. }));
        assert!(parse_message(r#"{"metadata":{"message_type":"session_reconnect"},"payload":{"session":{"reconnect_url":"wss://evil.example/ws"}}}"#).is_none());
        assert!(parse_message("pas du json").is_none());
        assert!(!is_trusted_reconnect_url("wss://twitch.tv.evil.example/"));
    }

    #[test]
    fn chat_message_normalization() {
        let event = parse(r##"{"message_id":"m1","chatter_user_id":"7","chatter_user_login":"lina","chatter_user_name":"Lina","color":"#FF00AA","broadcaster_user_id":"1",
            "badges":[{"set_id":"subscriber","id":"12","info":"x"}],"message_type":"user_intro",
            "message":{"text":"salut Kappa","fragments":[{"type":"text","text":"salut "},{"type":"emote","text":"Kappa","emote":{"id":"25"}},{"type":"emote","text":"X","emote":{"id":"../x"}}]},
            "reply":{"parent_user_name":"Rémi"}}"##).unwrap();
        let m = to_overlay_message("channel.chat.message", &event, 99).unwrap();
        let msg = m.get("message");
        assert_eq!((msg.get("color").as_str(), msg.get("highlight").as_str(), msg.get("replyTo").as_str()), (Some("#ff00aa"), Some("first"), Some("Rémi")));
        assert_eq!(msg.get("badges").to_json(), r#"[{"setId":"subscriber","id":"12"}]"#);
        assert_eq!(msg.get("fragments").at(1).get("emoteId").as_str(), Some("25"));
        assert_eq!(msg.get("fragments").at(2).get("type").as_str(), Some("text"), "identifiant d'émote suspect → texte");
        assert!(to_overlay_message("channel.chat.message", &parse(r#"{"message_id":"m1"}"#).unwrap(), 0).is_none());
        assert_eq!(to_overlay_message("channel.chat.clear", &Value::Null, 0).unwrap().to_json(), r#"{"type":"clear"}"#);
    }

    #[test]
    fn alert_payloads() {
        assert_eq!(to_alert_payload("channel.follow", &parse(r#"{"user_name":"Lina"}"#).unwrap()).unwrap().to_json(), r#"{"type":"follow","user":"Lina"}"#);
        assert!(to_alert_payload("channel.subscribe", &parse(r#"{"user_name":"x","is_gift":true}"#).unwrap()).is_none());
        let gift = to_alert_payload("channel.subscription.gift", &parse(r#"{"is_anonymous":true,"tier":"1000","total":5}"#).unwrap()).unwrap();
        assert_eq!(gift.to_json(), r#"{"type":"gift","user":"Un donateur anonyme","tier":"1","amount":5}"#);
        let resub = to_alert_payload("channel.subscription.message", &parse(r#"{"user_login":"kaya","tier":"2000","cumulative_months":14,"message":{"text":"merci"}}"#).unwrap()).unwrap();
        assert_eq!(resub.to_json(), r#"{"type":"resub","user":"kaya","tier":"2","months":14,"message":"merci"}"#);
    }

    #[test]
    fn badges_and_backoff() {
        let b = parse_badge_sets(&parse(r#"{"data":[{"set_id":"moderator","versions":[{"id":"1","image_url_1x":"https://static-cdn.jtvnw.net/a","image_url_2x":"https://static-cdn.jtvnw.net/b"},{"id":"2","image_url_2x":"https://evil.example/x"}]}]}"#).unwrap());
        assert_eq!(b.to_json(), r#"{"moderator/1":"https://static-cdn.jtvnw.net/b"}"#);
        assert_eq!([0, 1, 2, 4, 5, 9].map(reconnect_delay_ms), [1000, 2000, 4000, 16000, 30000, 30000]);
    }
}
