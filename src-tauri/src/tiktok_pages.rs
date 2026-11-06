//! Pages TikTok ouvertes par Batlay, toutes dans un profil WebView2 À PART
//! (dossier `TikTokWebView`) : les cookies TikTok n'y côtoient rien d'autre.
//!
//! - page du live, INVISIBLE (ni affichée, ni dans la barre des tâches), son
//!   coupé, navigation limitée à tiktok.com, aucune fenêtre secondaire. Batlay
//!   n'y injecte aucun script : il écoute le flux par le protocole DevTools
//!   propre à cette fenêtre (voir cdp.rs) ;
//! - sonde : même page, légère, lue puis refermée (20 s au plus) ;
//! - connexion : la page officielle tiktok.com/login, visible. Batlay ne voit
//!   jamais le mot de passe et ne lit que la PRÉSENCE du cookie « sessionid ».
//!
//! Ces pages sont des sites distants : elles n'ont accès à AUCUNE commande de
//! Batlay (capabilities/main.json ne vise que les fenêtres « main » et « live »).

use crate::cdp;
use crate::host::{emit_app, BROWSER_ARGS};
use batlay_core::app::App;
use batlay_core::json::Value;
use batlay_core::tiktok::browser::{LivePage, PageEvent, PageHost, ProbeResult};
use batlay_core::tiktok::model::{live_api_url, live_page_url};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;
use tauri::webview::{Color, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry};

pub const LOGIN: &str = "tiktok-login";
const PREFIX: &str = "tiktok-";
const LOGIN_URL: &str = "https://www.tiktok.com/login";
const COOKIE_URL: &str = "https://www.tiktok.com/";
const PAGE_TITLE: &str = "Page TikTok du live (Batlay) · fermer la remet en arrière-plan";

/// Page du site TikTok (https uniquement) ?
fn is_tiktok_page(url: &Url) -> bool {
    url.scheme() == "https" && matches!(url.host_str(), Some("www.tiktok.com" | "tiktok.com"))
}

/// Page du live en cours d'utilisation (la plus récente).
#[derive(Clone)]
struct Current {
    label: String,
    visible: Arc<AtomicBool>,
}

pub struct TikTokPages {
    handle: AppHandle,
    /// Profil WebView2 réservé à TikTok.
    profile: PathBuf,
    core: OnceLock<Weak<App>>,
    current: Arc<Mutex<Option<Current>>>,
    /// Une seule sonde à la fois.
    probing: Mutex<()>,
    counter: AtomicU64,
    login_watch: AtomicBool,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Cache la page du live (elle continue de tourner) et prévient le tableau de bord.
fn hide(handle: &AppHandle, window: &WebviewWindow, visible: &AtomicBool) {
    let _ = window.hide();
    let _ = window.set_skip_taskbar(true);
    let _ = window.set_focusable(false);
    if visible.swap(false, Ordering::SeqCst) {
        emit_app(handle, "tiktok:page-visible", "false".into());
    }
}

/// Fenêtre de la page du live côté cœur.
struct HiddenPage {
    handle: AppHandle,
    label: String,
    visible: Arc<AtomicBool>,
    current: Arc<Mutex<Option<Current>>>,
}

impl LivePage for HiddenPage {
    fn close(&mut self) {
        {
            let mut current = lock(&self.current);
            if current.as_ref().is_some_and(|c| c.label == self.label) {
                *current = None;
            }
        }
        if self.visible.swap(false, Ordering::SeqCst) {
            emit_app(&self.handle, "tiktok:page-visible", "false".into());
        }
        if let Some(window) = self.handle.get_webview_window(&self.label) {
            // destroy : pas de « CloseRequested » (qui ne ferait que cacher la page).
            let _ = window.destroy();
        }
    }

