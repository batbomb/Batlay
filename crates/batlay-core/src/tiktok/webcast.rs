//! Flux d'un live TikTok (« webcast ») : trame binaire → événements de Batlay.
//!
//! Trame `WebcastPushFrame` → charge utile (souvent gzip) → `ProtoMessageFetchResult`
//! → messages typés (chat, cadeau, follow, likes, spectateurs, Super Fan, fin).
//! Numéros de champs : schéma protobuf de la communauté (tiktok-live-connector,
//! TikTokLive), vérifiés par des trames fabriquées avec l'encodeur officiel de
//! Google à partir de ce schéma (voir les tests).

use super::proto::Msg;
use crate::util::inflate::decompress_auto;

/// Taille maximale d'une charge utile décompressée.
const MAX_PAYLOAD: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TikTokUser {
    pub id: Option<u64>,
    pub unique_id: Option<String>,
    pub nickname: Option<String>,
}

impl TikTokUser {
    fn read(m: &Msg) -> TikTokUser {
        let nonempty = |s: Option<String>| s.filter(|v| !v.trim().is_empty());
        TikTokUser { id: m.uint(1).filter(|v| *v != 0), unique_id: nonempty(m.string(38)), nickname: nonempty(m.string(3)) }
    }
    /// Nom affiché : surnom, sinon identifiant.
    pub fn display_name(&self) -> Option<&str> {
        self.nickname.as_deref().or(self.unique_id.as_deref())
    }
    /// Clé stable (un même follow répété n'affiche qu'une alerte).
    pub fn key(&self) -> Option<String> {
        self.id.map(|i| i.to_string()).or_else(|| self.unique_id.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveEvent {
    Chat { msg_id: Option<u64>, user: TikTokUser, comment: String },
    Follow { user: TikTokUser },
    SuperFanJoin { user: TikTokUser },
    Gift { user: TikTokUser, name: Option<String>, diamonds: Option<u64>, repeat_count: u64, repeat_end: bool, gift_type: Option<u64> },
    Viewers(u64),
    Likes(u64),
    StreamEnd,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    Proto(&'static str),
    Decompress(String),
}

impl std::fmt::Display for FrameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameError::Proto(m) => write!(f, "trame illisible ({m})"),
            FrameError::Decompress(m) => write!(f, "trame illisible ({m})"),
        }
    }
}

/// Clé (champ 1) d'un texte TikTok (`Text.key`, « pm_main_follow_message… »).
fn text_key(m: &Msg, field: u32) -> String {
    m.msg(field).and_then(|t| t.string(1)).unwrap_or_default().to_lowercase()
}

/// Premier utilisateur cité dans un texte (pièces « utilisateur »).
fn text_user(m: &Msg, field: u32) -> Option<TikTokUser> {
    let text = m.msg(field)?;
    text.msgs(4).iter().find_map(|piece| piece.msg(21).and_then(|u| u.bytes(1).and_then(|b| Msg::parse(b).ok())).map(|u| TikTokUser::read(&u)))
}

/// Un message du flux (`type` = nom protobuf) → événement, ou `None` s'il n'intéresse pas Batlay.
pub fn decode_message(kind: &str, payload: &[u8]) -> Option<LiveEvent> {
    let m = Msg::parse(payload).ok()?;
    let user_at = |n: u32| m.msg(n).map(|u| TikTokUser::read(&u)).unwrap_or_default();
    match kind {
        "WebcastChatMessage" => {
            let comment = m.string(3)?;
            let msg_id = m.msg(1).and_then(|c| c.uint(2));
            Some(LiveEvent::Chat { msg_id, user: user_at(2), comment })
        }
        "WebcastSocialMessage" => {
            // Même règle que la bibliothèque : le follow se reconnaît à sa clé d'affichage.
            let key = m.msg(1).map(|c| text_key(&c, 8)).unwrap_or_default();
            key.contains("follow").then(|| LiveEvent::Follow { user: user_at(2) })
        }
        "WebcastGiftMessage" => {
            let details = m.msg(15).unwrap_or_default();
            Some(LiveEvent::Gift {
                user: user_at(7),
                name: details.string(16).filter(|s| !s.trim().is_empty()),
                diamonds: details.uint(12),
                repeat_count: m.uint(5).filter(|c| *c > 0).or_else(|| m.uint(6).filter(|c| *c > 0)).unwrap_or(1),
                repeat_end: m.uint(9) == Some(1),
                gift_type: details.uint(11),
            })
        }
        "WebcastBarrageMessage" => {
            let keys = [text_key(&m, 5), text_key(&m, 24)];
            if keys.iter().any(|k| k.contains("ttlive_superfan_commentnotif_superfanjoined")) {
                let user = text_user(&m, 5).or_else(|| text_user(&m, 24)).unwrap_or_default();
                return Some(LiveEvent::SuperFanJoin { user });
            }
            None
        }
        "WebcastLikeMessage" => m.uint(3).map(LiveEvent::Likes),
        "WebcastRoomUserSeqMessage" => m.uint(3).or_else(|| m.uint(7)).map(LiveEvent::Viewers),
        // Fin du live : CONTROL_ACTION_STREAM_ENDED (3) ou SUSPENDED (4).
        "WebcastControlMessage" => matches!(m.uint(2), Some(3 | 4)).then_some(LiveEvent::StreamEnd),
        _ => None,
    }
}

/// Trame binaire du flux → événements (dans l'ordre). Les trames de service (« hb »…) n'en donnent aucun.
pub fn decode_frame(frame: &[u8]) -> Result<Vec<LiveEvent>, FrameError> {
    decode_push(frame).map(|d| d.events)
}

/// Trame décodée, avec ce qu'il faut pour en accuser réception (connexion directe au flux).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DecodedFrame {
    pub events: Vec<LiveEvent>,
    /// `logId` de la trame (champ 2), à renvoyer dans l'accusé de réception.
    pub log_id: Option<u64>,
    /// TikTok demande un accusé de réception (`needsAck`).
    pub needs_ack: bool,
    /// `internalExt`, renvoyé tel quel dans l'accusé de réception.
    pub internal_ext: Option<String>,
}

