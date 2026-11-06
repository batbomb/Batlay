//! Branchement sur le live TikTok de l'utilisateur, par la méthode choisie :
//!
//!   - navigateur intégré (défaut) : la page du live est ouverte dans une
//!     fenêtre invisible et Batlay lit le flux que TikTok envoie à cette page ;
//!   - Euler Stream : le service tiers signe l'adresse du flux, Batlay s'y
//!     connecte lui-même (`euler.rs`) ;
//!   - adresse locale : Batlay ne se connecte pas, un autre outil appelle
//!     l'adresse locale des alertes (rien à faire ici).
//!
//!   - « Se connecter » : enregistre le pseudo et attend le live ; une fois le
//!     live trouvé, ses follows, Super Fans, cadeaux, commentaires, spectateurs
//!     et likes sont transmis à Batlay ;
//!   - hors live, fin de live, coupure : nouvel essai automatique, espacé ;
//!   - « Déconnecter » : plus aucun essai.

use super::browser::{self, Outcome, PageEvent, PageHost, Timing};
use super::euler::{self, EulerTiming, SignError};
use super::model::{
    error_message, format_diagnostic, live_api_url, live_page_url, oembed_url, parse_live_page_html, parse_live_room_api, parse_oembed, parse_username, retry_delay, Account, AccountLookup, ErrorKind,
    Method, RoomLookup, Settings, State, Status, RETRY_AFTER_END_MS, RETRY_OFFLINE_MS,
};
use super::webcast::{LiveEvent, TikTokUser};
use crate::json::Value;
use crate::net::{Http, HttpRequest, WsConnect};
use crate::obj;
use crate::runtime::{Rt, TimerId};
use crate::util::text::clean_text;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// Au-delà, la liste des spectateurs déjà vus (follows) repart de zéro.
const MAX_SEEN_FOLLOWS: usize = 5000;
/// Euler Stream sature (limites gratuites) : nouvel essai dans 2 min.
const RETRY_EULER_LIMIT_MS: u64 = 120_000;
/// Requêtes vers tiktok.com : User-Agent d'un Chrome de bureau (TikTok refuse les autres).
pub const BROWSER_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

pub trait TikTokHost: Send + Sync {
    fn state_changed(&self, state: &State);
    /// Événement brut pour le hub des alertes (source « tiktok »).
    fn alert(&self, payload: Value);
    /// Commentaire, déjà au format commun du chat.
    fn chat(&self, message: Value);
    fn audience(&self, viewers: Option<u64>, likes: Option<u64>);
    fn save_settings(&self, settings: &Settings);
    /// Clé API d'Euler Stream déchiffrée, si l'utilisateur en a enregistré une.
    fn euler_key(&self) -> Option<String> {
        None
    }
}

struct Inner {
    settings: Settings,
    state: State,
    generation: u64,
    failures: u32,
    no_stream_failures: u32,
    started: bool,
    seen_follows: HashSet<String>,
    account_note: Option<String>,
    timer: Option<TimerId>,
    stop: Option<Arc<AtomicBool>>,
    wake: Option<Sender<PageEvent>>,
}

pub struct TikTokService {
    me: Weak<TikTokService>,
    http: Arc<dyn Http>,
    pages: Arc<dyn PageHost>,
    ws: Arc<dyn WsConnect>,
    host: Arc<dyn TikTokHost>,
    rt: Rt,
    timing: fn() -> Timing,
    euler_timing: fn() -> EulerTiming,
    inner: Mutex<Inner>,
}

/// Commentaire TikTok → message de chat commun (comme `tiktokChatToMessage`).
pub fn chat_message(msg_id: Option<u64>, user: &TikTokUser, comment: &str, now: u64, seq: u64) -> Option<Value> {
    let text = clean_text(comment, 500);
    if text.is_empty() {
        return None;
    }
    let login = user.unique_id.clone().unwrap_or_else(|| "tiktok".into());
    let display = clean_text(user.nickname.as_deref().unwrap_or(&login), 40);
    let display = if display.is_empty() { login.clone() } else { display };
    let key = user.id.map(|i| i.to_string()).unwrap_or_else(|| login.clone());
    let id = match msg_id {
        Some(m) => format!("tt-{m}"),
        None => format!("tt-{now}-{seq}"),
    };
    Some(obj! {
        "id" => id, "userId" => format!("tt:{key}"), "login" => clean_text(&login, 40), "displayName" => display, "color" => Value::Null,
        "badges" => Value::Array(Vec::new()), "fragments" => Value::Array(vec![obj! { "type" => "text", "text" => text.clone() }]), "text" => text,
        "timestamp" => now, "highlight" => "none", "replyTo" => Value::Null, "platform" => "tiktok",
    })
}