    fn is_visible(&self) -> bool {
        self.visible.load(Ordering::SeqCst)
    }
}

impl TikTokPages {
    pub fn new(handle: AppHandle, profile: PathBuf) -> TikTokPages {
        TikTokPages {
            handle,
            profile,
            core: OnceLock::new(),
            current: Arc::new(Mutex::new(None)),
            probing: Mutex::new(()),
            counter: AtomicU64::new(0),
            login_watch: AtomicBool::new(false),
        }
    }

    pub fn set_core(&self, core: &Arc<App>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    fn next_label(&self, kind: &str) -> String {
        format!("{PREFIX}{kind}-{}", self.counter.fetch_add(1, Ordering::SeqCst) + 1)
    }

    /// Fenêtre du profil TikTok. Cachée : ni affichée, ni dans la barre des tâches, ni focalisable.
    fn builder<'a>(&'a self, label: &str, url: WebviewUrl, hidden: bool) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
        let builder = WebviewWindowBuilder::new(&self.handle, label, url)
            .data_directory(self.profile.clone())
            .additional_browser_args(BROWSER_ARGS)
            .devtools(false);
        if hidden {
            builder
                .visible(false)
                .skip_taskbar(true)
                .focused(false)
                .focusable(false)
                .inner_size(1280.0, 800.0)
                .title(PAGE_TITLE)
                // Navigation limitée à tiktok.com ; aucune fenêtre secondaire.
                .on_navigation(|url| url.as_str() == "about:blank" || is_tiktok_page(url))
                .on_new_window(|_, _| NewWindowResponse::Deny)
        } else {
            builder
        }
    }

    /// Exécute `f` avec une fenêtre du profil TikTok (une existante, sinon une
    /// fenêtre vide et cachée, refermée aussitôt). Jamais sur le fil principal.
    fn with_profile<T>(&self, f: impl FnOnce(&WebviewWindow) -> T) -> Option<T> {
        let existing = self.handle.webview_windows().into_iter().find(|(label, _)| label.starts_with(PREFIX)).map(|(_, w)| w);
        if let Some(window) = existing {
            return Some(f(&window));
        }
        let label = self.next_label("session");
        let url = WebviewUrl::External(Url::parse("about:blank").ok()?);
        let window = self.builder(&label, url, true).build().ok()?;
        let out = f(&window);
        let _ = window.destroy();
        Some(out)
    }

    fn read_logged_in(window: &WebviewWindow) -> bool {
        let Ok(url) = Url::parse(COOKIE_URL) else { return false };
        window.cookies_for_url(url).is_ok_and(|cookies| cookies.iter().any(|c| c.name() == "sessionid" && !c.value().is_empty()))
    }

    fn notify_login(&self, logged_in: bool) {
        if let Some(core) = self.core.get().and_then(Weak::upgrade) {
            core.tiktok_session_changed(logged_in);
        }
    }

    // -- Connexion ------------------------------------------------------------------------

    /// Ouvre (ou ramène) la page de connexion officielle de TikTok.
    pub fn open_login(self: &Arc<Self>, theme: &str) {
        if let Some(window) = self.handle.get_webview_window(LOGIN) {
            let _ = window.unminimize();
            let _ = window.set_focus();
            return;
        }
        let Ok(url) = Url::parse(LOGIN_URL) else { return };
        let (r, g, b) = batlay_core::theme::window_background(theme != "light");
        let built = self
            .builder(LOGIN, WebviewUrl::External(url), false)
            .title("Connexion à TikTok")
            .inner_size(480.0, 780.0)
            .background_color(Color(r, g, b, 255))
            // https uniquement ; les fenêtres « Continuer avec Google / Apple… » aussi.
            .on_navigation(|url| url.scheme() == "https")
            .on_new_window(|url, _| if url.scheme() == "https" { NewWindowResponse::Allow } else { NewWindowResponse::Deny })
            .build();
        if let Err(e) = built {
            batlay_core::log::error(&format!("TikTok : page de connexion non ouverte ({e})"));
            return;
        }
        self.watch_login();
    }

