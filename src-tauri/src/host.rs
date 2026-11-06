//! Ce que l'application fournit au cœur (`batlay_core::app::Host`) : fenêtres du
//! tableau de bord, événements vers ces fenêtres, thème, ouverture d'adresses,
//! démarrage avec Windows, lancement de l'installateur d'une mise à jour.
//!
//! Règle de sûreté : le fil principal (fenêtres) n'attend JAMAIS le cœur. Tout
//! appel au cœur venant d'un événement de fenêtre part sur un fil à part, car le
//! cœur, lui, peut attendre le fil principal (création d'une fenêtre, cookies).

use crate::tiktok_pages::TikTokPages;
use crate::win32;
use batlay_core::app::{App, Host};
use batlay_core::json::Value;
use serde_json::value::RawValue;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;
use tauri::webview::{Color, NewWindowResponse, PageLoadEvent};
use tauri::{AppHandle, Emitter, Manager, Theme, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent, Wry};

pub const MAIN: &str = "main";
pub const LIVE: &str = "live";

/// Options de WebView2, IDENTIQUES pour toutes les fenêtres (WebView2 l'exige
/// pour des fenêtres qui partagent un même dossier de données).
/// - les trois premières désactivations reprennent celles de Tauri par défaut ;
/// - CalculateNativeWinOcclusion et les options « background » : une fenêtre
///   réduite ou cachée garde ses minuteurs à pleine vitesse. Le suivi de la
///   musique (toutes les 2 s) et la diffusion vers OBS vivent dans le tableau de
///   bord : sans ceci, ils ralentiraient dès que Batlay est réduit pendant un live
///   (même rôle que `backgroundThrottling: false` sous Electron). Idem pour la
///   page TikTok invisible ;
/// - IntensiveWakeUpThrottling : sans ceci, au bout de 5 min cachée, une page ne
///   serait plus réveillée qu'une fois par minute.
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,CalculateNativeWinOcclusion,IntensiveWakeUpThrottling --disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows";

/// Délai au bout duquel la fenêtre principale s'affiche même si la page n'a pas
/// signalé la fin de son chargement (une fenêtre invisible serait inatteignable).
const REVEAL_SAFETY: Duration = Duration::from_secs(5);

/// Envoie un événement (déjà en JSON) aux fenêtres du tableau de bord.
pub fn emit_app(handle: &AppHandle, channel: &str, json: String) {
    let Ok(raw) = RawValue::from_string(json) else {
        return;
    };
    for label in [MAIN, LIVE] {
        let _ = handle.emit_to(label, channel, raw.clone());
    }
}

/// Adresse appartenant à l'application (pages du tableau de bord) ?
pub fn is_app_url(url: &Url) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => url.host_str() == Some("tauri.localhost") || (cfg!(debug_assertions) && url.host_str() == Some("localhost") && url.port() == Some(5173)),
        _ => false,
    }
}

fn tauri_theme(preference: &str) -> Option<Theme> {
    match preference {
        "light" => Some(Theme::Light),
        "system" => None,
        _ => Some(Theme::Dark),
    }
}

pub struct TauriHost {
    handle: AppHandle,
    pages: Arc<TikTokPages>,
    core: OnceLock<Weak<App>>,
    /// Réglage du thème (« dark », « light », « system »), appliqué aux nouvelles fenêtres.
    theme: Mutex<String>,
    exe: PathBuf,
}

impl TauriHost {
    pub fn new(handle: AppHandle, pages: Arc<TikTokPages>) -> TauriHost {
        TauriHost { handle, pages, core: OnceLock::new(), theme: Mutex::new("dark".into()), exe: std::env::current_exe().unwrap_or_default() }
    }

