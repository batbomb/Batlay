//! Assemblage de Batlay (l'équivalent de l'ancien `electron/main/index.ts`, sans
//! les fenêtres) : démarre les serveurs et les services, relie leurs événements,
//! et répond à chaque commande du tableau de bord (`invoke`).
//!
//! Le tableau de bord est de confiance LIMITÉE : chaque commande est dans une
//! liste fermée, chaque argument est revalidé ici. Aucune commande ne reçoit
//! de port, de chemin, de jeton ni d'URL arbitraire.

use crate::alerts::hub::{AlertHub, AlertSink, ReceiveResult};
use crate::alerts::media::MediaStore;
use crate::alerts::model::{AlertEvent, AlertSource, ALERTS_HOOK_PREFIX, ALERTS_PAGE_PATH};
use crate::config::{self, SecretBox, Store};
use crate::hosts_alias::{self, AliasStatus};
use crate::json::Value;
use crate::live::{LiveHub, LiveSink};
use crate::loopback::LOCAL_ALIAS_HOSTNAME;
use crate::net::client::{Connector, HttpClient, HttpRequest};
use crate::net::{Http, WsConnector};
use crate::obj;
use crate::overlay_server::{OverlayHooks, OverlayServer};
use crate::runtime::Rt;
use crate::spotify::Spotify;
use crate::system_media::{PowerShell, SystemMedia};
use crate::tiktok::browser::PageHost;
use crate::tiktok::model::{Settings as TikTokSettings, State as TikTokState, Status as TikTokStatus};
use crate::tiktok::service::{TikTokHost, TikTokService};
use crate::twitch::model::{ChatStatus, TwitchState};
use crate::twitch::{TwitchConfig, TwitchHost, TwitchService};
use crate::updater::{self, Updater};
use crate::util::encoding::{base64_decode, base64_encode};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

/// Ce que l'application (fenêtres, système) fournit au cœur.
pub trait Host: Send + Sync {
    /// Événement poussé aux fenêtres du tableau de bord (principale et Live).
    fn emit(&self, channel: &str, payload: Value);
    /// Ouvre une adresse https dans le navigateur par défaut (déjà vérifiée par le cœur).
    fn open_external(&self, url: &str) -> Result<(), String>;
    fn open_live_window(&self) -> bool;
    fn set_live_always_on_top(&self, on: bool) -> bool;
    /// Fenêtre de connexion officielle de TikTok (le cœur ne voit ni mot de passe ni cookie).
    fn tiktok_login(&self);
    /// Efface la session TikTok de Batlay.
    fn tiktok_logout(&self);
    fn tiktok_show_page(&self, show: bool) -> bool;
    /// Thème natif des fenêtres (« dark », « light », « system »).
    fn apply_theme(&self, preference: &str);
    fn system_prefers_dark(&self) -> bool;
    fn set_launch_on_startup(&self, enabled: bool);
    /// Lance l'installateur vérifié puis ferme Batlay.
    fn run_installer_and_quit(&self, path: &Path);
}

pub struct AppConfig {
    pub data_dir: PathBuf,
    /// Pages servies aux logiciels de streaming (build Vite : overlay.html, alerts.html…).
    pub static_dir: PathBuf,
    pub version: String,
    /// Faux en développement (pas de mise à jour, pas de démarrage avec Windows).
    pub packaged: bool,
    /// Port du serveur Twitch (4000 ; 0 dans les tests).
    pub twitch_port: u16,
    /// Port imposé au serveur des overlays (tests) ; `None` = réglage de l'utilisateur.
    pub overlay_port: Option<u16>,
}

pub struct Deps {
    pub connector: Arc<dyn Connector>,
    pub secrets: Arc<dyn SecretBox>,
    pub pages: Arc<dyn PageHost>,
    pub powershell: Box<dyn PowerShell>,
    pub host: Arc<dyn Host>,
    pub rt: Rt,
}

pub struct App {
    me: Weak<App>,
    cfg: AppConfig,
    host: Arc<dyn Host>,
    rt: Rt,
    store: Arc<Store>,
    http: Arc<HttpClient>,
    media: MediaStore,
    overlay: OverlayServer,
    alerts: Arc<AlertHub>,
    live: Arc<LiveHub>,
    twitch: Arc<TwitchService>,
    tiktok: Arc<TikTokService>,
    spotify: Spotify,
    system_media: SystemMedia,
    updater: Updater,
    pages: Arc<dyn PageHost>,
    secrets: Arc<dyn SecretBox>,
    alias: Mutex<Option<AliasStatus>>,
    last_twitch_chat: Mutex<Option<ChatStatus>>,
    last_tiktok_status: Mutex<Option<TikTokStatus>>,
}

// -- Adaptateurs : chaque service parle à l'App sans la posséder -----------------------

struct Glue(Weak<App>);

impl Glue {
    fn app(&self) -> Option<Arc<App>> {
        self.0.upgrade()
    }
}

