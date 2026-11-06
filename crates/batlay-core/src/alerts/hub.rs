//! Point central des alertes : réglages validés, clé de l'adresse locale,
//! filtrage, limite de débit, diffusion à la page des alertes.

use super::model::{normalize_config, normalize_incoming, sample, should_show, AlertEvent, AlertKind, AlertSource};
use crate::json::Value;
use crate::runtime::Clock;
use crate::util::encoding::constant_time_eq;
use crate::util::random::random_hex;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Au plus 20 événements par tranche de 10 s : un outil mal réglé ne peut pas inonder le live.
pub const RATE_MAX: usize = 20;
pub const RATE_WINDOW_MS: u64 = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    Invalid,
    Filtered,
    Rate,
    NoSource,
}

impl Refusal {
    pub fn as_str(self) -> &'static str {
        match self {
            Refusal::Invalid => "invalid",
            Refusal::Filtered => "filtered",
            Refusal::Rate => "rate",
            Refusal::NoSource => "no-source",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ReceiveResult {
    pub shown: bool,
    pub reason: Option<Refusal>,
    pub event: Option<AlertEvent>,
}

impl ReceiveResult {
    pub fn to_json(&self) -> Value {
        let mut v = Value::object().with("shown", self.shown);
        if let Some(r) = self.reason {
            v.set("reason", r.as_str());
        }
        if let Some(e) = &self.event {
            v.set("event", e.to_json());
        }
        v
    }
}

/// Ce dont le hub a besoin (fourni par l'application, simulé en test).
pub trait AlertSink: Send + Sync {
    /// Diffuse aux pages d'alertes ; renvoie le nombre de pages prévenues.
    fn broadcast(&self, event: &AlertEvent) -> usize;
    fn push_config(&self, config: &Value);
    fn save_config(&self, config: &Value);
    fn save_secret(&self, secret: &str);
    /// Chaque événement et ce qui lui est arrivé (journal de la fenêtre Live).
    fn on_result(&self, result: &ReceiveResult, source: AlertSource);
}

struct State {
    config: Value,
    secret: String,
    recent: VecDeque<u64>,
}

pub struct AlertHub {
    state: Mutex<State>,
    sink: Arc<dyn AlertSink>,
    clock: Arc<dyn Clock>,
}

fn is_secret(s: &str) -> bool {
    s.len() == 32 && s.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

impl AlertHub {
    pub fn new(stored_config: &Value, stored_secret: Option<&str>, sink: Arc<dyn AlertSink>, clock: Arc<dyn Clock>) -> AlertHub {
        let secret = match stored_secret.filter(|s| is_secret(s)) {
            Some(s) => s.to_string(),
            None => {
                let s = random_hex(16);
                sink.save_secret(&s);
                s
            }
        };
        AlertHub { state: Mutex::new(State { config: normalize_config(stored_config), secret, recent: VecDeque::new() }), sink, clock }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn config(&self) -> Value {
        self.lock().config.clone()
    }

    /// Réglages venant du tableau de bord : toujours revalidés.
    pub fn set_config(&self, raw: &Value) -> Value {
        let config = normalize_config(raw);
        self.lock().config = config.clone();
        self.sink.save_config(&config);
        self.sink.push_config(&config);
        config
    }

    pub fn secret(&self) -> String {
        self.lock().secret.clone()
    }

    /// Nouvelle clé : l'ancienne adresse cesse immédiatement de fonctionner.
    pub fn regenerate_secret(&self) -> String {
        let s = random_hex(16);
        self.lock().secret = s.clone();
        self.sink.save_secret(&s);
        s
    }

    fn allow_rate(&self) -> bool {
        let now = self.clock.now();
        let mut st = self.lock();
        while st.recent.front().is_some_and(|t| now.saturating_sub(*t) >= RATE_WINDOW_MS) {
            st.recent.pop_front();
        }
        if st.recent.len() >= RATE_MAX {
            return false;
        }
        st.recent.push_back(now);
        true
    }

    /// Événement déjà construit (Twitch, TikTok) ou brut (outil tiers).
    pub fn receive(&self, raw: &Value, source: AlertSource) -> ReceiveResult {
        let result = self.decide(raw, source);
        self.sink.on_result(&result, source);
        result
    }

    fn decide(&self, raw: &Value, source: AlertSource) -> ReceiveResult {
        let Some(event) = normalize_incoming(raw, source, random_hex(8), self.clock.now()) else {
            return ReceiveResult { shown: false, reason: Some(Refusal::Invalid), event: None };
        };
        let config = self.config();
        if !should_show(&config, &event) {
            return ReceiveResult { shown: false, reason: Some(Refusal::Filtered), event: Some(event) };
        }
        if source != AlertSource::Test && !self.allow_rate() {
            crate::log::warn(&format!("[Alertes] trop d'événements : {} de {} ignoré", event.kind.as_str(), event.user));
            return ReceiveResult { shown: false, reason: Some(Refusal::Rate), event: Some(event) };
        }
        let pages = self.sink.broadcast(&event);
        if pages > 0 {
            ReceiveResult { shown: true, reason: None, event: Some(event) }
        } else {
            ReceiveResult { shown: false, reason: Some(Refusal::NoSource), event: Some(event) }
        }
    }

    /// « Rejouer » : nouvel identifiant, affichée même si son type a été désactivé depuis.
    pub fn replay(&self, event: &AlertEvent) -> bool {
        let fresh = AlertEvent { id: random_hex(8), at: self.clock.now(), ..event.clone() };
        self.sink.broadcast(&fresh) > 0
    }

    /// Bouton « Tester ».
    pub fn test(&self, kind: &Value) -> ReceiveResult {
        let kind = kind.as_str().and_then(AlertKind::from_exact).unwrap_or(AlertKind::Follow);
        let e = sample(kind, random_hex(8), self.clock.now());
        let raw = e.to_json().with("type", kind.as_str());
        self.receive(&raw, AlertSource::Test)
    }

    /// Adresse locale appelée par un outil tiers. Clé comparée en temps constant.
    pub fn handle_hook(&self, secret: &str, payload: &Value) -> (u16, String) {
        let expected = self.secret();
        if !constant_time_eq(secret.as_bytes(), expected.as_bytes()) {
            return (404, "Adresse inconnue.".into());
        }
        let r = self.receive(payload, AlertSource::Tiktok);
        match (r.reason, &r.event) {
            (None, Some(e)) => (200, format!("Alerte affichée : {} ({}).", e.kind.as_str(), e.user)),
            (Some(Refusal::Invalid), _) | (None, None) => (400, "Type d'événement manquant ou inconnu (follow, subscribe, resub, gift, tiktokGift).".into()),
            (Some(Refusal::Filtered), _) => (202, "Alerte désactivée dans Batlay (type ou source TikTok).".into()),
            (Some(Refusal::Rate), _) => (429, "Trop d'alertes en peu de temps : celle-ci est ignorée.".into()),
            (Some(Refusal::NoSource), _) => (202, "Aucune page d'alertes ouverte (ajoutez l'URL des alertes dans votre logiciel).".into()),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::json::parse;
    use crate::runtime::ManualClock;

    #[derive(Default)]
    pub struct FakeSink {
        pub pages: Mutex<usize>,
        pub broadcasts: Mutex<Vec<AlertEvent>>,
        pub results: Mutex<Vec<(ReceiveResult, AlertSource)>>,
        pub saved_secret: Mutex<Option<String>>,
        pub saved_config: Mutex<Option<Value>>,
    }
    impl AlertSink for FakeSink {
        fn broadcast(&self, event: &AlertEvent) -> usize {
            self.broadcasts.lock().unwrap().push(event.clone());
            *self.pages.lock().unwrap()
        }
        fn push_config(&self, _: &Value) {}
        fn save_config(&self, c: &Value) {
            *self.saved_config.lock().unwrap() = Some(c.clone());
        }
        fn save_secret(&self, s: &str) {
            *self.saved_secret.lock().unwrap() = Some(s.to_string());
        }
        fn on_result(&self, r: &ReceiveResult, s: AlertSource) {
            self.results.lock().unwrap().push((r.clone(), s));
        }
    }

    fn hub(pages: usize) -> (AlertHub, Arc<FakeSink>, Arc<ManualClock>) {
        let sink = Arc::new(FakeSink { pages: Mutex::new(pages), ..Default::default() });
        let clock = ManualClock::new(1000);
        (AlertHub::new(&Value::Null, Some("0123456789abcdef0123456789abcdef"), sink.clone(), clock.clone()), sink, clock)
    }

    #[test]
    fn hook_key_and_outcomes() {
        let (hub, sink, _) = hub(1);
        assert_eq!(hub.handle_hook("mauvaise", &parse(r#"{"type":"follow"}"#).unwrap()).0, 404);
        let key = hub.secret();
        let (status, message) = hub.handle_hook(&key, &parse(r#"{"type":"follow","user":"Lina"}"#).unwrap());
        assert_eq!((status, message.as_str()), (200, "Alerte affichée : follow (Lina)."));
        assert_eq!(hub.handle_hook(&key, &parse(r#"{"type":"raid"}"#).unwrap()).0, 400);
        *sink.pages.lock().unwrap() = 0;
        assert_eq!(hub.handle_hook(&key, &parse(r#"{"type":"follow"}"#).unwrap()).0, 202);
        let old = hub.secret();
        let new = hub.regenerate_secret();
        assert_ne!(old, new);
        assert_eq!(sink.saved_secret.lock().unwrap().as_deref(), Some(new.as_str()));
        assert_eq!(hub.handle_hook(&old, &parse(r#"{"type":"follow"}"#).unwrap()).0, 404);
        assert_eq!(sink.results.lock().unwrap().len(), 3, "une mauvaise clé n'atteint jamais le journal");
    }

    #[test]
    fn rate_limit_but_not_for_tests() {
        let (hub, _sink, clock) = hub(1);
        for _ in 0..RATE_MAX {
            assert!(hub.receive(&parse(r#"{"type":"follow"}"#).unwrap(), AlertSource::Twitch).shown);
        }
        let r = hub.receive(&parse(r#"{"type":"follow"}"#).unwrap(), AlertSource::Twitch);
        assert_eq!(r.reason, Some(Refusal::Rate));
        assert!(hub.test(&Value::from("resub")).shown, "un test passe toujours");
        clock.advance(RATE_WINDOW_MS);
        assert!(hub.receive(&parse(r#"{"type":"follow"}"#).unwrap(), AlertSource::Twitch).shown);
    }

    #[test]
    fn filtered_and_replay() {
        let (hub, sink, _) = hub(1);
        hub.set_config(&parse(r#"{"kinds":{"follow":{"enabled":false}}}"#).unwrap());
        assert!(sink.saved_config.lock().unwrap().is_some());
        let r = hub.receive(&parse(r#"{"type":"follow","user":"x"}"#).unwrap(), AlertSource::Tiktok);
        assert_eq!(r.reason, Some(Refusal::Filtered));
        assert!(hub.replay(r.event.as_ref().unwrap()), "rejouée même désactivée");
        assert_ne!(sink.broadcasts.lock().unwrap()[0].id, r.event.unwrap().id);
    }

    #[test]
    fn invalid_stored_secret_is_replaced() {
        let sink = Arc::new(FakeSink::default());
        let hub = AlertHub::new(&Value::Null, Some("trop-court"), sink.clone(), ManualClock::new(0));
        assert_eq!(hub.secret().len(), 32);
        assert_eq!(sink.saved_secret.lock().unwrap().as_deref(), Some(hub.secret().as_str()));
    }
}