impl TikTokService {
    pub fn new(stored: &Value, http: Arc<dyn Http>, pages: Arc<dyn PageHost>, ws: Arc<dyn WsConnect>, host: Arc<dyn TikTokHost>, rt: Rt) -> Arc<TikTokService> {
        Self::with_timing(stored, http, pages, ws, host, rt, Timing::default, EulerTiming::default)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_timing(
        stored: &Value,
        http: Arc<dyn Http>,
        pages: Arc<dyn PageHost>,
        ws: Arc<dyn WsConnect>,
        host: Arc<dyn TikTokHost>,
        rt: Rt,
        timing: fn() -> Timing,
        euler_timing: fn() -> EulerTiming,
    ) -> Arc<TikTokService> {
        let settings = Settings::normalize(stored);
        Arc::new_cyclic(|me| TikTokService {
            me: me.clone(),
            http,
            pages,
            ws,
            host,
            rt,
            timing,
            euler_timing,
            inner: Mutex::new(Inner {
                state: State::initial(settings.clone()),
                settings,
                generation: 0,
                failures: 0,
                no_stream_failures: 0,
                started: false,
                seen_follows: HashSet::new(),
                account_note: None,
                timer: None,
                stop: None,
                wake: None,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn state(&self) -> State {
        self.lock().state.clone()
    }

    pub fn settings(&self) -> Settings {
        self.lock().settings.clone()
    }

    fn set_state(&self, f: impl FnOnce(&mut State)) {
        let snapshot = {
            let mut inner = self.lock();
            f(&mut inner.state);
            inner.state.settings = inner.settings.clone();
            inner.state.clone()
        };
        self.host.state_changed(&snapshot);
    }

    fn save_settings(&self, settings: Settings) {
        self.lock().settings = settings.clone();
        self.host.save_settings(&settings);
    }

    fn current(&self, gen: u64) -> bool {
        self.lock().generation == gen
    }

    /// Ferme la connexion en cours et annule le prochain essai.
    fn teardown(&self) -> u64 {
        let (timer, stop, wake, gen) = {
            let mut inner = self.lock();
            inner.generation += 1;
            (inner.timer.take(), inner.stop.take(), inner.wake.take(), inner.generation)
        };
        self.rt.cancel(timer);
        if let Some(s) = stop {
            s.store(true, Ordering::SeqCst);
        }
        if let Some(w) = wake {
            let _ = w.send(PageEvent::Gone("arrêt demandé".into()));
        }
        gen
    }

    /// Au démarrage de Batlay : reprend l'attente du live si elle était active.
    pub fn start(&self) {
        let go = {
            let mut inner = self.lock();
            inner.started = true;
            inner.settings.enabled && inner.settings.username.is_some() && inner.settings.method != Method::Local
        };
        if go {
            self.spawn_attempt();
        }
    }

    /// Bouton « Se connecter ». Faux si le pseudo n'est pas valable.
    pub fn connect(&self, input: &str) -> bool {
        let Some(username) = parse_username(input) else { return false };
        let reset_account = {
            let mut inner = self.lock();
            inner.started = true;
            inner.account_note = None;
            inner.failures = 0;
            inner.seen_follows.clear();
            inner.state.account.as_ref().map(|a| a.username != username).unwrap_or(false)
        };
        if reset_account {
            self.set_state(|s| s.account = None);
        }
        let mut settings = self.settings();
        settings.username = Some(username);
        settings.enabled = true;
        self.save_settings(settings);
        self.spawn_attempt();
        true
    }

    /// Bouton « Déconnecter » : le pseudo est gardé, plus aucun essai.
    pub fn disconnect(&self) {
        let mut settings = self.settings();
        settings.enabled = false;
        self.save_settings(settings);
        self.teardown();
        self.set_state(|s| {
            s.status = Status::Idle;
            s.message = None;
            s.detail = None;
            s.retry_at = None;
            s.since = None;
        });
    }

    /// Session TikTok connectée ou non : si Batlay attend le live, il réessaie tout de suite.
    pub fn set_logged_in(&self, logged_in: bool) {
        let go = {
            let inner = self.lock();
            if inner.state.logged_in == logged_in {
                return;
            }
            inner.started && inner.settings.enabled && inner.settings.username.is_some() && inner.settings.method == Method::Browser && !matches!(inner.state.status, Status::Live | Status::Connecting)
        };
        self.set_state(|s| s.logged_in = logged_in);
        if go {
            self.spawn_attempt();
        }
    }

    /// Clé API d'Euler ajoutée ou retirée : avec Euler, Batlay réessaie tout de suite s'il attendait.
    pub fn set_euler_key(&self, present: bool) {
        let go = {
            let inner = self.lock();
            inner.started && inner.settings.enabled && inner.settings.username.is_some() && inner.settings.method == Method::Euler && !matches!(inner.state.status, Status::Live | Status::Connecting)
        };
        self.set_state(|s| s.euler_key = present);
        if go {
            self.spawn_attempt();
        }
    }

    /// Réglages modifiables en direct : valeur minimale des cadeaux, méthode de connexion.
    /// Changer de méthode rebranche aussitôt (ou arrête tout, pour l'adresse locale).
    pub fn update_settings(&self, patch: &Value) -> Settings {
        let previous = self.settings();
        let mut raw = previous.to_json();
        if !patch.get("giftMinDiamonds").is_null() {
            raw.set("giftMinDiamonds", patch.get("giftMinDiamonds").clone());
        }
        if let Some(mode) = patch.get("mode").as_str().filter(|m| matches!(*m, "browser" | "euler" | "local")) {
            raw.set("mode", Value::from(mode));
        }
        let settings = Settings::normalize(&raw);
        self.save_settings(settings.clone());
        if settings.method != previous.method {
            crate::log::info(&format!("[TikTok] méthode : {}", settings.method.label()));
            {
                let mut inner = self.lock();
                inner.failures = 0;
                inner.no_stream_failures = 0;
            }
            if settings.method == Method::Local {
                self.go_local();
            } else if settings.enabled && settings.username.is_some() && self.lock().started {
                self.spawn_attempt();
            } else {
                self.set_state(|_| {});
            }
        } else {
            self.set_state(|_| {});
        }
        settings
    }

    /// Adresse locale : plus aucune connexion de Batlay au live.
    fn go_local(&self) {
        self.teardown();
        self.host.audience(Some(0), None);
        self.set_state(|s| {
            s.status = Status::Idle;
            s.message = None;
            s.detail = None;
            s.retry_at = None;
            s.since = None;
        });
    }

    pub fn stop(&self) {
        self.teardown();
    }

    fn spawn_attempt(&self) {
        if let Some(me) = self.me.upgrade() {
            self.rt.spawn("tiktok-live", move || me.attempt());
        }
    }

    fn schedule_retry(&self, ms: u64, gen: u64) {
        let weak = self.me.clone();
        let mut inner = self.lock();
        if inner.generation != gen {
            return;
        }
        let old = inner.timer.take();
        self.rt.cancel(old);
        inner.timer = Some(self.rt.after(ms, move || {
            if let Some(me) = weak.upgrade() {
                let go = {
                    let mut inner = me.lock();
                    inner.timer = None;
                    inner.generation == gen && inner.settings.enabled
                };
                if go {
                    me.spawn_attempt();
                }
            }
        }));
    }

    fn lookup_account(&self, username: &str) -> AccountLookup {
        let request = HttpRequest::get(oembed_url(username)).header("Accept", "application/json").header("User-Agent", BROWSER_UA).timeout(Duration::from_secs(8));
        match self.http.send(&request) {
            Ok(r) => parse_oembed(r.status, &if r.status == 200 { r.json().unwrap_or(Value::Null) } else { Value::Null }, username),
            Err(e) => AccountLookup::Unknown(e.to_string()),
        }
    }

    fn fetch_text(&self, url: &str, json: bool) -> Result<(u16, String), String> {
        let accept = if json { "application/json, text/plain, */*" } else { "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8" };
        let request = HttpRequest::get(url).header("User-Agent", BROWSER_UA).header("Accept", accept).header("Accept-Language", "fr-FR,fr;q=0.9,en;q=0.8").timeout(Duration::from_secs(10));
        self.http.send(&request).map(|r| (r.status, String::from_utf8_lossy(&r.body).chars().take(3_000_000).collect())).map_err(|e| e.to_string())
    }

    /// Recherche du live : fenêtre invisible (comme un vrai navigateur, avec la session),
    /// puis page du live, puis API « api-live ». « En live » trouvé n'importe où l'emporte ;
    /// sinon « caché », puis « pas en live », sinon « inconnu ».
    /// `browser_first` faux (méthode Euler) : la fenêtre invisible passe en dernier.
    pub fn find_room(&self, username: &str, browser_first: bool) -> (RoomLookup, Vec<String>) {
        let mut trace = Vec::new();
        let mut hidden: Option<RoomLookup> = None;
        let mut offline: Option<RoomLookup> = None;
        let mut account: Option<Account> = None;
        let mut steps: [(&str, Box<dyn Fn() -> Result<Vec<RoomLookup>, String> + '_>); 3] = [
            (
                "navigateur intégré",
                Box::new(|| {
                    let r = self.pages.probe(username)?;
                    let mut out = Vec::new();
                    if let Some((status, text)) = &r.api {
                        let api = parse_live_room_api(*status, &crate::json::parse(text).unwrap_or(Value::Null));
                        out.push(match api {
                            RoomLookup::Unknown(reason) => {
                                let excerpt: String = text.chars().take(120).collect::<String>().split_whitespace().collect::<Vec<_>>().join(" ");
                                RoomLookup::Unknown(format!("{reason} [{excerpt}]"))
                            }
                            other => other,
                        });
                    }
                    if let Some(sigi) = &r.sigi {
                        out.push(parse_live_page_html(200, &format!("<script id=\"SIGI_STATE\" type=\"application/json\">{sigi}</script>")));
                    }
                    if out.is_empty() || out.iter().all(|x| matches!(x, RoomLookup::Unknown(_))) {
                        let blocked = format!("{} {}", r.href, r.title).to_ascii_lowercase();
                        out.push(RoomLookup::Unknown(if blocked.contains("captcha") || blocked.contains("verify") {
                            format!("vérification anti-robot de TikTok ({})", r.href)
                        } else {
                            format!("page chargée sans informations de live ({}, « {} »)", r.href.chars().take(120).collect::<String>(), r.title.chars().take(60).collect::<String>())
                        }));
                    }
                    Ok(out)
                }),
            ),
            ("page du live", Box::new(|| self.fetch_text(&live_page_url(username), false).map(|(s, t)| vec![parse_live_page_html(s, &t)]))),
            ("API live", Box::new(|| self.fetch_text(&live_api_url(username), true).map(|(s, t)| vec![parse_live_room_api(s, &crate::json::parse(&t).unwrap_or(Value::Null))]))),
        ];
        if !browser_first {
            steps.rotate_left(1);
        }
        for (name, run) in steps.iter() {
            let results = match run() {
                Ok(r) => r,
                Err(e) => {
                    trace.push(format!("{name} : {e}"));
                    continue;
                }
            };
            for result in results {
                let (label, info) = match &result {
                    RoomLookup::Unknown(reason) => {
                        trace.push(format!("{name} : {reason}"));
                        continue;
                    }
                    RoomLookup::Live { info, .. } => ("EN LIVE", info.clone()),
                    RoomLookup::Hidden { info, .. } => ("en live, identifiant caché", info.clone()),
                    RoomLookup::Offline { info, .. } => ("pas en live", info.clone()),
                };
                if account.is_none() {
                    account = result.account().cloned();
                }
                trace.push(format!("{name} : {label} ({info})"));
                match result {
                    RoomLookup::Live { room_id, account: a, info } => return (RoomLookup::Live { room_id, account: a.or(account), info }, trace),
                    r @ RoomLookup::Hidden { .. } => {
                        if hidden.is_none() {
                            hidden = Some(r)
                        }
                    }
                    r => {
                        if offline.is_none() {
                            offline = Some(r)
                        }
                    }
                }
            }
        }
        let best = hidden.or(offline).map(|r| match r {
            RoomLookup::Hidden { private, account: a, info } => RoomLookup::Hidden { private, account: a.or(account.clone()), info },
            RoomLookup::Offline { account: a, info } => RoomLookup::Offline { account: a.or(account.clone()), info },
            other => other,
        });
        (best.unwrap_or_else(|| RoomLookup::Unknown("live non trouvé par Batlay".into())), trace)
    }

    fn attempt(self: Arc<Self>) {
        let (username, method) = {
            let inner = self.lock();
            match (&inner.settings.username, inner.settings.enabled) {
                (Some(u), true) => (u.clone(), inner.settings.method),
                _ => return,
            }
        };
        if method == Method::Local {
            return self.go_local();
        }
        let gen = self.teardown();
        let known = self.lock().state.account.as_ref().map(|a| a.username == username).unwrap_or(false);
        // oEmbed répond « introuvable » aussi pour un compte PRIVÉ (TikTok refuse de
        // l'intégrer) : ce n'est une vraie erreur que si la recherche du live ne voit
        // pas non plus le compte (voir plus bas).
        let mut oembed_not_found = false;
        if !known {
            self.set_state(|s| {
                s.status = Status::Connecting;
                s.message = Some(format!("Vérification du compte @{username}…"));
                s.detail = None;
                s.retry_at = None;
            });
            let found = self.lookup_account(&username);
            if !self.current(gen) {
                return;
            }
            match found {
                AccountLookup::NotFound => {
                    oembed_not_found = true;
                    self.lock().account_note = Some("oEmbed : compte introuvable (normal pour un compte privé)".into());
                }
                AccountLookup::Found(a) => self.set_state(|s| s.account = Some(a)),
                AccountLookup::Unknown(reason) => {
                    let note = format!("Vérification du compte impossible : {reason}");
                    crate::log::warn(&format!("[TikTok] {note} : on continue"));
                    self.lock().account_note = Some(note);
                }
            }
        }
        self.set_state(|s| {
            s.status = Status::Connecting;
            s.message = Some(format!("Recherche du live de @{username}…"));
            s.retry_at = None;
        });
        let mut trace: Vec<String> = self.lock().account_note.clone().into_iter().collect();
        // Euler : recherche par simples requêtes d'abord (aucune fenêtre ouverte si elles suffisent).
        let (found, steps) = self.find_room(&username, method == Method::Browser);
        if !self.current(gen) {
            return;
        }
        crate::log::info(&format!("[TikTok] recherche du live : {}", if steps.is_empty() { format!("{found:?}") } else { steps.join(" ; ") }));
        trace.extend(steps);
        if let Some(seen) = found.account().filter(|a| a.username == username) {
            if self.lock().state.account.is_none() {
                let seen = seen.clone();
                self.set_state(|s| s.account = Some(seen));
            }
        }
        if oembed_not_found && matches!(found, RoomLookup::Unknown(_)) {
            // Ni oEmbed ni la recherche du live ne connaissent ce compte : pseudo erroné.
            self.set_state(|s| {
                s.status = Status::Error;
                s.message = Some(format!("Compte @{username} introuvable sur TikTok : vérifiez le pseudo (celui de l'adresse tiktok.com/@…)."));
                s.detail = (!trace.is_empty()).then(|| trace.join("\n"));
                s.retry_at = None;
                s.since = None;
                s.account = None;
            });
            return;
        }
        let logged_in = self.lock().state.logged_in;
        match &found {
            RoomLookup::Offline { .. } => return self.wait_for_live(&username, &trace, gen),
            // Euler ne reçoit jamais la session TikTok : un live caché aux visiteurs lui est inaccessible.
            RoomLookup::Hidden { private, .. } if method == Method::Euler => {
                let message = format!(
                    "Votre live est bien en cours, mais TikTok le cache aux visiteurs non connectés ({}). La méthode Euler Stream ne peut pas le lire : Batlay n'envoie jamais votre session TikTok à un service tiers. Choisissez « Navigateur intégré » (avec votre compte TikTok connecté), ou passez le compte en public.",
                    if *private { "compte privé" } else { "live restreint : âge, pays, abonnés…" }
                );
                let now = self.rt.now();
                self.set_state(|s| {
                    s.status = Status::Hidden;
                    s.message = Some(message);
                    s.detail = (!trace.is_empty()).then(|| trace.join("\n"));
                    s.retry_at = Some(now + RETRY_OFFLINE_MS);
                    s.since = None;
                });
                return self.schedule_retry(RETRY_OFFLINE_MS, gen);
            }
            RoomLookup::Unknown(_) if method == Method::Euler => {
                trace.push("Euler Stream a besoin de l'identifiant du live, que Batlay n'a pas trouvé".into());
                return self.on_failure(ErrorKind::NoRoom, &username, trace, gen);
            }
            // Avec la session de l'utilisateur, sa propre page de live s'ouvre, même pour un compte privé.
            RoomLookup::Hidden { private, .. } if !logged_in => {
                let message = if *private {
                    "Votre live est bien en cours, mais votre compte TikTok est privé : TikTok cache le live aux visiteurs non connectés. Connectez votre compte TikTok ci-dessous (« Se connecter à TikTok »), ou passez-le en public.\n\nPour le passer en public : TikTok → Profil → ☰ → Paramètres et confidentialité → Confidentialité → Compte privé : désactivé. Ensuite, cliquez sur « Réessayer maintenant »."
                } else {
                    "Votre live est bien en cours, mais TikTok cache son identifiant aux visiteurs non connectés (live restreint : âge, pays, abonnés…). Connectez votre compte TikTok ci-dessous, ou retirez la restriction, puis cliquez sur « Réessayer maintenant »."
                };
                let now = self.rt.now();
                self.set_state(|s| {
                    s.status = Status::Hidden;
                    s.message = Some(message.into());
                    s.detail = (!trace.is_empty()).then(|| trace.join("\n"));
                    s.retry_at = Some(now + RETRY_OFFLINE_MS);
                    s.since = None;
                });
                return self.schedule_retry(RETRY_OFFLINE_MS, gen);
            }
            _ => {}
        }
        if method == Method::Euler {
            if let RoomLookup::Live { room_id, .. } = &found {
                return self.run_euler(username, room_id.clone(), trace, gen);
            }
        }
        self.set_state(|s| s.message = Some(format!("Ouverture de la page du live de @{username} (invisible)…")));
        let lite = self.lock().no_stream_failures % 2 == 0;
        let (tx, rx) = channel();
        let page = match self.pages.open_live_page(&username, lite, tx.clone()) {
            Ok(p) => p,
            Err(e) => {
                trace.push(format!("ouverture de la page : {e}"));
                return self.on_failure(ErrorKind::Other, &username, trace, gen);
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut inner = self.lock();
            if inner.generation != gen {
                drop(inner);
                let mut page = page;
                page.close();
                return;
            }
            inner.stop = Some(stop.clone());
            inner.wake = Some(tx);
        }
        let me = self.clone();
        let me2 = self.clone();
        let user2 = username.clone();
        let mut chat_seq = 0u64;
        let outcome = browser::run(
            page,
            rx,
            stop,
            (self.timing)(),
            move || me.mark_live(gen, &user2, "mode navigateur, fenêtre invisible"),
            move |event| {
                chat_seq += 1;
                me2.handle_event(gen, event, chat_seq);
            },
        );
        if !self.current(gen) {
            return;
        }
        match outcome {
            Outcome::Stopped => {}
            Outcome::NoStream(reason) => {
                self.lock().no_stream_failures += 1;
                trace.push(reason);
                self.on_failure(ErrorKind::NoStream, &username, trace, gen);
            }
            Outcome::Ended => self.ended(&username, "fin du live", gen),
            Outcome::Lost(reason) => self.ended(&username, &reason, gen),
        }
    }

    /// Premier signe du flux : « Branché ».
    fn mark_live(&self, gen: u64, username: &str, via: &str) {
        if !self.current(gen) {
            return;
        }
        {
            let mut inner = self.lock();
            inner.failures = 0;
            inner.no_stream_failures = 0;
        }
        let now = self.rt.now();
        self.set_state(|s| {
            s.status = Status::Live;
            s.message = Some(format!("Branché sur le live de @{username} ({via})."));
            s.detail = None;
            s.retry_at = None;
            s.since = Some(now);
        });
        crate::log::info(&format!("[TikTok] branché sur le live de @{username} ({via})"));
    }

    /// Méthode Euler Stream : signature par le service, puis flux lu par Batlay.
    fn run_euler(self: Arc<Self>, username: String, room_id: String, mut trace: Vec<String>, gen: u64) {
        self.set_state(|s| s.message = Some(format!("Demande de signature à Euler Stream pour le live de @{username}…")));
        let key = self.host.euler_key();
        let signed = match euler::sign_request(&room_id, BROWSER_UA, key.as_deref(), env!("CARGO_PKG_VERSION")) {
            None => Err(SignError::Other(format!("identifiant de live invalide ({room_id})"))),
            Some(request) => match self.http.send(&request) {
                Ok(response) => euler::parse_signed(&response, &room_id, BROWSER_UA),
                Err(e) => Err(SignError::Other(format!("service injoignable : {e}"))),
            },
        };
        if !self.current(gen) {
            return;
        }
        let signed = match signed {
            Ok(s) => s,
            Err(e) => return self.euler_failed(e, &username, trace, gen),
        };
        let Ok(room) = signed.room_id.parse::<u64>() else {
            return self.euler_failed(SignError::Other("identifiant de live invalide".into()), &username, trace, gen);
        };
        trace.push(format!("Euler Stream : connexion signée (room {})", signed.room_id));
        self.set_state(|s| s.message = Some(format!("Connexion au flux du live de @{username} (Euler Stream)…")));
        let mut headers = vec![("User-Agent".to_string(), BROWSER_UA.to_string())];
        if let Some(cookie) = &signed.cookie {
            headers.push(("Cookie".into(), cookie.clone()));
        }
        let conn = match self.ws.connect_with(&signed.ws_url, &headers, euler::MAX_WS_MESSAGE) {
            Ok(c) => c,
            Err(e) => {
                trace.push(format!("flux du live : {e}"));
                self.lock().failures += 1;
                return self.on_failure(ErrorKind::Other, &username, trace, gen);
            }
        };
        let stop = Arc::new(AtomicBool::new(false));
        {
            let mut inner = self.lock();
            if inner.generation != gen {
                drop(inner);
                let mut conn = conn;
                conn.close();
                return;
            }
            inner.stop = Some(stop.clone());
            inner.wake = None;
        }
        let me = self.clone();
        let me2 = self.clone();
        let user2 = username.clone();
        let mut chat_seq = 0u64;
        let outcome = euler::run(
            conn,
            room,
            &stop,
            (self.euler_timing)(),
            move || me.mark_live(gen, &user2, "Euler Stream"),
            move |event| {
                chat_seq += 1;
                me2.handle_event(gen, event, chat_seq);
            },
        );
        if !self.current(gen) {
            return;
        }
        match outcome {
            Outcome::Stopped => {}
            Outcome::NoStream(reason) => {
                trace.push(reason);
                self.lock().failures += 1;
                self.on_failure(ErrorKind::Other, &username, trace, gen);
            }
            Outcome::Ended => self.ended(&username, "fin du live", gen),
            Outcome::Lost(reason) => self.ended(&username, &reason, gen),
        }
    }

    /// Euler refuse ou ne répond pas : message clair, nouvel essai seulement s'il peut aboutir.
    fn euler_failed(&self, error: SignError, username: &str, mut trace: Vec<String>, gen: u64) {
        let failures = {
            let mut inner = self.lock();
            inner.stop = None;
            inner.wake = None;
            if matches!(error, SignError::Other(_)) {
                inner.failures += 1;
            }
            inner.failures
        };
        let retry = match &error {
            SignError::RateLimited(_) => Some(RETRY_EULER_LIMIT_MS),
            SignError::KeyRefused(_) | SignError::Premium(_) => None,
            SignError::Other(_) => retry_delay(ErrorKind::Other, failures),
        };
        let message = error.message();
        trace.push(format!("Euler Stream : {message}"));
        crate::log::warn(&format!("[TikTok/Euler] @{username} : {message}"));
        let detail: String = trace.join("\n").chars().take(2000).collect();
        let now = self.rt.now();
        self.set_state(|s| {
            s.status = if retry.is_some() { Status::Waiting } else { Status::Error };
            s.message = Some(message);
            s.detail = (!detail.is_empty()).then_some(detail);
            s.retry_at = retry.map(|ms| now + ms);
            s.since = None;
        });
        if let Some(ms) = retry {
            self.schedule_retry(ms, gen);
        }
    }

    fn handle_event(&self, gen: u64, event: LiveEvent, seq: u64) {
        let min = {
            let inner = self.lock();
            if inner.generation != gen {
                return;
            }
            inner.settings.gift_min_diamonds
        };
        match event {
            LiveEvent::Follow { user } => {
                // Un même spectateur qui se désabonne puis se réabonne : une seule alerte par live.
                if let Some(key) = user.key() {
                    let mut inner = self.lock();
                    if inner.seen_follows.contains(&key) {
                        return;
                    }
                    if inner.seen_follows.len() >= MAX_SEEN_FOLLOWS {
                        inner.seen_follows.clear();
                    }
                    inner.seen_follows.insert(key);
                }
                self.host.alert(obj! { "kind" => "follow", "user" => user.display_name() });
            }
            // Les abonnements TikTok s'appellent « Super Fan » dans les lives.
            LiveEvent::SuperFanJoin { user } => self.host.alert(obj! { "kind" => "subscribe", "user" => user.display_name() }),
            LiveEvent::Gift { user, name, diamonds, repeat_count, repeat_end, gift_type } => {
                // Cadeau « en série » (type 1) : une seule alerte, à la fin de la série.
                let last = gift_type != Some(1) || repeat_end;
                if !last {
                    return;
                }
                let count = repeat_count.max(1);
                if diamonds.is_some_and(|d| d.saturating_mul(count) < min) {
                    return;
                }
                self.host.alert(obj! { "kind" => "tiktokGift", "user" => user.display_name(), "amount" => count, "gift" => name });
            }
            LiveEvent::Chat { msg_id, user, comment } => {
                if let Some(m) = chat_message(msg_id, &user, &comment, crate::util::now_ms(), seq) {
                    self.host.chat(m);
                }
            }
            LiveEvent::Viewers(v) => self.host.audience(Some(v), None),
            LiveEvent::Likes(l) => self.host.audience(None, Some(l)),
            LiveEvent::StreamEnd => {}
        }
    }

    fn ended(&self, username: &str, reason: &str, gen: u64) {
        let gen = {
            let mut inner = self.lock();
            if inner.generation != gen || !inner.settings.enabled {
                return;
            }
            inner.generation += 1;
            inner.stop = None;
            inner.wake = None;
            inner.seen_follows.clear();
            inner.generation
        };
        self.host.audience(Some(0), None);
        crate::log::info(&format!("[TikTok] {reason}"));
        let now = self.rt.now();
        self.set_state(|s| {
            s.status = Status::Waiting;
            s.message = Some(format!("Live de @{username} terminé ou coupé. Batlay se rebranchera au prochain live."));
            s.detail = None;
            s.retry_at = Some(now + RETRY_AFTER_END_MS);
            s.since = None;
        });
        self.schedule_retry(RETRY_AFTER_END_MS, gen);
    }

    fn wait_for_live(&self, username: &str, trace: &[String], gen: u64) {
        let has_account = self.lock().state.account.is_some();
        let message = if has_account { "Pas en live pour le moment : Batlay se branchera tout seul dès le début du live.".to_string() } else { error_message(ErrorKind::Offline, username) };
        let now = self.rt.now();
        self.set_state(|s| {
            s.status = Status::Waiting;
            s.message = Some(message);
            s.detail = (!trace.is_empty()).then(|| trace.join("\n"));
            s.retry_at = Some(now + RETRY_OFFLINE_MS);
            s.since = None;
        });
        self.schedule_retry(RETRY_OFFLINE_MS, gen);
    }

    fn on_failure(&self, kind: ErrorKind, username: &str, trace: Vec<String>, gen: u64) {
        let (failures, has_account) = {
            let mut inner = self.lock();
            if kind == ErrorKind::Other {
                inner.failures += 1;
            }
            inner.stop = None;
            inner.wake = None;
            (inner.failures, inner.state.account.is_some())
        };
        let delay = retry_delay(kind, failures);
        let message = if has_account && matches!(kind, ErrorKind::Offline | ErrorKind::NoRoom) {
            "Pas en live pour le moment : Batlay se branchera tout seul dès le début du live.".to_string()
        } else {
            error_message(kind, username)
        };
        let detail: String = trace.join("\n").chars().take(2000).collect();
        crate::log::warn(&format!("[TikTok] {kind:?} : {detail}"));
        let now = self.rt.now();
        match delay {
            None => self.set_state(|s| {
                s.status = Status::Error;
                s.message = Some(message);
                s.detail = (!detail.is_empty()).then_some(detail);
                s.retry_at = None;
                s.since = None;
            }),
            Some(ms) => {
                self.set_state(|s| {
                    s.status = Status::Waiting;
                    s.message = Some(message);
                    s.detail = (!detail.is_empty()).then_some(detail);
                    s.retry_at = Some(now + ms);
                    s.since = None;
                });
                self.schedule_retry(ms, gen);
            }
        }
    }

    /// Bouton « Diagnostic » : chaque étape, sans ouvrir de connexion au live.
    pub fn diagnose(&self, version: &str) -> Value {
        let mut steps: Vec<(String, &str, String)> = Vec::new();
        let settings = self.settings();
        steps.push(("Décodeur du flux TikTok".into(), "ok", "intégré à Batlay (Rust), aucun service tiers".into()));
        let Some(username) = settings.username.clone() else {
            steps.push(("Pseudo".into(), "fail", "aucun pseudo enregistré : entrez-le dans la carte TikTok.".into()));
            return self.diagnostic_json(steps, version);
        };
        steps.push(("Pseudo".into(), "ok", format!("@{username}{}", if settings.enabled { "" } else { " (Batlay est déconnecté : cliquez sur « Se connecter »)" })));
        match self.lookup_account(&username) {
            AccountLookup::Found(a) => steps.push(("Compte".into(), "ok", format!("trouvé : {}", a.display_name))),
            AccountLookup::NotFound => steps.push(("Compte".into(), "warn", "non trouvé par oEmbed : normal pour un compte privé ; sinon, vérifiez le pseudo (celui de tiktok.com/@…).".into())),
            AccountLookup::Unknown(r) => steps.push(("Compte".into(), "warn", format!("vérification impossible ({r})"))),
        }
        let method = settings.method;
        steps.push(("Méthode".into(), "ok", method.label().into()));
        let logged_in = self.state().logged_in || self.pages.logged_in();
        match method {
            Method::Browser => steps.push((
                "Session TikTok dans Batlay".into(),
                if logged_in { "ok" } else { "skip" },
                if logged_in { "connectée".into() } else { "non connectée (nécessaire seulement pour un compte privé)".into() },
            )),
            Method::Euler => {
                steps.push((
                    "Clé API Euler".into(),
                    if self.state().euler_key { "ok" } else { "skip" },
                    if self.state().euler_key { "enregistrée (chiffrée par Windows)".into() } else { "aucune : limites gratuites d'Euler Stream".into() },
                ));
                steps.push(("Session TikTok".into(), "skip", "jamais envoyée à Euler Stream : seuls les lives visibles sans compte sont lisibles".into()));
            }
            Method::Local => steps.push((
                "Adresse locale".into(),
                "ok",
                "Batlay ne se connecte pas au live : votre outil (TikFinity, Streamer.bot…) envoie les événements à l'adresse locale".into(),
            )),
        }
        let (found, trace) = self.find_room(&username, method == Method::Browser);
        let trace = if trace.is_empty() { String::new() } else { format!(" · {}", trace.join(" | ")) };
        steps.push(match found {
            RoomLookup::Live { room_id, .. } => ("Live".into(), "ok", format!("en cours, identifiant {room_id}{trace}")),
            RoomLookup::Offline { .. } => ("Live".into(), "warn", format!("pas en live d'après TikTok{trace}")),
            RoomLookup::Hidden { .. } if method == Method::Euler => ("Live".into(), "fail", format!("en cours mais caché aux visiteurs : Euler Stream ne peut pas le lire (choisissez « Navigateur intégré »){trace}")),
            RoomLookup::Hidden { .. } if logged_in => ("Live".into(), "ok", format!("en cours, caché aux visiteurs : le mode navigateur le lit avec votre session TikTok{trace}")),
            RoomLookup::Hidden { private, .. } => (
                "Live".into(),
                "fail",
                format!("en cours mais identifiant caché{}{trace}", if private { " (compte privé : connectez votre compte TikTok, ou passez-le en public)" } else { "" }),
            ),
            RoomLookup::Unknown(reason) => ("Live".into(), "fail", format!("introuvable ({reason}){trace}")),
        });
        let st = self.state();
        if method == Method::Local {
            steps.push(("Dernier état de la connexion".into(), "skip", "aucune connexion de Batlay (adresse locale)".into()));
            return self.diagnostic_json(steps, version);
        }
        let status = match st.status {
            Status::Live => "ok",
            Status::Error | Status::Hidden => "fail",
            Status::Idle => "skip",
            _ => "warn",
        };
        let detail = format!(
            "{}{}{}",
            st.status.label(),
            st.message.as_deref().map(|m| format!(" · {m}")).unwrap_or_default(),
            st.detail.as_deref().map(|d| format!(" · {}", d.replace('\n', " | "))).unwrap_or_default()
        );
        steps.push(("Dernier état de la connexion".into(), status, detail));
        self.diagnostic_json(steps, version)
    }

    fn diagnostic_json(&self, steps: Vec<(String, &str, String)>, version: &str) -> Value {
        let at = self.rt.now();
        let report = format_diagnostic(at, &steps, version);
        obj! {
            "diagnostic" => obj! { "at" => at, "steps" => Value::Array(steps.into_iter().map(|(l, s, d)| obj! { "label" => l, "status" => s, "detail" => d }).collect()) },
            "report" => report,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{ManualClock, ManualTimers, ThreadSpawner};
    use crate::tiktok::browser::tests::{fast, FakePage, WEBCAST};
    use crate::tiktok::browser::{LivePage, ProbeResult};
    use crate::tiktok::webcast::tests_fixtures::{FRAME_END, FRAME_EVENTS};
    use crate::net::{HttpResponse, NetError, WsConn, WsEvent};
    use crate::tiktok::euler::tests::{fake as fake_conn, signed_response};
    use crate::twitch::chat::tests::{wait_until, FakeHttp};
    use crate::util::encoding::base64_decode;
    use std::collections::VecDeque;

    /// Requêtes : Euler Stream à part (réponse binaire + en-têtes), le reste par FakeHttp.
    struct Net {
        tiktok: Arc<FakeHttp>,
        euler: Mutex<Option<HttpResponse>>,
        euler_calls: Mutex<Vec<HttpRequest>>,
    }
    impl Http for Net {
        fn send(&self, r: &HttpRequest) -> Result<HttpResponse, NetError> {
            if r.url.starts_with(euler::EULER_API) {
                self.euler_calls.lock().unwrap().push(r.clone());
                return self.euler.lock().unwrap().clone().ok_or_else(|| NetError::Protocol("injoignable".into()));
            }
            self.tiktok.send(r)
        }
    }

    /// Adresse et en-têtes d'une WebSocket ouverte.
    type Opened = (String, Vec<(String, String)>);

    #[derive(Default)]
    struct Ws {
        conns: Mutex<VecDeque<Box<dyn WsConn>>>,
        opened: Mutex<Vec<Opened>>,
    }
    impl WsConnect for Ws {
        fn connect(&self, url: &str) -> Result<Box<dyn WsConn>, NetError> {
            self.connect_with(url, &[], 0)
        }
        fn connect_with(&self, url: &str, headers: &[(String, String)], _max: usize) -> Result<Box<dyn WsConn>, NetError> {
            self.opened.lock().unwrap().push((url.to_string(), headers.to_vec()));
            self.conns.lock().unwrap().pop_front().ok_or_else(|| NetError::Protocol("refusé".into()))
        }
    }

    fn fast_euler() -> EulerTiming {
        EulerTiming { first_frame: Duration::from_millis(200), heartbeat: Duration::from_millis(50) }
    }

    #[derive(Default)]
    struct Pages {
        probe: Mutex<Option<ProbeResult>>,
        senders: Mutex<Vec<(bool, Sender<PageEvent>)>>,
        closed: Arc<Mutex<u32>>,
        logged_in: AtomicBool,
    }
    impl PageHost for Pages {
        fn open_live_page(&self, _u: &str, lite: bool, events: Sender<PageEvent>) -> Result<Box<dyn LivePage>, String> {
            self.senders.lock().unwrap().push((lite, events));
            Ok(Box::new(FakePage { closed: self.closed.clone(), visible: Arc::new(AtomicBool::new(false)) }))
        }
        fn probe(&self, _u: &str) -> Result<ProbeResult, String> {
            self.probe.lock().unwrap().clone().ok_or_else(|| "fenêtre indisponible".into())
        }
        fn logged_in(&self) -> bool {
            self.logged_in.load(Ordering::SeqCst)
        }
    }
    impl Pages {
        fn send(&self, i: usize, e: PageEvent) {
            let tx = self.senders.lock().unwrap()[i].1.clone();
            tx.send(e).unwrap();
        }
        fn opened(&self) -> usize {
            self.senders.lock().unwrap().len()
        }
    }

    #[derive(Default)]
    struct Host {
        states: Mutex<Vec<State>>,
        alerts: Mutex<Vec<Value>>,
        chats: Mutex<Vec<Value>>,
        audience: Mutex<Vec<(Option<u64>, Option<u64>)>>,
        saved: Mutex<Vec<Settings>>,
        euler_key: Mutex<Option<String>>,
    }
    impl TikTokHost for Host {
        fn state_changed(&self, s: &State) {
            self.states.lock().unwrap().push(s.clone());
        }
        fn alert(&self, p: Value) {
            self.alerts.lock().unwrap().push(p);
        }
        fn chat(&self, m: Value) {
            self.chats.lock().unwrap().push(m);
        }
        fn audience(&self, v: Option<u64>, l: Option<u64>) {
            self.audience.lock().unwrap().push((v, l));
        }
        fn save_settings(&self, s: &Settings) {
            self.saved.lock().unwrap().push(s.clone());
        }
        fn euler_key(&self) -> Option<String> {
            self.euler_key.lock().unwrap().clone()
        }
    }

    const OEMBED: &str = r#"{"author_url":"https://www.tiktok.com/@adil.bli","author_name":"Adil"}"#;
    const LIVE_API: &str = r#"{"data":{"liveRoom":{"status":2},"user":{"roomId":"7351234567890123456","uniqueId":"adil.bli"}},"statusCode":0}"#;
    const HIDDEN_API: &str = r#"{"data":{"liveRoom":{"status":2},"user":{"roomId":"","secret":true,"uniqueId":"adil.bli","status":2}},"statusCode":0}"#;
    const OFFLINE_API: &str = r#"{"data":{"liveRoom":{"status":4},"user":{"roomId":"","uniqueId":"adil.bli"}},"statusCode":0}"#;

    fn setup(api: &str) -> (Arc<TikTokService>, Arc<Pages>, Arc<Host>, Arc<FakeHttp>, Arc<ManualTimers>) {
        let (svc, pages, host, http, timers, _net, _ws) = setup_full(api, &Value::Null);
        (svc, pages, host, http, timers)
    }

    type Full = (Arc<TikTokService>, Arc<Pages>, Arc<Host>, Arc<FakeHttp>, Arc<ManualTimers>, Arc<Net>, Arc<Ws>);

    fn setup_full(api: &str, stored: &Value) -> Full {
        let http = Arc::new(FakeHttp::default());
        http.route("oembed", 200, OEMBED);
        http.route("/live", 200, "<html></html>");
        http.route("api-live", 200, api);
        let pages = Arc::new(Pages::default());
        *pages.probe.lock().unwrap() = Some(ProbeResult { api: Some((200, api.to_string())), sigi: None, href: "https://www.tiktok.com/@adil.bli/live".into(), title: "Live".into() });
        let host = Arc::new(Host::default());
        let timers = ManualTimers::new();
        let rt = Rt { clock: ManualClock::new(1000), timers: timers.clone(), spawner: Arc::new(ThreadSpawner) };
        let net = Arc::new(Net { tiktok: http.clone(), euler: Mutex::new(None), euler_calls: Mutex::new(Vec::new()) });
        let ws = Arc::new(Ws::default());
        let svc = TikTokService::with_timing(stored, net.clone(), pages.clone(), ws.clone(), host.clone(), rt, fast, fast_euler);
        (svc, pages, host, http, timers, net, ws)
    }

    #[test]
    fn live_flow_alerts_chat_audience_and_end() {
        let (svc, pages, host, _http, timers) = setup(LIVE_API);
        assert!(!svc.connect("pas valable !"));
        assert!(svc.connect("@Adil.Bli"));
        assert_eq!(host.saved.lock().unwrap()[0].username.as_deref(), Some("adil.bli"));
        wait_until("page ouverte", || pages.opened() == 1);
        assert!(pages.senders.lock().unwrap()[0].0, "premier essai sans vidéo");
        pages.send(0, PageEvent::SocketCreated { id: "w".into(), url: WEBCAST.into() });
        wait_until("en live", || svc.state().status == Status::Live);
        assert_eq!(svc.state().account.unwrap().display_name, "Adil");
        pages.send(0, PageEvent::Frame { id: "w".into(), data: base64_decode(FRAME_EVENTS).unwrap() });
        pages.send(0, PageEvent::Frame { id: "w".into(), data: base64_decode(FRAME_EVENTS).unwrap() });
        wait_until("événements", || host.chats.lock().unwrap().len() == 2);
        let alerts = host.alerts.lock().unwrap().clone();
        let kinds: Vec<String> = alerts.iter().map(|a| format!("{}:{}", a.get("kind").as_str().unwrap(), a.get("user").as_str().unwrap_or("?"))).collect();
        assert_eq!(
            kinds,
            vec!["follow:Rémi", "tiktokGift:Sofia", "tiktokGift:Max", "subscribe:Nyx", "tiktokGift:Sofia", "tiktokGift:Max", "subscribe:Nyx"],
            "follow répété ignoré, série de cadeaux : une alerte à la fin"
        );
        assert_eq!(alerts[1].to_json(), r#"{"kind":"tiktokGift","user":"Sofia","amount":5,"gift":"Rose"}"#);
        let chat = host.chats.lock().unwrap()[0].clone();
        assert_eq!((chat.get("id").as_str(), chat.get("userId").as_str(), chat.get("platform").as_str()), (Some("tt-7351234567890123456"), Some("tt:6812345678901234567"), Some("tiktok")));
        assert!(host.audience.lock().unwrap().contains(&(Some(86), None)));
        assert!(host.audience.lock().unwrap().contains(&(None, Some(4210))));
        pages.send(0, PageEvent::Frame { id: "w".into(), data: base64_decode(FRAME_END).unwrap() });
        wait_until("fin du live", || svc.state().status == Status::Waiting);
        assert_eq!(timers.pending(), vec![RETRY_AFTER_END_MS]);
        assert!(host.audience.lock().unwrap().contains(&(Some(0), None)));
        assert_eq!(*pages.closed.lock().unwrap(), 1);
        svc.disconnect();
        assert_eq!((svc.state().status, timers.count()), (Status::Idle, 0));
    }

    #[test]
    fn disconnect_keeps_username_and_restart_follows_enabled() {
        let (svc, pages, host, http, timers) = setup(LIVE_API);
        svc.connect("adil.bli");
        wait_until("page", || pages.opened() == 1);
        svc.disconnect();
        let st = svc.state();
        assert_eq!((st.status, st.settings.username.as_deref(), st.settings.enabled), (Status::Idle, Some("adil.bli"), false));
        assert_eq!(timers.count(), 0, "plus aucun essai programmé");
        let saved = host.saved.lock().unwrap().last().cloned().unwrap();
        assert!(!saved.enabled, "« Déconnecter » est enregistré");
        // Redémarrage de Batlay avec ce réglage : rien ne se rebranche.
        svc.start();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(pages.opened(), 1);
        // Réglage actif (« Se connecter » sans « Déconnecter ») : reprise au démarrage.
        let rt = Rt { clock: ManualClock::new(1000), timers: ManualTimers::new(), spawner: Arc::new(ThreadSpawner) };
        let saved_on = crate::json::parse(r#"{"username":"adil.bli","enabled":true,"giftMinDiamonds":1}"#).unwrap();
        let again = TikTokService::with_timing(&saved_on, http.clone(), pages.clone(), Arc::new(Ws::default()), host.clone(), rt, fast, fast_euler);
        again.start();
        wait_until("reprise", || pages.opened() == 2);
        again.stop();
    }

    #[test]
    fn gift_minimum() {
        let (svc, pages, host, _http, _t) = setup(LIVE_API);
        svc.update_settings(&crate::json::parse(r#"{"giftMinDiamonds":10}"#).unwrap());
        svc.connect("adil.bli");
        wait_until("page", || pages.opened() == 1);
        pages.send(0, PageEvent::SocketCreated { id: "w".into(), url: WEBCAST.into() });
        pages.send(0, PageEvent::Frame { id: "w".into(), data: base64_decode(FRAME_EVENTS).unwrap() });
        wait_until("chat", || host.chats.lock().unwrap().len() == 1);
        let gifts: Vec<String> = host.alerts.lock().unwrap().iter().filter(|a| a.get("kind").as_str() == Some("tiktokGift")).map(|a| a.get("gift").as_str().unwrap().to_string()).collect();
        assert_eq!(gifts, vec!["Lion"], "5 roses = 5 diamants < 10");
        svc.stop();
    }

    #[test]
    fn offline_waits_and_private_needs_login() {
        let (svc, pages, _host, http, timers) = setup(OFFLINE_API);
        svc.connect("adil.bli");
        wait_until("attente", || svc.state().status == Status::Waiting);
        assert_eq!(svc.state().message.as_deref(), Some("Pas en live pour le moment : Batlay se branchera tout seul dès le début du live."));
        assert_eq!((timers.pending(), pages.opened()), (vec![RETRY_OFFLINE_MS], 0));
        // Le live commence, mais le compte est privé et non connecté.
        http.route("api-live", 200, HIDDEN_API);
        *pages.probe.lock().unwrap() = Some(ProbeResult { api: Some((200, HIDDEN_API.into())), ..Default::default() });
        timers.fire_last();
        wait_until("caché", || svc.state().status == Status::Hidden);
        assert!(svc.state().message.unwrap().contains("compte TikTok est privé"));
        assert_eq!(pages.opened(), 0);
        // Connexion du compte : nouvel essai immédiat, la page s'ouvre.
        svc.set_logged_in(true);
        wait_until("page ouverte", || pages.opened() == 1);
        svc.stop();
    }

    #[test]
    fn no_stream_alternates_lite_and_full() {
        let (svc, pages, _host, _http, timers) = setup(LIVE_API);
        svc.connect("adil.bli");
        wait_until("échec sans flux", || svc.state().status == Status::Waiting && timers.count() == 1);
        assert!(svc.state().message.unwrap().contains("Afficher la page TikTok"));
        timers.fire_last();
        wait_until("deuxième page", || pages.opened() == 2);
        assert_eq!(pages.senders.lock().unwrap().iter().map(|s| s.0).collect::<Vec<_>>(), vec![true, false]);
        pages.send(1, PageEvent::SocketCreated { id: "w".into(), url: WEBCAST.into() });
        wait_until("en live", || svc.state().status == Status::Live);
        svc.stop();
        wait_until("page fermée", || *pages.closed.lock().unwrap() == 2);
    }

    #[test]
    fn unknown_account_is_an_error_without_retry() {
        let (svc, pages, _host, http, timers) = setup(LIVE_API);
        http.route("oembed", 404, "");
        // La recherche du live ne connaît pas non plus ce compte.
        http.route("/live", 404, "");
        http.route("api-live", 200, r#"{"statusCode":19881007,"data":{}}"#);
        *pages.probe.lock().unwrap() = None;
        svc.connect("nobody");
        wait_until("erreur", || svc.state().status == Status::Error);
        assert!(svc.state().message.unwrap().contains("introuvable"));
        assert_eq!((timers.count(), pages.opened()), (0, 0));
    }

    fn euler_settings() -> Value {
        crate::json::parse(r#"{"mode":"euler"}"#).unwrap()
    }

    #[test]
    fn euler_flow_signs_connects_and_never_sends_the_session() {
        let (svc, pages, host, _http, timers, net, ws) = setup_full(LIVE_API, &euler_settings());
        *host.euler_key.lock().unwrap() = Some("euler_ABCDEF12".into());
        *net.euler.lock().unwrap() = Some(signed_response("wss://webcast-ws.tiktok.com/webcast/im/push/v2/"));
        let frames = vec![Ok(WsEvent::Binary(base64_decode(FRAME_EVENTS).unwrap())), Ok(WsEvent::Idle), Ok(WsEvent::Binary(base64_decode(FRAME_END).unwrap()))];
        let (conn, sent, closed) = fake_conn(frames);
        ws.conns.lock().unwrap().push_back(conn);
        svc.set_logged_in(true);
        svc.connect("adil.bli");
        wait_until("fin du live", || svc.state().status == Status::Waiting && *closed.lock().unwrap());
        assert_eq!(timers.pending(), vec![RETRY_AFTER_END_MS]);
        assert_eq!(host.chats.lock().unwrap().len(), 1);
        assert!(host.alerts.lock().unwrap().iter().any(|a| a.get("kind").as_str() == Some("follow")));
        assert!(host.states.lock().unwrap().iter().any(|s| s.status == Status::Live && s.message.as_deref().is_some_and(|m| m.contains("Euler Stream"))));
        // Signature : identifiant du live trouvé par Batlay, clé dans un en-tête.
        let call = net.euler_calls.lock().unwrap()[0].clone();
        assert!(call.url.contains("/webcast/rooms/7351234567890123456/connect"));
        assert!(call.headers.iter().any(|(k, v)| k == "x-api-key" && v == "euler_ABCDEF12"));
        // Jamais la session TikTok : ni vers Euler, ni ailleurs que le cookie anonyme d'Euler.
        assert!(!call.headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("cookie") || k.eq_ignore_ascii_case("x-cookie-header") || k.eq_ignore_ascii_case("x-oauth-token")));
        let (url, headers) = ws.opened.lock().unwrap()[0].clone();
        assert!(url.starts_with("wss://webcast-ws.tiktok.com/webcast/im/push/v2/?") && url.contains("room_id=7701654510762380054"));
        assert!(headers.contains(&("Cookie".into(), "ttwid=1%7CabcDEF".into())));
        assert!(headers.contains(&("User-Agent".into(), BROWSER_UA.into())));
        assert_eq!(pages.opened(), 0, "aucune fenêtre invisible avec Euler");
        assert!(!sent.lock().unwrap().is_empty(), "entrée dans le live envoyée");
        svc.stop();
    }

    #[test]
    fn euler_cannot_read_a_hidden_live() {
        let (svc, pages, _host, _http, timers, net, _ws) = setup_full(HIDDEN_API, &euler_settings());
        svc.set_logged_in(true);
        svc.connect("adil.bli");
        wait_until("caché", || svc.state().status == Status::Hidden);
        assert!(svc.state().message.unwrap().contains("Navigateur intégré"));
        assert_eq!((pages.opened(), net.euler_calls.lock().unwrap().len(), timers.pending()), (0, 0, vec![RETRY_OFFLINE_MS]));
        let d = svc.diagnose("0.5.1");
        let labels: Vec<String> = d.path(&["diagnostic", "steps"]).items().iter().map(|s| format!("{}:{}", s.get("label").as_str().unwrap(), s.get("status").as_str().unwrap())).collect();
        assert!(labels.contains(&"Clé API Euler:skip".to_string()) && labels.contains(&"Live:fail".to_string()), "{labels:?}");
        svc.stop();
    }

    #[test]
    fn euler_errors_rate_limit_then_key() {
        let (svc, _pages, _host, _http, timers, net, ws) = setup_full(LIVE_API, &euler_settings());
        *net.euler.lock().unwrap() = Some(crate::net::fake_response(429, r#"{"code":429,"message":"Rate limit exceeded."}"#));
        svc.connect("adil.bli");
        wait_until("limite", || svc.state().status == Status::Waiting);
        assert!(svc.state().message.unwrap().contains("limite"));
        assert_eq!(timers.pending(), vec![RETRY_EULER_LIMIT_MS]);
        *net.euler.lock().unwrap() = Some(crate::net::fake_response(401, r#"{"message":"API key required"}"#));
        timers.fire_last();
        wait_until("clé refusée", || svc.state().status == Status::Error);
        assert!(svc.state().message.unwrap().contains("clé API"));
        assert_eq!(timers.count(), 0, "sans clé valable, pas de nouvel essai");
        assert!(ws.opened.lock().unwrap().is_empty());
        // Clé ajoutée : nouvel essai aussitôt.
        *net.euler.lock().unwrap() = None;
        svc.set_euler_key(true);
        wait_until("nouvel essai", || net.euler_calls.lock().unwrap().len() == 3);
        svc.stop();
    }

    #[test]
    fn local_address_mode_never_connects_and_switching_back_reconnects() {
        let (svc, pages, host, _http, timers, net, _ws) = setup_full(LIVE_API, &Value::Null);
        svc.connect("adil.bli");
        wait_until("page", || pages.opened() == 1);
        let settings = svc.update_settings(&crate::json::parse(r#"{"mode":"local"}"#).unwrap());
        assert_eq!(settings.method, Method::Local);
        assert_eq!(host.saved.lock().unwrap().last().unwrap().method, Method::Local);
        wait_until("page fermée", || *pages.closed.lock().unwrap() == 1);
        assert_eq!((svc.state().status, timers.count()), (Status::Idle, 0));
        svc.start();
        assert!(svc.connect("adil.bli"));
        std::thread::sleep(Duration::from_millis(80));
        assert_eq!((pages.opened(), svc.state().status, net.euler_calls.lock().unwrap().len()), (1, Status::Idle, 0));
        let d = svc.diagnose("0.5.1");
        assert!(d.get("report").as_str().unwrap().contains("[OK] Adresse locale"));
        // Mode inconnu : ignoré.
        assert_eq!(svc.update_settings(&crate::json::parse(r#"{"mode":"pirate"}"#).unwrap()).method, Method::Local);
        // Retour au navigateur : rebranché aussitôt.
        svc.update_settings(&crate::json::parse(r#"{"mode":"browser"}"#).unwrap());
        wait_until("page rouverte", || pages.opened() == 2);
        svc.stop();
    }

    #[test]
    fn private_account_refused_by_oembed_still_connects_with_session() {
        // Cas réel (octobre 2026) : oEmbed répond 400 pour un compte privé, alors que le
        // live est en cours et que la session TikTok de l'utilisateur permet de le lire.
        let (svc, pages, _host, http, _timers) = setup(HIDDEN_API);
        http.route("oembed", 400, "");
        svc.set_logged_in(true);
        svc.connect("adil.bli");
        wait_until("page ouverte", || pages.opened() == 1);
        assert_ne!(svc.state().status, Status::Error);
        let d = svc.diagnose("0.5.0");
        let compte = d.path(&["diagnostic", "steps"]).items().iter().find(|s| s.get("label").as_str() == Some("Compte")).cloned().unwrap();
        assert_eq!(compte.get("status").as_str(), Some("warn"));
        svc.stop();
    }

    #[test]
    fn diagnostic_report() {
        let (svc, _pages, _host, _http, _t) = setup(HIDDEN_API);
        svc.connect("adil.bli");
        wait_until("caché", || svc.state().status == Status::Hidden);
        let d = svc.diagnose("0.5.0");
        let steps: Vec<String> = d.path(&["diagnostic", "steps"]).items().iter().map(|s| format!("{}:{}", s.get("label").as_str().unwrap(), s.get("status").as_str().unwrap())).collect();
        assert_eq!(steps, vec!["Décodeur du flux TikTok:ok", "Pseudo:ok", "Compte:ok", "Méthode:ok", "Session TikTok dans Batlay:skip", "Live:fail", "Dernier état de la connexion:fail"]);
        assert!(d.get("report").as_str().unwrap().contains("[ÉCHEC] Live : en cours mais identifiant caché (compte privé"));
        svc.stop();
    }
}