    pub fn set_core(&self, core: &Arc<App>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    fn core(&self) -> Option<Arc<App>> {
        self.core.get()?.upgrade()
    }

    fn preference(&self) -> String {
        self.theme.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn resolved_dark(&self) -> bool {
        match self.preference().as_str() {
            "light" => false,
            "system" => win32::system_prefers_dark(),
            _ => true,
        }
    }

    /// Script exécuté avant toute page du tableau de bord : thème déjà résolu (aucun
    /// flash au premier rendu) et version. Remplace l'appel synchrone d'Electron.
    fn initial_script(&self) -> String {
        let theme = self.core().map(|c| c.initial_theme().to_json()).unwrap_or_else(|| r#"{"preference":"dark","resolved":"dark"}"#.into());
        let version = Value::from(env!("CARGO_PKG_VERSION")).to_json();
        format!("Object.defineProperty(window,'__BATLAY_INITIAL__',{{value:Object.freeze({{theme:Object.freeze({theme}),version:{version}}}),writable:false,configurable:false}});")
    }

    /// Fenêtre du tableau de bord (principale ou Live) : mêmes protections pour les deux.
    fn app_window<'a>(&'a self, label: &str, path: &str, title: &str) -> WebviewWindowBuilder<'a, Wry, AppHandle> {
        let (r, g, b) = batlay_core::theme::window_background(self.resolved_dark());
        WebviewWindowBuilder::new(&self.handle, label, WebviewUrl::App(path.into()))
            .title(title)
            .background_color(Color(r, g, b, 255))
            .theme(tauri_theme(&self.preference()))
            .additional_browser_args(BROWSER_ARGS)
            // Batlay 0.5 dessine sa propre barre de titre (src/layouts/TitleBar.tsx) : pas de
            // cadre Windows. L'ombre garde le contour, les coins arrondis de Windows 11 et le
            // redimensionnement par les bords.
            .decorations(false)
            .shadow(true)
            .initialization_script(self.initial_script())
            // Le tableau de bord ne quitte jamais ses propres pages et n'ouvre aucune fenêtre :
            // les liens externes passent par le cœur (liste d'adresses autorisées).
            .on_navigation(is_app_url)
            .on_new_window(|_, _| NewWindowResponse::Deny)
            .devtools(true)
    }

    /// Fenêtre principale : créée cachée, affichée dès le premier rendu (ou au bout
    /// de 5 s quoi qu'il arrive), réduite ensuite si « Démarrer réduit » est coché.
    pub fn create_main_window(&self, start_minimized: bool) -> tauri::Result<()> {
        let revealed = Arc::new(AtomicBool::new(false));
        let reveal = {
            let revealed = revealed.clone();
            move |w: &WebviewWindow| {
                if revealed.swap(true, Ordering::SeqCst) {
                    return;
                }
                let _ = w.show();
                if start_minimized {
                    let _ = w.minimize();
                } else {
                    let _ = w.set_focus();
                }
            }
        };
        let on_load = reveal.clone();
        let window = self
            .app_window(MAIN, "index.html", "Batlay")
            .inner_size(1280.0, 800.0)
            .min_inner_size(1024.0, 700.0)
            .visible(false)
            .on_page_load(move |w, payload| {
                if payload.event() == PageLoadEvent::Finished {
                    on_load(&w);
                }
            })
            .build()?;
        let safety = window.clone();
        std::thread::spawn(move || {
            std::thread::sleep(REVEAL_SAFETY);
            reveal(&safety);
        });
        let handle = self.handle.clone();
        let core = self.core.get().cloned();
        window.on_window_event(move |event| match event {
            // Fermer la fenêtre principale ferme Batlay (fenêtre Live et pages TikTok comprises).
            WindowEvent::Destroyed => handle.exit(0),
            // Windows passe en clair / sombre (reçu seulement si le thème suit le système).
            WindowEvent::ThemeChanged(_) => {
                if let Some(core) = core.as_ref().and_then(Weak::upgrade) {
                    std::thread::spawn(move || core.system_theme_changed());
                }
            }
            _ => {}
        });
        Ok(())
    }

    fn focus(window: &WebviewWindow) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// Batlay relancé alors qu'il tourne déjà : on ramène la fenêtre existante.
pub fn focus_main(handle: &AppHandle) {
    if let Some(window) = handle.get_webview_window(MAIN) {
        TauriHost::focus(&window);
    }
}

impl Host for TauriHost {
    fn emit(&self, channel: &str, payload: Value) {
        emit_app(&self.handle, channel, payload.to_json());
    }

    fn open_external(&self, url: &str) -> Result<(), String> {
        win32::shell_open(url, None)
    }

    fn open_live_window(&self) -> bool {
        if let Some(window) = self.handle.get_webview_window(LIVE) {
            TauriHost::focus(&window);
            return true;
        }
        match self.app_window(LIVE, "index.html#/live?window=1", "Batlay · Live").inner_size(1100.0, 760.0).min_inner_size(520.0, 420.0).build() {
            Ok(_) => true,
            Err(e) => {
                batlay_core::log::error(&format!("Fenêtre Live : ouverture impossible ({e})"));
                false
            }
        }
    }

    fn set_live_always_on_top(&self, on: bool) -> bool {
        self.handle.get_webview_window(LIVE).is_some_and(|w| w.set_always_on_top(on).is_ok())
    }

    fn tiktok_login(&self) {
        self.pages.open_login(&self.preference());
    }

    fn tiktok_logout(&self) {
        self.pages.logout();
    }

    fn tiktok_show_page(&self, show: bool) -> bool {
        self.pages.show_page(show)
    }

    fn apply_theme(&self, preference: &str) {
        *self.theme.lock().unwrap_or_else(|e| e.into_inner()) = preference.to_string();
        let theme = tauri_theme(preference);
        for label in [MAIN, LIVE, crate::tiktok_pages::LOGIN] {
            if let Some(window) = self.handle.get_webview_window(label) {
                let _ = window.set_theme(theme);
            }
        }
    }

    fn system_prefers_dark(&self) -> bool {
        win32::system_prefers_dark()
    }

    fn set_launch_on_startup(&self, enabled: bool) {
        win32::set_run_at_login(enabled, &self.exe);
    }

    fn run_installer_and_quit(&self, path: &Path) {
        // /S : installation silencieuse, sans fenêtre de l'installateur (Batlay a déjà affiché
        // son propre écran « Installation de la mise à jour ») ; /R : relance Batlay ensuite.
        match win32::shell_open(&path.to_string_lossy(), Some("/S /R")) {
            Ok(()) => {
                batlay_core::log::info("Installateur de la mise à jour lancé : fermeture de Batlay.");
                self.handle.exit(0);
            }
            Err(e) => batlay_core::log::error(&format!("Mise à jour : installateur non lancé ({e})")),
        }
    }
}

/// Dossier des pages servies à OBS (overlay.html, alerts.html, twitch-overlay.html…).
pub fn static_dir(handle: &AppHandle) -> PathBuf {
    if let Ok(resources) = handle.path().resource_dir() {
        let dist = resources.join("dist");
        if dist.join("overlay.html").is_file() {
            return dist;
        }
    }
    // Développement (`tauri dev`) : le build Vite du dépôt.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("dist")
}