impl AlertSink for Glue {
    fn broadcast(&self, event: &AlertEvent) -> usize {
        self.app().map(|a| a.overlay.broadcast_alert(&event.to_json())).unwrap_or(0)
    }
    fn push_config(&self, config: &Value) {
        if let Some(a) = self.app() {
            a.overlay.set_alerts_config(config);
        }
    }
    fn save_config(&self, config: &Value) {
        if let Some(a) = self.app() {
            a.store.set("alerts", config.clone());
        }
    }
    fn save_secret(&self, secret: &str) {
        if let Some(a) = self.app() {
            a.store.set("alertsHookKey", Value::from(secret));
        }
    }
    fn on_result(&self, result: &ReceiveResult, source: AlertSource) {
        if let Some(a) = self.app() {
            a.live.record_alert(result, source);
        }
    }
}

impl LiveSink for Glue {
    fn update(&self, update: Value) {
        if let Some(a) = self.app() {
            a.host.emit("live:update", update);
        }
    }
    fn stats(&self, stats: Value) {
        if let Some(a) = self.app() {
            a.overlay.broadcast_live_stats(stats);
        }
    }
}

impl OverlayHooks for Glue {
    fn read_media(&self, id: &str) -> Option<(Vec<u8>, &'static str)> {
        self.app()?.media.read(id)
    }
    fn handle_hook(&self, secret: &str, payload: &Value) -> (u16, String) {
        match self.app() {
            Some(a) => a.alerts.handle_hook(secret, payload),
            None => (503, "Batlay se ferme.".into()),
        }
    }
    fn alerts_config(&self) -> Value {
        self.app().map(|a| a.alerts.config()).unwrap_or(Value::Null)
    }
}

impl TwitchHost for Glue {
    fn state_changed(&self, state: &TwitchState) {
        if let Some(a) = self.app() {
            a.on_twitch_state(state);
        }
    }
    fn alert(&self, payload: Value) {
        if let Some(a) = self.app() {
            a.alerts.receive(&payload, AlertSource::Twitch);
        }
    }
    fn overlay_message(&self, message: &Value) {
        if let Some(a) = self.app() {
            a.live.twitch_message(message);
        }
    }
    fn open_external(&self, url: &str) -> Result<(), String> {
        match self.app() {
            Some(a) => a.open_url(url),
            None => Err("Batlay se ferme".into()),
        }
    }
}

impl TikTokHost for Glue {
    fn state_changed(&self, state: &TikTokState) {
        if let Some(a) = self.app() {
            a.on_tiktok_state(state);
        }
    }
    fn alert(&self, payload: Value) {
        if let Some(a) = self.app() {
            a.alerts.receive(&payload, AlertSource::Tiktok);
        }
    }
    fn chat(&self, message: Value) {
        if let Some(a) = self.app() {
            a.live.tiktok_message(message.clone());
            // Overlays du chat (paramètre src= pour choisir la plateforme).
            a.twitch.inject_chat(message);
        }
    }
    fn audience(&self, viewers: Option<u64>, likes: Option<u64>) {
        if let Some(a) = self.app() {
            a.live.tiktok_audience(viewers, likes);
        }
    }
    fn save_settings(&self, settings: &TikTokSettings) {
        if let Some(a) = self.app() {
            a.store.set("tiktokLive", settings.to_json());
        }
    }
    fn euler_key(&self) -> Option<String> {
        self.app()?.euler_key()
    }
}

// -- Validation des arguments -----------------------------------------------------------

fn arg(args: &[Value], i: usize) -> &Value {
    static NULL: Value = Value::Null;
    args.get(i).unwrap_or(&NULL)
}

fn short_str(v: &Value, max: usize) -> Option<&str> {
    v.as_str().filter(|s| s.chars().count() <= max)
}

fn is_overlay_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Hôtes des sources de pochettes (recherche faite par le tableau de bord, réseau par le cœur).
const ARTWORK_HOSTS: [&str; 6] = ["itunes.apple.com", "api.deezer.com", "musicbrainz.org", "coverartarchive.org", "api.listenbrainz.org", "api.audius.co"];