    /// Tant que la page de connexion est ouverte : regarde toutes les 1,5 s si
    /// TikTok a posé son cookie de session ; la referme une fois connecté.
    fn watch_login(self: &Arc<Self>) {
        if self.login_watch.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(1500));
                let Some(window) = me.handle.get_webview_window(LOGIN) else {
                    // Fermée par l'utilisateur : état final de la session.
                    let logged_in = me.with_profile(Self::read_logged_in).unwrap_or(false);
                    me.notify_login(logged_in);
                    break;
                };
                if Self::read_logged_in(&window) {
                    me.notify_login(true);
                    // Laisse TikTok finir sa redirection avant de fermer.
                    std::thread::sleep(Duration::from_millis(1500));
                    let _ = window.destroy();
                    break;
                }
            }
            me.login_watch.store(false, Ordering::SeqCst);
        });
    }

    /// Efface tout ce que TikTok a stocké dans le profil de Batlay (cookies, stockage, cache).
    pub fn logout(&self) {
        if let Some(window) = self.handle.get_webview_window(LOGIN) {
            let _ = window.destroy();
        }
        // Effacement attendu jusqu'au bout : une fenêtre temporaire n'est fermée qu'après.
        let cleared = self.with_profile(|w| {
            let (done_tx, done_rx) = mpsc::channel::<Result<(), String>>();
            w.with_webview(move |webview| {
                cdp::clear_profile(webview, move |r| {
                    let _ = done_tx.send(r);
                })
            })
            .map_err(|e| e.to_string())?;
            done_rx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| Err("délai dépassé".into()))
        });
        match cleared {
            Some(Ok(())) => batlay_core::log::info("TikTok : session effacée de Batlay."),
            Some(Err(e)) => batlay_core::log::warn(&format!("TikTok : effacement de la session incomplet ({e}).")),
            None => batlay_core::log::warn("TikTok : effacement de la session impossible (fenêtre indisponible)."),
        }
    }

    // -- Page du live : affichage à la demande ----------------------------------------------

    /// « Afficher la page TikTok » : montre la page invisible telle quelle (utile si
    /// TikTok demande une vérification). Le son reste coupé.
    pub fn show_page(&self, show: bool) -> bool {
        let Some(current) = lock(&self.current).clone() else { return false };
        let Some(window) = self.handle.get_webview_window(&current.label) else { return false };
        if !show {
            hide(&self.handle, &window, &current.visible);
            return true;
        }
        let _ = window.set_focusable(true);
        let _ = window.set_skip_taskbar(false);
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
        if !current.visible.swap(true, Ordering::SeqCst) {
            emit_app(&self.handle, "tiktok:page-visible", "true".into());
        }
        true
    }
}

impl PageHost for TikTokPages {
    fn open_live_page(&self, username: &str, lite: bool, events: Sender<PageEvent>) -> Result<Box<dyn LivePage>, String> {
        let label = self.next_label("live");
        let blank = Url::parse("about:blank").map_err(|e| e.to_string())?;
        let window = self.builder(&label, WebviewUrl::External(blank), true).build().map_err(|e| format!("fenêtre du live impossible ({e})"))?;
        let visible = Arc::new(AtomicBool::new(false));

        // Fermée par l'utilisateur (une fois affichée) : remise en arrière-plan, le live continue.
        let handle = self.handle.clone();
        let on_close = visible.clone();
        let gone = events.clone();
        let own_label = label.clone();
        window.on_window_event(move |event| match event {
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(w) = handle.get_webview_window(&own_label) {
                    hide(&handle, &w, &on_close);
                }
            }
            WindowEvent::Destroyed => {
                cdp::forget(&own_label);
                let _ = gone.send(PageEvent::Gone("fenêtre fermée".into()));
            }
            _ => {}
        });