pub fn decode_push(frame: &[u8]) -> Result<DecodedFrame, FrameError> {
    let push = Msg::parse(frame).map_err(|e| FrameError::Proto(e.0))?;
    let mut out = DecodedFrame { log_id: push.uint(2).filter(|v| *v != 0), ..DecodedFrame::default() };
    if push.string(6).as_deref() != Some("pb") {
        return Ok(out);
    }
    let Some(payload) = push.bytes(8).filter(|p| !p.is_empty()) else { return Ok(out) };
    if push.string(7).is_some_and(|t| t != "msg") {
        return Ok(out);
    }
    let compressed = payload.len() > 2 && payload[0] == 0x1f && payload[1] == 0x8b;
    let owned;
    let payload = if compressed {
        owned = decompress_auto(payload, MAX_PAYLOAD).map_err(|e| FrameError::Decompress(e.to_string()))?;
        &owned[..]
    } else {
        payload
    };
    let result = Msg::parse(payload).map_err(|e| FrameError::Proto(e.0))?;
    out.needs_ack = result.uint(9) == Some(1);
    out.internal_ext = result.string(5).filter(|s| !s.is_empty());
    out.events = result
        .msgs(1)
        .iter()
        .filter_map(|m| {
            let kind = m.string(1)?;
            decode_message(&kind, m.bytes(2).unwrap_or(&[]))
        })
        .collect();
    Ok(out)
}

/// Adresse de la socket du flux (« webcast … »), pour ne lire qu'elle dans la page.
pub fn is_webcast_socket_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    let Some(rest) = lower.strip_prefix("wss://") else { return false };
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    let path = &rest[host.len()..];
    let tiktok_host = host.ends_with(".tiktok.com") || host == "tiktok.com" || host.ends_with(".tiktokv.com");
    tiktok_host && (host.starts_with("webcast") || path.contains("webcast"))
}