impl App {
    pub fn start(cfg: AppConfig, deps: Deps) -> Arc<App> {
        let _ = std::fs::create_dir_all(&cfg.data_dir);
        crate::log::init(cfg.data_dir.join("logs"));
        crate::log::info(&format!("Démarrage de Batlay {} (données : {})", cfg.version, cfg.data_dir.display()));
        let store = Arc::new(Store::open(cfg.data_dir.join("batlay-config.json"), config::default_config()));
        // Le port 4000 appartient au serveur Twitch : un ancien réglage qui le viserait repasse sur 3000.
        let settings = config::normalize_settings(&store.get("settings"));
        store.set("settings", settings.clone());
        let port = cfg.overlay_port.unwrap_or(settings.get("overlayServerPort").as_u64().unwrap_or(3000) as u16);
        let http = Arc::new(HttpClient::new(deps.connector.clone(), format!("Batlay/{} (overlays pour le streaming)", cfg.version)));
        let host = deps.host.clone();
        let rt = deps.rt.clone();
        let app = Arc::new_cyclic(|me: &Weak<App>| {
            let glue = || Arc::new(Glue(me.clone()));
            let ws = Arc::new(WsConnector(deps.connector.clone()));
            let twitch = TwitchService::new(
                TwitchConfig { port: cfg.twitch_port, static_dir: cfg.static_dir.clone(), session_file: cfg.data_dir.join("batlay-twitch.json") },
                http.clone() as Arc<dyn Http>,
                ws.clone(),
                deps.secrets.clone(),
                glue(),
                rt.clone(),
            );
            let tiktok = TikTokService::new(&store.get("tiktokLive"), http.clone() as Arc<dyn Http>, deps.pages.clone(), ws.clone(), glue(), rt.clone());
            let me_open = me.clone();
            let spotify = Spotify::new(
                store.clone(),
                deps.secrets.clone(),
                http.clone() as Arc<dyn Http>,
                rt.clock.clone(),
                Arc::new(move |url: &str| me_open.upgrade().ok_or("Batlay se ferme".to_string())?.open_url(url)),
            );
            let me_status = me.clone();
            let updater = Updater::new(
                &cfg.version,
                cfg.packaged,
                deps.connector.clone(),
                std::env::temp_dir().join("batlay-update"),
                Arc::new(move |s: &updater::Status| {
                    if let Some(a) = me_status.upgrade() {
                        a.host.emit("update:status", s.to_json());
                    }
                }),
            );
            App {
                me: me.clone(),
                media: MediaStore::new(cfg.data_dir.join("alert-media")),
                overlay: OverlayServer::new(port, cfg.static_dir.clone(), glue()),
                alerts: Arc::new(AlertHub::new(&store.get("alerts"), store.get("alertsHookKey").as_str(), glue(), rt.clock.clone())),
                live: LiveHub::new(glue(), rt.clone()),
                twitch,
                tiktok,
                spotify,
                system_media: SystemMedia::new(deps.powershell),
                updater,
                pages: deps.pages.clone(),
                secrets: deps.secrets.clone(),
                alias: Mutex::new(None),
                last_twitch_chat: Mutex::new(None),
                last_tiktok_status: Mutex::new(None),
                store,
                http,
                host,
                rt,
                cfg,
            }
        });
        app.overlay.set_overlay_configs(&app.store.get("overlays"));
        if let Err(e) = app.overlay.start() {
            crate::log::error(&format!("Échec du démarrage du serveur d'overlay sur le port {port} : {e}"));
            app.host.emit("overlay-server:error", obj! { "message" => format!("Le port {port} est déjà utilisé par une autre application. Changez le port dans Paramètres > Serveur d'overlay.") });
        }
        // Une panne du module Twitch n'empêche jamais le reste de démarrer.
        app.twitch.start();
        // Avant la fenêtre : les premières URL affichées utilisent déjà le bon hôte.
        app.refresh_alias();
        let theme = config::normalize_theme(app.store.get("settings").get("theme"));
        app.host.apply_theme(theme);
        app
    }

    /// Après l'ouverture de la fenêtre principale (non bloquant) : live TikTok,
    /// proposition unique d'ajouter batlay.test, recherche de mise à jour.
    pub fn after_window(self: &Arc<Self>) {
        let me = self.clone();
        self.rt.spawn("batlay-after-window", move || {
            let logged_in = me.pages.logged_in();
            me.tiktok.set_logged_in(logged_in);
            me.tiktok.set_euler_key(me.euler_key().is_some());
            me.tiktok.start();
            me.auto_setup_alias();
        });
        if self.cfg.packaged {
            let me = self.clone();
            // Recherche discrète 10 s après le démarrage : elle ne télécharge rien.
            self.rt.after(10_000, move || {
                let m2 = me.clone();
                me.rt.spawn("batlay-update-check", move || {
                    m2.updater.check(true);
                });
            });
        }
    }

    /// Clé API d'Euler Stream, déchiffrée (jamais écrite en clair sur le disque).
    fn euler_key(&self) -> Option<String> {
        let blob = base64_decode(self.store.get("tiktokEuler").get("apiKeyDpapi").as_str()?)?;
        let key = String::from_utf8(self.secrets.unprotect(&blob)?).ok()?;
        crate::tiktok::euler::is_valid_key(&key).then_some(key)
    }

    /// Enregistre (chiffrée par Windows) ou efface la clé API d'Euler. Faux si la clé
    /// n'est pas valable ou si le chiffrement est indisponible : jamais de clé en clair.
    fn set_euler_key(&self, key: Option<&str>) -> bool {
        let ok = match key.map(str::trim).filter(|k| !k.is_empty()) {
            None => {
                self.store.set("tiktokEuler", Value::Null);
                true
            }
            Some(k) if crate::tiktok::euler::is_valid_key(k) => match self.secrets.protect(k.as_bytes()) {
                Some(blob) => {
                    self.store.set("tiktokEuler", obj! { "apiKeyDpapi" => base64_encode(&blob) });
                    true
                }
                None => false,
            },
            Some(_) => false,
        };
        if ok {
            self.tiktok.set_euler_key(self.euler_key().is_some());
        }
        ok
    }