        let url = live_page_url(username);
        let attach_label = label.clone();
        let failed = events.clone();
        let attached = window.with_webview(move |webview| {
            if let Err(e) = cdp::attach_live(&attach_label, webview, &url, lite, events) {
                let _ = failed.send(PageEvent::Gone(format!("lecture du flux impossible ({e})")));
            }
        });
        if let Err(e) = attached {
            let _ = window.destroy();
            return Err(format!("lecture du flux impossible ({e})"));
        }
        *lock(&self.current) = Some(Current { label: label.clone(), visible: visible.clone() });
        Ok(Box::new(HiddenPage { handle: self.handle.clone(), label, visible, current: self.current.clone() }))
    }

    fn probe(&self, username: &str) -> Result<ProbeResult, String> {
        let _one_at_a_time = lock(&self.probing);
        let label = self.next_label("probe");
        let (loaded_tx, loaded_rx) = mpsc::channel::<()>();
        let blank = Url::parse("about:blank").map_err(|e| e.to_string())?;
        let window = self
            .builder(&label, WebviewUrl::External(blank), true)
            .on_page_load(move |_, payload| {
                if payload.event() == PageLoadEvent::Finished && payload.url().scheme() == "https" {
                    let _ = loaded_tx.send(());
                }
            })
            .build()
            .map_err(|e| format!("fenêtre de recherche impossible ({e})"))?;
        let forget_label = label.clone();
        window.on_window_event(move |event| {
            if let WindowEvent::Destroyed = event {
                cdp::forget(&forget_label);
            }
        });

        let result = (|| {
            let url = live_page_url(username);
            let attach_label = label.clone();
            window
                .with_webview(move |webview| {
                    let _ = cdp::attach_probe(&attach_label, webview, &url);
                })
                .map_err(|e| e.to_string())?;
            // Une redirection interne peut interrompre le chargement alors que la page est là : on continue.
            let _ = loaded_rx.recv_timeout(Duration::from_secs(15));

            let api_path = live_api_url(username).trim_start_matches("https://www.tiktok.com").to_string();
            let script = format!(
                r#"(async () => {{
  const el = document.getElementById("SIGI_STATE");
  let api = null;
  try {{
    const r = await fetch({path}, {{ credentials: "include", headers: {{ Accept: "application/json" }} }});
    api = {{ status: r.status, text: (await r.text()).slice(0, 200000) }};
  }} catch (e) {{ api = {{ status: 0, text: String(e) }}; }}
  return {{ sigi: el ? el.textContent.slice(0, 2000000) : null, api, href: location.href, title: document.title }};
}})()"#,
                path = Value::from(api_path).to_json()
            );
            let (done_tx, done_rx) = mpsc::channel::<Result<Value, String>>();
            window
                .with_webview(move |webview| {
                    cdp::evaluate(webview, &script, move |r| {
                        let _ = done_tx.send(r);
                    })
                })
                .map_err(|e| e.to_string())?;
            let value = match done_rx.recv_timeout(Duration::from_secs(8)) {
                Ok(Ok(v)) => v,
                Ok(Err(e)) => return Err(format!("lecture de la page impossible ({e})")),
                Err(_) => Value::Null,
            };
            if value.is_null() {
                let href = window.url().map(|u| u.to_string()).unwrap_or_default();
                return Ok(ProbeResult { api: None, sigi: None, href, title: "délai dépassé".into() });
            }
            let api = value.get("api");
            Ok(ProbeResult {
                api: api.get("status").as_u64().map(|s| (s.min(999) as u16, api.get("text").as_str().unwrap_or_default().to_string())),
                sigi: value.get("sigi").as_str().map(str::to_string),
                href: value.get("href").as_str().unwrap_or_default().to_string(),
                title: value.get("title").as_str().unwrap_or_default().to_string(),
            })
        })();
        let _ = window.destroy();
        result
    }

    fn logged_in(&self) -> bool {
        self.with_profile(Self::read_logged_in).unwrap_or(false)
    }
}