#[cfg(test)]
pub mod tests_fixtures {
    // Trames fabriquées par l'encodeur protobuf officiel de Google (python
    // protobuf 7) à partir du schéma communautaire (webcast.proto, data.proto,
    // synthetic.proto de tiktok-live-connector) : voir docs/DEVELOPPEMENT.md.
    pub const FRAME_EVENTS: &str = "CAEQYyoVCg1jb21wcmVzc190eXBlEgRnemlwKg8KCWltLWN1cnNvchICYzEyAnBiOgNtc2dC8gMfiwgAd4nhagL/dZHNaxNBGIedTRbHKaXrkspmEbt+YUjaisVDDYgQUUNNekiq9iBup5t32yE7O9vdTbvpsQgingShl4I3US9KwGOhIh49+R949CSCd2dpNgeJMDDDO7/neeeDxER/BOsOjeLbmzRuQhTRDdAfk3Pj6trRnzeHnw8+7bn6DH62/+3j9+evvj4xcYP51Ppx8HagYE8u53lw3Sw6oieH1YGgxyJrhXVXRNf69frlCwORNikM7W3hMOplfW+Qi7XzZCbgNqfMt13heWLH5se79jaDHQjtBV3DZVNtfXnH2UDJh8CZhaT0zn+kFWLUzpBCKo3tjR7rUN8BO9qkIehTuGLm79M+laKunKTm5ujm95g7epHL2v5sKVc9jWdNtS1cJgE1SufdiVW0hvZQviUikPitsfiVFFfH4HX0r2BxrMAqoeoknjNzTZoMlBynye7kqrL24WdKNpjwJVkakQ3WhYzUtCnjt1U+hedNJFGUyOQlUhwmW0LwBxGEbdjKANV4uPj0pEwBmR6majQM5VaWWCrXSSWOPbYt37EXQOhS33YE5+DHyyJm7nH1LvWXBPOhc6GIJ96jAtHxVTO33JenwH4/sVNetjk7+rcm8HUIsy4KOWEgXXGuVQlRGQ/CQEdJHf0FMBvAG7MCAAA=";
    pub const FRAME_END: &str = "MgJwYjoDbXNnQh0KGwoVV2ViY2FzdENvbnRyb2xNZXNzYWdlEgIQAw==";
    pub const FRAME_HEARTBEAT: &str = "MgJwYjoCaGI=";
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::encoding::base64_decode;

    fn frame(b64: &str) -> Vec<u8> {
        base64_decode(b64).unwrap()
    }

    fn user(id: u64, nick: &str, unique: &str) -> TikTokUser {
        TikTokUser { id: Some(id), unique_id: Some(unique.into()), nickname: Some(nick.into()) }
    }

    #[test]
    fn decodes_a_gzip_frame_built_by_the_official_encoder() {
        let events = decode_frame(&frame(tests_fixtures::FRAME_EVENTS)).unwrap();
        assert_eq!(
            events,
            vec![
                LiveEvent::Chat { msg_id: Some(7351234567890123456), user: user(6812345678901234567, "Lina ✨", "lina.mp4"), comment: "coucou depuis TikTok 👋".into() },
                LiveEvent::Follow { user: user(42, "Rémi", "remi") },
                LiveEvent::Gift { user: user(44, "Sofia", "sofia"), name: Some("Rose".into()), diamonds: Some(1), repeat_count: 3, repeat_end: false, gift_type: Some(1) },
                LiveEvent::Gift { user: user(44, "Sofia", "sofia"), name: Some("Rose".into()), diamonds: Some(1), repeat_count: 5, repeat_end: true, gift_type: Some(1) },
                LiveEvent::Gift { user: user(45, "Max", "max"), name: Some("Lion".into()), diamonds: Some(29999), repeat_count: 1, repeat_end: false, gift_type: Some(2) },
                LiveEvent::Likes(4210),
                LiveEvent::Viewers(86),
                LiveEvent::SuperFanJoin { user: user(47, "Nyx", "nyx_live") },
            ],
            "partage et arrivée d'un spectateur ignorés"
        );
    }

    #[test]
    fn stream_end_and_service_frames() {
        assert_eq!(decode_frame(&frame(tests_fixtures::FRAME_END)).unwrap(), vec![LiveEvent::StreamEnd]);
        assert_eq!(decode_frame(&frame(tests_fixtures::FRAME_HEARTBEAT)).unwrap(), vec![]);
        assert!(decode_frame(&[0x0a, 0x05, 1]).is_err());
        // Charge gzip corrompue : erreur propre.
        let mut bad = frame(tests_fixtures::FRAME_EVENTS);
        let n = bad.len();
        bad[n - 10] ^= 0xff;
        assert!(decode_frame(&bad).is_err());
    }

    #[test]
    fn socket_urls() {
        assert!(is_webcast_socket_url("wss://webcast16-ws-useast1a.tiktok.com/webcast/im/ws_proxy/ws_reuse_supplement/?x=1"));
        assert!(is_webcast_socket_url("wss://webcast-ws.tiktok.com/webcast/im/push/v2/"));
        assert!(!is_webcast_socket_url("wss://im-ws.tiktok.com/ws/v2"));
        assert!(!is_webcast_socket_url("wss://evil.example/webcast"));
        assert!(!is_webcast_socket_url("wss://tiktok.com.evil.example/webcast"));
        assert!(!is_webcast_socket_url("ws://webcast.tiktok.com/"));
    }
}