    /// Session TikTok changée (fenêtre de connexion fermée, cookie posé ou retiré).
    pub fn tiktok_session_changed(&self, logged_in: bool) {
        self.tiktok.set_logged_in(logged_in);
    }

    pub fn shutdown(&self) {
        self.tiktok.stop();
        self.twitch.stop();
        self.overlay.stop();
        crate::log::info("Batlay fermé.");
    }

    pub fn version(&self) -> &str {
        &self.cfg.version
    }

    /// Thème initial, lu avant le premier rendu (injecté dans la page, sans aller-retour).
    pub fn initial_theme(&self) -> Value {
        let preference = config::normalize_theme(self.store.get("settings").get("theme"));
        obj! { "preference" => preference, "resolved" => crate::theme::resolve(preference, self.host.system_prefers_dark()) }
    }

    /// Windows a changé de mode clair / sombre.
    pub fn system_theme_changed(&self) {
        self.host.emit("theme:changed", self.initial_theme());
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        if !crate::integrations::is_openable(url) {
            return Err("adresse refusée".into());
        }
        self.host.open_external(url)
    }

    fn preferred_host(&self) -> &'static str {
        if self.alias.lock().unwrap_or_else(|e| e.into_inner()).as_ref().is_some_and(AliasStatus::is_ok) {
            LOCAL_ALIAS_HOSTNAME
        } else {
            "localhost"
        }
    }

    fn refresh_alias(&self) -> AliasStatus {
        let before = self.preferred_host();
        let status = hosts_alias::check_local_alias();
        *self.alias.lock().unwrap_or_else(|e| e.into_inner()) = Some(status.clone());
        if self.preferred_host() != before {
            self.host.emit("integrations:alias-changed", status.to_json());
            self.on_twitch_state(&self.twitch.state());
        }
        status
    }

    fn change_alias(&self, add: bool) -> Value {
        let run = |args: Vec<String>| -> i32 {
            // 2 min : laisse le temps de répondre à la fenêtre de Windows.
            crate::system_media::run_process("powershell.exe", &args, Duration::from_secs(120)).map(|(code, _)| code).unwrap_or(1)
        };
        let outcome = hosts_alias::apply(add, &hosts_alias::check_local_alias, &run);
        self.refresh_alias();
        outcome
    }

    /// Premier lancement sous Windows : propose UNE fois d'ajouter batlay.test.
    fn auto_setup_alias(&self) {
        if !cfg!(windows) {
            return;
        }
        let status = self.refresh_alias();
        if status.is_ok() || self.store.get("localAlias").get("autoSetupAttempted").as_bool() == Some(true) {
            return;
        }
        self.store.set("localAlias", obj! { "autoSetupAttempted" => true });
        let outcome = self.change_alias(true);
        crate::log::info(&format!("Configuration automatique de {LOCAL_ALIAS_HOSTNAME} : {}", outcome.get("result").as_str().unwrap_or("?")));
    }

    fn on_twitch_state(&self, state: &TwitchState) {
        let changed = {
            let mut last = self.last_twitch_chat.lock().unwrap_or_else(|e| e.into_inner());
            let changed = *last != Some(state.chat);
            let first = last.is_none();
            *last = Some(state.chat);
            changed && !(first && state.chat == ChatStatus::Off)
        };
        if changed {
            let who = state.channel.as_ref().map(|c| c.display_name.clone()).or_else(|| state.channel_login.clone()).unwrap_or_else(|| "la chaîne".into());
            let (text, level) = match state.chat {
                ChatStatus::Off => ("Chat Twitch arrêté.".to_string(), "info"),
                ChatStatus::Connecting => (format!("Connexion au chat Twitch de {who}…"), "info"),
                ChatStatus::Live => (format!("Chat Twitch de {who} connecté."), "success"),
                ChatStatus::Reconnecting => ("Chat Twitch coupé : reconnexion automatique…".to_string(), "warn"),
            };
            self.live.system("twitch", level, &text);
        }
        self.host.emit("twitch:state", state.to_json(self.preferred_host()));
    }

    fn on_tiktok_state(&self, state: &TikTokState) {
        let line = {
            let mut last = self.last_tiktok_status.lock().unwrap_or_else(|e| e.into_inner());
            let previous = *last;
            let line = if previous != Some(state.status) || matches!(state.status, TikTokStatus::Error | TikTokStatus::Hidden) {
                let who = state.settings.username.as_ref().map(|u| format!("@{u}")).unwrap_or_else(|| "TikTok".into());
                match state.status {
                    TikTokStatus::Idle => previous.filter(|p| *p != TikTokStatus::Idle).map(|_| ("TikTok déconnecté.".to_string(), "info")),
                    TikTokStatus::Connecting => None,
                    TikTokStatus::Live => Some((format!("Branché sur le live TikTok de {who}."), "success")),
                    TikTokStatus::Waiting => Some((state.message.clone().unwrap_or_else(|| format!("En attente du live de {who}.")), "info")),
                    TikTokStatus::Hidden => Some(("Live TikTok en cours mais caché (compte privé) : voir la carte TikTok dans Alertes.".to_string(), "warn")),
                    TikTokStatus::Error => Some((state.message.clone().unwrap_or_else(|| "Erreur TikTok.".into()), "error")),
                }
            } else {
                None
            };
            *last = Some(state.status);
            line
        };
        if let Some((text, level)) = line {
            self.live.system("tiktok", level, &text);
        }
        if state.status != TikTokStatus::Live {
            self.live.tiktok_audience(Some(0), None);
        }
        self.host.emit("tiktok:state", state.to_json());
    }

    fn alerts_urls(&self) -> Value {
        let base = format!("http://{}:{}", self.preferred_host(), self.overlay.port());
        obj! { "page" => format!("{base}{ALERTS_PAGE_PATH}"), "hook" => format!("{base}{ALERTS_HOOK_PREFIX}{}", self.alerts.secret()) }
    }

    fn artwork_fetch(&self, method: &Value, url: &Value) -> Result<Value, String> {
        let url = short_str(url, 2000).ok_or("adresse invalide")?;
        let parsed = crate::util::url::Url::parse(url).filter(|u| u.scheme == "https" && ARTWORK_HOSTS.contains(&u.host.as_str())).ok_or("adresse refusée")?;
        let request = match method.as_str() {
            Some("HEAD") => HttpRequest::head(parsed.to_string_full()),
            _ => HttpRequest::get(parsed.to_string_full()).header("Accept", "application/json"),
        }
        .timeout(Duration::from_millis(3500))
        .max_body(2 * 1024 * 1024);
        match self.http.send(&request) {
            Ok(r) => Ok(obj! { "ok" => true, "status" => r.status, "body" => r.text() }),
            Err(e) => Ok(obj! { "ok" => false, "status" => 0, "error" => e.to_string() }),
        }
    }

    /// Commande du tableau de bord. `args` : arguments positionnels (comme l'ancien preload).
    pub fn invoke(&self, command: &str, args: &[Value]) -> Result<Value, String> {
        let a0 = arg(args, 0);
        let a1 = arg(args, 1);
        Ok(match command {
            // --- Réglages ---
            "config:get" => obj! { "settings" => config::normalize_settings(&self.store.get("settings")), "overlays" => self.store.get("overlays"), "activeOverlayId" => self.store.get("activeOverlayId") },
            "config:set-settings" => {
                let previous = config::normalize_settings(&self.store.get("settings"));
                let next = config::merge_settings(&previous, a0);
                self.store.set("settings", next.clone());
                if next.get("theme") != previous.get("theme") {
                    let theme = config::normalize_theme(next.get("theme"));
                    self.host.apply_theme(theme);
                    self.host.emit("theme:changed", self.initial_theme());
                }
                if next.get("launchOnStartup") != previous.get("launchOnStartup") && self.cfg.packaged {
                    self.host.set_launch_on_startup(next.get("launchOnStartup").as_bool() == Some(true));
                }
                next
            }
            "config:get-overlays" => self.store.get("overlays"),
            "config:save-overlays" => {
                let list: Vec<Value> = a0.items().iter().filter(|o| o.get("id").as_str().is_some_and(is_overlay_id)).cloned().collect();
                if list.len() > 500 {
                    return Err("Trop d'overlays.".into());
                }
                let list = Value::Array(list);
                if list.to_json().len() > 8 * 1024 * 1024 {
                    return Err("Overlays trop volumineux.".into());
                }
                self.store.set("overlays", list.clone());
                self.overlay.set_overlay_configs(&list);
                Value::Bool(true)
            }
            "config:get-active-overlay-id" => self.store.get("activeOverlayId"),
            "config:set-active-overlay-id" => {
                let id = a0.as_str().filter(|s| is_overlay_id(s)).map(Value::from).unwrap_or(Value::Null);
                self.store.set("activeOverlayId", id);
                Value::Bool(true)
            }
            // --- Serveur des overlays ---
            "overlay:get-url" => {
                if !self.overlay.is_running() {
                    return Err("Le serveur d'overlay n'est pas démarré (port déjà utilisé ?). Vérifiez Paramètres > Serveur d'overlay.".into());
                }
                let id = a0.as_str().filter(|s| is_overlay_id(s)).ok_or("Identifiant d'overlay invalide.")?;
                Value::from(self.overlay.overlay_url(id, self.preferred_host()))
            }
            "overlay:is-running" => Value::Bool(self.overlay.is_running()),
            "overlay:get-client-counts" => self.overlay.client_counts(),
            "overlay:broadcast-state" => {
                if !a0.is_object() || a0.to_json().len() > 2 * 1024 * 1024 {
                    return Err("État de lecture invalide.".into());
                }
                self.overlay.broadcast_state(a0.clone());
                Value::Bool(true)
            }
            // --- Intégrations ---
            "integrations:open-docs" => match a0.as_str().and_then(crate::integrations::docs_url) {
                Some(url) => Value::Bool(self.open_url(url).is_ok()),
                None => Value::Bool(false),
            },
            "integrations:check-local-alias" => self.refresh_alias().to_json(),
            "integrations:setup-local-alias" => self.change_alias(true),
            "integrations:remove-local-alias" => self.change_alias(false),
            // --- Alertes ---
            "alerts:get" => obj! { "config" => self.alerts.config(), "urls" => self.alerts_urls(), "pages" => self.overlay.alert_client_count() },
            "alerts:save" => self.alerts.set_config(a0),
            "alerts:test" => self.alerts.test(a0).to_json(),
            "alerts:skip" => {
                self.overlay.skip_alerts();
                Value::Bool(true)
            }
            "alerts:regenerate-key" => {
                self.alerts.regenerate_secret();
                self.alerts_urls()
            }
            "alerts:import-media" => {
                let expected = match a1.as_str() {
                    Some(k @ ("image" | "sound")) => k,
                    _ => return Ok(obj! { "ok" => false, "reason" => "Fichier illisible." }),
                };
                let data = a0.as_str().filter(|s| s.len() <= 12 * 1024 * 1024).and_then(base64_decode);
                match data {
                    Some(bytes) => self.media.save(&bytes, expected),
                    None => obj! { "ok" => false, "reason" => "Fichier illisible." },
                }
            }
            "alerts:list-media" => Value::from(self.media.list(if a0.as_str() == Some("sound") { "sound" } else { "image" }, 60)),
            "alerts:media-data-url" => match a0.as_str().and_then(|id| self.media.read(id)) {
                Some((data, mime)) => Value::from(format!("data:{mime};base64,{}", base64_encode(&data))),
                None => Value::Null,
            },
            // --- Fenêtre Live ---
            "live:get-snapshot" => self.live.snapshot(),
            "live:reset" => self.live.reset(),
            "live:replay" => {
                let shown = a0.as_str().and_then(|id| self.live.find_event(id)).map(|e| self.alerts.replay(&e)).unwrap_or(false);
                obj! { "shown" => shown }
            }
            "live:open-window" => Value::Bool(self.host.open_live_window()),
            "live:set-always-on-top" => Value::Bool(self.host.set_live_always_on_top(a0.as_bool() == Some(true))),
            // --- TikTok ---
            "tiktok:get-state" => self.tiktok.state().to_json(),
            "tiktok:connect" => Value::Bool(short_str(a0, 300).is_some_and(|u| self.tiktok.connect(u))),
            "tiktok:disconnect" => {
                self.tiktok.disconnect();
                Value::Bool(true)
            }
            "tiktok:login" => {
                self.host.tiktok_login();
                Value::Bool(true)
            }
            "tiktok:logout" => {
                self.host.tiktok_logout();
                self.tiktok.set_logged_in(false);
                Value::Bool(true)
            }
            "tiktok:update-settings" => self.tiktok.update_settings(a0).to_json(),
            "tiktok:show-page" => Value::Bool(self.host.tiktok_show_page(a0.as_bool() == Some(true))),
            "tiktok:diagnose" => self.tiktok.diagnose(&self.cfg.version),
            "tiktok:set-euler-key" => Value::Bool(match a0 {
                Value::Null => self.set_euler_key(None),
                v => short_str(v, 300).is_some_and(|k| self.set_euler_key(Some(k))),
            }),
            // --- Spotify ---
            "spotify:is-configured" => Value::Bool(self.spotify.is_configured()),
            "spotify:set-client-id" => Value::Bool(short_str(a0, 100).is_some_and(|id| self.spotify.set_client_id(id))),
            "spotify:connect" => {
                self.spotify.connect()?;
                Value::Bool(true)
            }
            "spotify:disconnect" => {
                self.spotify.disconnect();
                Value::Bool(true)
            }
            "spotify:is-session-restorable" => Value::Bool(self.spotify.is_session_restorable()),
            "spotify:restore-session" => Value::Bool(self.spotify.restore_session()),
            "spotify:get-currently-playing" => self.spotify.currently_playing()?,
            // --- Twitch ---
            "twitch:get-state" => self.twitch.state().to_json(self.preferred_host()),
            "twitch:connect" => {
                if let Some(a) = self.me.upgrade() {
                    a.twitch.connect();
                }
                Value::Bool(true)
            }
            "twitch:reauthorize" => {
                self.twitch.reauthorize();
                Value::Bool(true)
            }
            "twitch:cancel" => {
                self.twitch.cancel();
                Value::Bool(true)
            }
            "twitch:disconnect" => {
                self.twitch.disconnect();
                Value::Bool(true)
            }
            "twitch:set-channel" => match a0 {
                Value::Null => Value::Bool(self.twitch.set_channel(None)),
                Value::String(s) => Value::Bool(self.twitch.set_channel(Some(&s.chars().take(300).collect::<String>()))),
                _ => Value::Bool(false),
            },
            "twitch:send-test" => {
                self.twitch.send_test_message();
                Value::Bool(true)
            }
            "twitch:restart-overlay" => Value::from(self.twitch.restart_overlay()),
            // --- Mises à jour ---
            "update:get-version" => Value::from(self.cfg.version.clone()),
            "update:get-status" => self.updater.status().to_json(),
            "update:check" => self.updater.check(false).to_json(),
            "update:download" => self.updater.download().to_json(),
            "update:install" => match self.updater.installer() {
                Some(path) => {
                    self.host.run_installer_and_quit(&path);
                    Value::Bool(true)
                }
                None => Value::Bool(false),
            },
            "update:open-releases" => Value::Bool(self.open_url(&updater::releases_url()).is_ok()),
            // --- Musique ---
            "artwork:fetch" => self.artwork_fetch(a0, a1)?,
            "system-media:is-supported" => Value::Bool(crate::system_media::is_supported()),
            "system-media:list-sessions" => self.system_media.list_sessions()?,
            "system-media:get-current" => {
                let preferred = a0.get("preferredAppId").as_str();
                self.system_media.current(preferred, a0.get("includeArtwork").as_bool() == Some(true), self.rt.now())?
            }
            _ => return Err(format!("Commande inconnue : {command}")),
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::json::parse;
    use crate::net::client::PlainConnector;
    use crate::runtime::{ManualClock, ManualTimers, ThreadSpawner};
    use crate::tiktok::browser::{LivePage, PageEvent, ProbeResult};
    use std::sync::mpsc::Sender;

    #[derive(Default)]
    pub struct TestHost {
        pub events: Mutex<Vec<(String, Value)>>,
        pub opened: Mutex<Vec<String>>,
    }
    impl Host for TestHost {
        fn emit(&self, channel: &str, payload: Value) {
            self.events.lock().unwrap().push((channel.into(), payload));
        }
        fn open_external(&self, url: &str) -> Result<(), String> {
            self.opened.lock().unwrap().push(url.into());
            Ok(())
        }
        fn open_live_window(&self) -> bool {
            true
        }
        fn set_live_always_on_top(&self, on: bool) -> bool {
            on
        }
        fn tiktok_login(&self) {}
        fn tiktok_logout(&self) {}
        fn tiktok_show_page(&self, _: bool) -> bool {
            false
        }
        fn apply_theme(&self, _: &str) {}
        fn system_prefers_dark(&self) -> bool {
            true
        }
        fn set_launch_on_startup(&self, _: bool) {}
        fn run_installer_and_quit(&self, _: &Path) {}
    }

    pub struct NoPages;
    impl PageHost for NoPages {
        fn open_live_page(&self, _: &str, _: bool, _: Sender<PageEvent>) -> Result<Box<dyn LivePage>, String> {
            Err("indisponible".into())
        }
        fn probe(&self, _: &str) -> Result<ProbeResult, String> {
            Err("indisponible".into())
        }
        fn logged_in(&self) -> bool {
            false
        }
    }

    struct NoShell;
    impl PowerShell for NoShell {
        fn run(&self, _: &[String], _: Duration) -> Result<Vec<u8>, String> {
            Err("indisponible".into())
        }
    }

    pub fn start_app(dir: &Path, static_dir: &Path) -> (Arc<App>, Arc<TestHost>) {
        start_app_with(dir, static_dir, Arc::new(config::NoSecretBox))
    }

    /// Chiffrement simulé (réversible, jamais le texte en clair sur le disque).
    struct XorBox;
    impl SecretBox for XorBox {
        fn protect(&self, data: &[u8]) -> Option<Vec<u8>> {
            Some(data.iter().map(|b| b ^ 0x5a).collect())
        }
        fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>> {
            self.protect(data)
        }
    }

    #[test]
    fn euler_key_is_stored_encrypted_and_reaches_the_tiktok_service() {
        let dir = std::env::temp_dir().join(format!("batlay-app-{}", crate::util::random::random_hex(4)));
        let (app, _host) = start_app_with(&dir, &dir, Arc::new(XorBox));
        assert_eq!(app.invoke("tiktok:set-euler-key", &[Value::from("  euler_ABCDEF12  ")]).unwrap(), Value::Bool(true));
        assert_eq!(app.euler_key().as_deref(), Some("euler_ABCDEF12"));
        assert_eq!(app.invoke("tiktok:get-state", &[]).unwrap().get("eulerKey").as_bool(), Some(true));
        app.shutdown();
        let file = std::fs::read_to_string(dir.join("batlay-config.json")).unwrap();
        assert!(file.contains("apiKeyDpapi") && !file.contains("euler_ABCDEF12"), "clé chiffrée seulement");
        let (app, _host) = start_app_with(&dir, &dir, Arc::new(XorBox));
        assert_eq!(app.euler_key().as_deref(), Some("euler_ABCDEF12"), "relue au démarrage suivant");
        assert_eq!(app.invoke("tiktok:set-euler-key", &[Value::from("")]).unwrap(), Value::Bool(true));
        assert_eq!(app.euler_key(), None);
        app.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn start_app_with(dir: &Path, static_dir: &Path, secrets: Arc<dyn SecretBox>) -> (Arc<App>, Arc<TestHost>) {
        std::fs::create_dir_all(dir).unwrap();
        let host = Arc::new(TestHost::default());
        let rt = Rt { clock: ManualClock::new(1_000_000), timers: ManualTimers::new(), spawner: Arc::new(ThreadSpawner) };
        let app = App::start(
            AppConfig { data_dir: dir.into(), static_dir: static_dir.into(), version: "0.5.0".into(), packaged: false, twitch_port: 0, overlay_port: Some(0) },
            Deps { connector: Arc::new(PlainConnector), secrets, pages: Arc::new(NoPages), powershell: Box::new(NoShell), host: host.clone(), rt },
        );
        (app, host)
    }

    #[test]
    fn dashboard_commands_end_to_end() {
        let dir = std::env::temp_dir().join(format!("batlay-app-{}", crate::util::random::random_hex(4)));
        let (app, host) = start_app(&dir, &dir);
        assert!(app.invoke("commande:inconnue", &[]).is_err());
        let s = app.invoke("config:set-settings", &[parse(r#"{"theme":"light","density":"compact","overlayServerPort":4000}"#).unwrap()]).unwrap();
        assert_eq!((s.get("theme").as_str(), s.get("density").as_str()), (Some("light"), Some("compact")));
        assert!(host.events.lock().unwrap().iter().any(|(c, p)| c == "theme:changed" && p.get("resolved").as_str() == Some("light")));
        app.invoke("config:save-overlays", &[parse(r#"[{"id":"abc","name":"Glass"},{"id":"../x"},{"name":"sans id"}]"#).unwrap()]).unwrap();
        assert_eq!(app.invoke("config:get-overlays", &[]).unwrap().items().len(), 1);
        let alerts = app.invoke("alerts:get", &[]).unwrap();
        assert!(alerts.path(&["urls", "hook"]).as_str().unwrap().contains("/api/alerts/hook/"));
        let r = app.invoke("alerts:test", &[Value::from("follow")]).unwrap();
        assert_eq!(r.get("reason").as_str(), Some("no-source"), "aucune page d'alertes ouverte");
        let snap = app.invoke("live:get-snapshot", &[]).unwrap();
        assert_eq!(snap.get("activity").items().len(), 1);
        let entry = snap.get("activity").at(0).get("id").as_str().unwrap().to_string();
        assert_eq!(app.invoke("live:replay", &[Value::from(entry)]).unwrap().get("shown").as_bool(), Some(false));
        // Fichier importé : contenu vérifié, data URL rendue.
        let png = base64_encode(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0, 0, 0, 0, 0]);
        let saved = app.invoke("alerts:import-media", &[Value::from(png), Value::from("image")]).unwrap();
        let id = saved.get("id").as_str().unwrap().to_string();
        assert!(app.invoke("alerts:media-data-url", &[Value::from(id)]).unwrap().as_str().unwrap().starts_with("data:image/png;base64,"));
        // Adresses : liste fermée.
        assert_eq!(app.invoke("integrations:open-docs", &[Value::from("obs")]).unwrap(), Value::Bool(true));
        assert_eq!(app.invoke("integrations:open-docs", &[Value::from("https://evil.example")]).unwrap(), Value::Bool(false));
        assert_eq!(host.opened.lock().unwrap().as_slice(), ["https://obsproject.com/kb/browser-source"]);
        assert!(app.invoke("artwork:fetch", &[Value::from("GET"), Value::from("https://evil.example/x")]).is_err());
        assert!(app.invoke("artwork:fetch", &[Value::from("GET"), Value::from("http://itunes.apple.com/search")]).is_err());
        // TikTok : pseudo revalidé.
        assert_eq!(app.invoke("tiktok:connect", &[Value::from("pas valable !")]).unwrap(), Value::Bool(false));
        // Clé Euler : jamais enregistrée en clair (pas de chiffrement ici → refusée), effaçable.
        assert_eq!(app.invoke("tiktok:set-euler-key", &[Value::from("euler_ABCDEF12")]).unwrap(), Value::Bool(false));
        assert_eq!(app.invoke("tiktok:set-euler-key", &[Value::from("pas valable !")]).unwrap(), Value::Bool(false));
        assert_eq!(app.invoke("tiktok:set-euler-key", &[Value::Null]).unwrap(), Value::Bool(true));
        assert!(!std::fs::read_to_string(dir.join("batlay-config.json")).unwrap_or_default().contains("euler_ABCDEF12"));
        assert_eq!(app.invoke("tiktok:get-state", &[]).unwrap().get("eulerKey").as_bool(), Some(false));
        assert_eq!(app.invoke("tiktok:update-settings", &[crate::json::parse(r#"{"mode":"euler"}"#).unwrap()]).unwrap().get("mode").as_str(), Some("euler"));
        assert_eq!(app.invoke("twitch:get-state", &[]).unwrap().path(&["server", "running"]).as_bool(), Some(true));
        assert_eq!(app.invoke("update:get-status", &[]).unwrap().get("state").as_str(), Some("unavailable"));
        app.shutdown();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
