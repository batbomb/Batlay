//! Fenêtre « Live » : journal d'activité, chats récents (Twitch + TikTok),
//! compteurs du live et infos envoyées aux overlays (`{lastFollower}`…).
//! Tout reste en mémoire, le temps de la session de Batlay.

use crate::alerts::hub::ReceiveResult;
use crate::alerts::model::{describe, AlertEvent, AlertKind, AlertSource};
use crate::json::Value;
use crate::obj;
use crate::runtime::{Rt, TimerId};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub const LIMIT_ACTIVITY: usize = 300;
pub const LIMIT_CHAT: usize = 300;
/// Les likes arrivent par dizaines à la seconde : une mise à jour par seconde au plus.
pub const AUDIENCE_THROTTLE_MS: u64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Twitch,
    Tiktok,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Twitch => "twitch",
            Platform::Tiktok => "tiktok",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PlatformCounters {
    pub follows: u64,
    pub subs: u64,
    pub gifted_subs: u64,
    pub gifts: u64,
    pub messages: u64,
    pub viewers: u64,
    pub likes: u64,
}

impl PlatformCounters {
    fn to_json(self) -> Value {
        obj! {
            "follows" => self.follows,
            "subs" => self.subs,
            "giftedSubs" => self.gifted_subs,
            "gifts" => self.gifts,
            "messages" => self.messages,
            "viewers" => self.viewers,
            "likes" => self.likes,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters {
    pub since: u64,
    pub twitch: PlatformCounters,
    pub tiktok: PlatformCounters,
}

impl Counters {
    pub fn new(now: u64) -> Counters {
        Counters { since: now, ..Default::default() }
    }
    fn platform(&mut self, source: AlertSource) -> Option<&mut PlatformCounters> {
        match source {
            AlertSource::Twitch => Some(&mut self.twitch),
            AlertSource::Tiktok => Some(&mut self.tiktok),
            AlertSource::Test => None,
        }
    }
    /// Compte un événement RÉEL (jamais un test), affiché ou non : un follow reste un follow.
    pub fn count_alert(&mut self, e: &AlertEvent) -> bool {
        let Some(c) = self.platform(e.source) else { return false };
        match e.kind {
            AlertKind::Follow => c.follows += 1,
            AlertKind::Subscribe | AlertKind::Resub => c.subs += 1,
            AlertKind::Gift => c.gifted_subs += u64::from(e.amount.unwrap_or(1)),
            AlertKind::TiktokGift => c.gifts += u64::from(e.amount.unwrap_or(1)),
        }
        true
    }
    pub fn to_json(&self) -> Value {
        obj! { "since" => self.since, "twitch" => self.twitch.to_json(), "tiktok" => self.tiktok.to_json() }
    }
}

/// Derniers spectateurs (vrais événements seulement).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LastNames {
    pub follower: String,
    pub sub: String,
    pub gift: String,
}

impl LastNames {
    pub fn update(&mut self, e: &AlertEvent) -> bool {
        if e.source == AlertSource::Test {
            return false;
        }
        let (slot, value) = match e.kind {
            AlertKind::Follow => (&mut self.follower, e.user.clone()),
            AlertKind::Subscribe | AlertKind::Resub => (&mut self.sub, e.user.clone()),
            AlertKind::Gift | AlertKind::TiktokGift => (&mut self.gift, format!("{} ({})", e.user, describe(e))),
        };
        if *slot == value {
            return false;
        }
        *slot = value;
        true
    }
}

/// Infos du live pour les overlays : totaux Twitch + TikTok et derniers spectateurs.
pub fn stats_json(c: &Counters, last: &LastNames) -> Value {
    obj! {
        "followers" => c.twitch.follows + c.tiktok.follows,
        "subs" => c.twitch.subs + c.tiktok.subs,
        "gifts" => c.twitch.gifted_subs + c.tiktok.gifts,
        "messages" => c.twitch.messages + c.tiktok.messages,
        "viewers" => c.twitch.viewers + c.tiktok.viewers,
        "likes" => c.twitch.likes + c.tiktok.likes,
        "lastFollower" => last.follower.clone(),
        "lastSub" => last.sub.clone(),
        "lastGift" => last.gift.clone(),
    }
}

/// Où envoyer les changements (fenêtres du tableau de bord, pages d'overlay).
pub trait LiveSink: Send + Sync {
    /// Mise à jour pour la fenêtre Live (`live:update`).
    fn update(&self, update: Value);
    /// Infos du live pour les overlays personnalisés.
    fn stats(&self, stats: Value);
}

struct State {
    activity: VecDeque<Value>,
    chat: VecDeque<Value>,
    counters: Counters,
    last: LastNames,
    seq: u64,
    audience_sent_at: Option<u64>,
    audience_timer: Option<TimerId>,
}

pub struct LiveHub {
    state: Mutex<State>,
    sink: Arc<dyn LiveSink>,
    rt: Rt,
}

impl LiveHub {
    pub fn new(sink: Arc<dyn LiveSink>, rt: Rt) -> Arc<LiveHub> {
        let now = rt.now();
        Arc::new(LiveHub {
            state: Mutex::new(State { activity: VecDeque::new(), chat: VecDeque::new(), counters: Counters::new(now), last: LastNames::default(), seq: 0, audience_sent_at: None, audience_timer: None }),
            sink,
            rt,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn next_id(st: &mut State, prefix: &str, now: u64) -> String {
        st.seq += 1;
        format!("{prefix}-{now}-{}", st.seq)
    }

    pub fn snapshot(&self) -> Value {
        let st = self.lock();
        obj! {
            "activity" => Value::Array(st.activity.iter().cloned().collect()),
            "chat" => Value::Array(st.chat.iter().cloned().collect()),
            "counters" => st.counters.to_json(),
        }
    }

    pub fn stats(&self) -> Value {
        let st = self.lock();
        stats_json(&st.counters, &st.last)
    }

    fn push_activity(&self, st: &mut State, entry: Value) -> Value {
        st.activity.push_back(entry.clone());
        while st.activity.len() > LIMIT_ACTIVITY {
            st.activity.pop_front();
        }
        obj! { "type" => "activity", "entry" => entry }
    }

    /// Résultat du hub des alertes (Twitch, TikTok, adresse locale, tests).
    pub fn record_alert(&self, result: &ReceiveResult, source: AlertSource) {
        let Some(event) = &result.event else {
            let platform = if source == AlertSource::Test { "batlay" } else { source.as_str() };
            self.system(platform, "warn", "Événement d'alerte ignoré : type manquant ou inconnu.");
            return;
        };
        let outcome = if result.shown { "shown" } else { result.reason.map(|r| r.as_str()).unwrap_or("invalid") };
        let mut updates = Vec::new();
        let stats = {
            let mut st = self.lock();
            let id = Self::next_id(&mut st, "a", self.rt.now());
            let entry = obj! {
                "id" => id, "at" => event.at, "type" => "alert", "source" => source.as_str(), "kind" => event.kind.as_str(),
                "user" => event.user.clone(), "detail" => describe(event), "outcome" => outcome, "event" => event.to_json(),
            };
            updates.push(self.push_activity(&mut st, entry));
            let counted = st.counters.count_alert(event);
            if counted {
                updates.push(obj! { "type" => "counters", "counters" => st.counters.to_json() });
            }
            let renamed = st.last.update(event);
            (counted || renamed).then(|| stats_json(&st.counters, &st.last))
        };
        for u in updates {
            self.sink.update(u);
        }
        if let Some(s) = stats {
            self.sink.stats(s);
        }
    }

    /// Ligne d'état (connexion, fin de live…). Un même texte répété (nouvel essai
    /// toutes les 90 s) n'est pas ajouté deux fois en 30 minutes.
    pub fn system(&self, source: &str, level: &str, text: &str) {
        let now = self.rt.now();
        let update = {
            let mut st = self.lock();
            let last = st.activity.iter().rev().find(|e| e.get("type").as_str() == Some("system") && e.get("source").as_str() == Some(source));
            if let Some(l) = last {
                if l.get("text").as_str() == Some(text) && now.saturating_sub(l.get("at").as_u64().unwrap_or(0)) < 30 * 60_000 {
                    return;
                }
            }
            let id = Self::next_id(&mut st, "s", now);
            let entry = obj! { "id" => id, "at" => now, "type" => "system", "source" => source, "level" => level, "text" => text };
            self.push_activity(&mut st, entry)
        };
        self.sink.update(update);
    }

    fn bump_messages(st: &mut State, platform: Platform) {
        match platform {
            Platform::Twitch => st.counters.twitch.messages += 1,
            Platform::Tiktok => st.counters.tiktok.messages += 1,
        }
    }

    fn push_chat(st: &mut State, message: Value) -> bool {
        let id = message.get("id").as_str().unwrap_or("").to_string();
        if st.chat.iter().any(|m| m.get("id").as_str() == Some(&id)) {
            return false;
        }
        st.chat.push_back(message);
        while st.chat.len() > LIMIT_CHAT {
            st.chat.pop_front();
        }
        true
    }

    /// Message du serveur du chat Twitch (le même que celui envoyé aux overlays).
    pub fn twitch_message(&self, message: &Value) {
        let mut updates = Vec::new();
        let mut stats = None;
        {
            let mut st = self.lock();
            match message.get("type").as_str() {
                Some("chat") => {
                    let mut m = message.get("message").clone();
                    if m.get("userId").as_str() == Some("batlay-test") {
                        return;
                    }
                    let platform = if m.get("platform").as_str() == Some("tiktok") { Platform::Tiktok } else { Platform::Twitch };
                    m.set("platform", platform.as_str());
                    if !Self::push_chat(&mut st, m.clone()) {
                        return;
                    }
                    if platform == Platform::Twitch {
                        Self::bump_messages(&mut st, Platform::Twitch);
                        updates.push(obj! { "type" => "counters", "counters" => st.counters.to_json() });
                        stats = Some(stats_json(&st.counters, &st.last));
                    }
                    updates.insert(0, obj! { "type" => "chat", "message" => m });
                }
                Some(kind @ ("delete" | "clear_user" | "clear")) => {
                    let before: Vec<String> = st.chat.iter().filter_map(|m| m.get("id").as_str().map(str::to_string)).collect();
                    let target_id = message.get("messageId").as_str().map(str::to_string);
                    let target_user = message.get("userId").as_str().map(str::to_string);
                    st.chat.retain(|m| match kind {
                        "delete" => m.get("id").as_str() != target_id.as_deref(),
                        "clear_user" => m.get("userId").as_str() != target_user.as_deref(),
                        _ => m.get("platform").as_str() != Some("twitch"),
                    });
                    let kept: Vec<&str> = st.chat.iter().filter_map(|m| m.get("id").as_str()).collect();
                    let removed: Vec<Value> = before.iter().filter(|id| !kept.contains(&id.as_str())).map(|id| Value::from(id.as_str())).collect();
                    if removed.is_empty() {
                        return;
                    }
                    updates.push(obj! { "type" => "chat-remove", "ids" => Value::Array(removed) });
                }
                _ => return,
            }
        }
        for u in updates {
            self.sink.update(u);
        }
        if let Some(s) = stats {
            self.sink.stats(s);
        }
    }

    /// Commentaire TikTok, déjà au format commun du chat.
    pub fn tiktok_message(&self, message: Value) {
        let (u1, u2, stats) = {
            let mut st = self.lock();
            if !Self::push_chat(&mut st, message.clone()) {
                return;
            }
            Self::bump_messages(&mut st, Platform::Tiktok);
            (obj! { "type" => "chat", "message" => message }, obj! { "type" => "counters", "counters" => st.counters.to_json() }, stats_json(&st.counters, &st.last))
        };
        self.sink.update(u1);
        self.sink.update(u2);
        self.sink.stats(stats);
    }

    /// Spectateurs / likes TikTok : enregistrés tout de suite, envoyés au plus une fois par seconde.
    pub fn tiktok_audience(self: &Arc<Self>, viewers: Option<u64>, likes: Option<u64>) {
        let now = self.rt.now();
        let send_now = {
            let mut st = self.lock();
            let t = &mut st.counters.tiktok;
            let (v, l) = (viewers.unwrap_or(t.viewers), likes.unwrap_or(t.likes));
            if v == t.viewers && l == t.likes {
                return;
            }
            t.viewers = v;
            t.likes = l;
            let due = st.audience_sent_at.is_none_or(|at| now.saturating_sub(at) >= AUDIENCE_THROTTLE_MS);
            if viewers == Some(0) || due {
                true
            } else {
                if st.audience_timer.is_none() {
                    let wait = AUDIENCE_THROTTLE_MS - now.saturating_sub(st.audience_sent_at.unwrap_or(now));
                    let hub = Arc::downgrade(self);
                    st.audience_timer = Some(self.rt.after(wait, move || {
                        if let Some(hub) = hub.upgrade() {
                            hub.lock().audience_timer = None;
                            hub.send_audience();
                        }
                    }));
                }
                false
            }
        };
        if send_now {
            self.send_audience();
        }
    }

    fn send_audience(&self) {
        let (update, stats) = {
            let mut st = self.lock();
            st.audience_sent_at = Some(self.rt.now());
            (obj! { "type" => "counters", "counters" => st.counters.to_json() }, stats_json(&st.counters, &st.last))
        };
        self.sink.update(update);
        self.sink.stats(stats);
    }

    /// « Remettre à zéro » : compteurs et journal ; les chats et les spectateurs restent.
    pub fn reset(&self) -> Value {
        let now = self.rt.now();
        let stats = {
            let mut st = self.lock();
            let viewers = st.counters.tiktok.viewers;
            st.counters = Counters::new(now);
            st.counters.tiktok.viewers = viewers;
            st.activity.clear();
            st.last = LastNames::default();
            stats_json(&st.counters, &st.last)
        };
        self.sink.stats(stats);
        let snapshot = self.snapshot();
        self.sink.update(obj! { "type" => "reset", "snapshot" => snapshot.clone() });
        snapshot
    }

    /// Événement d'une ligne du journal, pour « Rejouer ».
    pub fn find_event(&self, entry_id: &str) -> Option<AlertEvent> {
        let st = self.lock();
        let entry = st.activity.iter().find(|e| e.get("id").as_str() == Some(entry_id))?;
        if entry.get("type").as_str() != Some("alert") {
            return None;
        }
        AlertEvent::from_json(entry.get("event"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alerts::hub::Refusal;
    use crate::alerts::model::sample;
    use crate::json::parse;
    use crate::runtime::test_rt;

    #[derive(Default)]
    struct Sink {
        updates: Mutex<Vec<Value>>,
        stats: Mutex<Vec<Value>>,
    }
    impl LiveSink for Sink {
        fn update(&self, u: Value) {
            self.updates.lock().unwrap().push(u);
        }
        fn stats(&self, s: Value) {
            self.stats.lock().unwrap().push(s);
        }
    }

    fn event(kind: AlertKind, source: AlertSource, user: &str, amount: Option<u32>) -> AlertEvent {
        AlertEvent { user: user.into(), source, amount, ..sample(kind, "e".into(), 5) }
    }

    #[test]
    fn alerts_counted_and_last_names() {
        let (rt, _clock, _t) = test_rt(1000);
        let sink = Arc::new(Sink::default());
        let hub = LiveHub::new(sink.clone(), rt);
        let follow = event(AlertKind::Follow, AlertSource::Tiktok, "Lina", None);
        hub.record_alert(&ReceiveResult { shown: false, reason: Some(Refusal::NoSource), event: Some(follow) }, AlertSource::Tiktok);
        let gift = event(AlertKind::TiktokGift, AlertSource::Tiktok, "Rémi", Some(5));
        hub.record_alert(&ReceiveResult { shown: true, reason: None, event: Some(gift) }, AlertSource::Tiktok);
        let test = event(AlertKind::Follow, AlertSource::Test, "Test", None);
        hub.record_alert(&ReceiveResult { shown: true, reason: None, event: Some(test) }, AlertSource::Test);
        let s = hub.stats();
        assert_eq!((s.get("followers").as_u64(), s.get("gifts").as_u64()), (Some(1), Some(5)));
        assert_eq!((s.get("lastFollower").as_str(), s.get("lastGift").as_str()), (Some("Lina"), Some("Rémi (5 × Rose)")));
        let snap = hub.snapshot();
        assert_eq!(snap.get("activity").items().len(), 3);
        assert_eq!(snap.get("activity").at(0).get("outcome").as_str(), Some("no-source"));
        let id = snap.get("activity").at(1).get("id").as_str().unwrap().to_string();
        assert_eq!(hub.find_event(&id).unwrap().user, "Rémi");
        assert_eq!(sink.stats.lock().unwrap().len(), 2, "le test ne change pas les infos du live");
        let after = hub.reset();
        assert_eq!(after.get("activity").items().len(), 0);
        assert_eq!(hub.stats().get("lastFollower").as_str(), Some(""));
    }

    #[test]
    fn system_lines_are_not_repeated() {
        let (rt, clock, _t) = test_rt(0);
        let sink = Arc::new(Sink::default());
        let hub = LiveHub::new(sink.clone(), rt);
        hub.system("tiktok", "info", "En attente du live.");
        clock.advance(90_000);
        hub.system("tiktok", "info", "En attente du live.");
        assert_eq!(hub.snapshot().get("activity").items().len(), 1);
        clock.advance(31 * 60_000);
        hub.system("tiktok", "info", "En attente du live.");
        assert_eq!(hub.snapshot().get("activity").items().len(), 2);
    }

    #[test]
    fn chat_and_moderation() {
        let (rt, _c, _t) = test_rt(0);
        let sink = Arc::new(Sink::default());
        let hub = LiveHub::new(sink.clone(), rt);
        hub.twitch_message(&parse(r#"{"type":"chat","message":{"id":"m1","userId":"u1","text":"salut"}}"#).unwrap());
        hub.twitch_message(&parse(r#"{"type":"chat","message":{"id":"m1","userId":"u1","text":"salut"}}"#).unwrap());
        hub.twitch_message(&parse(r#"{"type":"chat","message":{"id":"t","userId":"batlay-test","text":"test"}}"#).unwrap());
        hub.tiktok_message(parse(r#"{"id":"tt-1","userId":"tt:9","text":"coucou","platform":"tiktok"}"#).unwrap());
        let counters = hub.snapshot().get("counters").clone();
        assert_eq!((counters.path(&["twitch", "messages"]).as_u64(), counters.path(&["tiktok", "messages"]).as_u64()), (Some(1), Some(1)));
        hub.twitch_message(&parse(r#"{"type":"clear"}"#).unwrap());
        let chat = hub.snapshot().get("chat").clone();
        assert_eq!(chat.items().len(), 1, "« clear » ne vide que le chat Twitch");
        assert_eq!(sink.updates.lock().unwrap().last().unwrap().get("type").as_str(), Some("chat-remove"));
    }

    #[test]
    fn audience_is_throttled() {
        let (rt, clock, timers) = test_rt(0);
        let sink = Arc::new(Sink::default());
        let hub = LiveHub::new(sink.clone(), rt);
        hub.tiktok_audience(Some(10), None);
        assert_eq!(sink.stats.lock().unwrap().len(), 1);
        clock.set(200);
        hub.tiktok_audience(None, Some(50));
        hub.tiktok_audience(None, Some(80));
        hub.tiktok_audience(None, Some(80));
        assert_eq!(sink.stats.lock().unwrap().len(), 1);
        assert_eq!(timers.pending(), vec![800]);
        clock.set(1000);
        timers.fire_last();
        assert_eq!(sink.stats.lock().unwrap().last().unwrap().get("likes").as_u64(), Some(80));
        hub.tiktok_audience(Some(0), None);
        assert_eq!(sink.stats.lock().unwrap().last().unwrap().get("viewers").as_u64(), Some(0), "fin du live : tout de suite");
    }
}
