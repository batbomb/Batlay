//! Outil de développement : fait tourner le cœur de Batlay SANS fenêtre.
//!
//!   batlay-serve <dossier dist> [dossier de données]
//!
//! Les serveurs démarrent comme dans l'application (overlays sur le port des
//! réglages, Twitch sur 4000). Chaque ligne JSON lue sur l'entrée standard est
//! une commande du tableau de bord : `{"cmd":"overlay:broadcast-state","args":[…]}` ;
//! la réponse est écrite sur la sortie standard (`{"ok":…}`), comme les événements
//! du cœur (`{"event":…,"payload":…}`). Sert aux tests des pages
//! d'overlay dans un vrai Chromium (comme celui d'OBS). Pas de TLS : Twitch,
//! Spotify et TikTok n'y fonctionnent pas.

use batlay_core::app::{App, AppConfig, Deps, Host};
use batlay_core::config::NoSecretBox;
use batlay_core::json::{self, Value};
use batlay_core::net::client::PlainConnector;
use batlay_core::obj;
use batlay_core::runtime::Rt;
use batlay_core::system_media::SystemPowerShell;
use batlay_core::tiktok::browser::{LivePage, PageEvent, PageHost, ProbeResult};
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::Arc;

struct ConsoleHost;
impl Host for ConsoleHost {
    fn emit(&self, channel: &str, payload: Value) {
        // Une ligne JSON par événement, sur la sortie standard (comme les réponses, qui ont « ok »).
        println!("{}", obj! { "event" => channel, "payload" => payload }.to_json());
    }
    fn open_external(&self, url: &str) -> Result<(), String> {
        eprintln!("[ouvrir] {url}");
        Ok(())
    }
    fn open_live_window(&self) -> bool {
        false
    }
    fn set_live_always_on_top(&self, _: bool) -> bool {
        false
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

struct NoPages;
impl PageHost for NoPages {
    fn open_live_page(&self, _: &str, _: bool, _: Sender<PageEvent>) -> Result<Box<dyn LivePage>, String> {
        Err("pas de fenêtre dans batlay-serve".into())
    }
    fn probe(&self, _: &str) -> Result<ProbeResult, String> {
        Err("pas de fenêtre dans batlay-serve".into())
    }
    fn logged_in(&self) -> bool {
        false
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let static_dir = PathBuf::from(args.next().unwrap_or_else(|| "dist".into()));
    let data_dir = args.next().map(PathBuf::from).unwrap_or_else(batlay_core::config::data_dir);
    let app = App::start(
        AppConfig { data_dir, static_dir, version: env!("CARGO_PKG_VERSION").into(), packaged: false, twitch_port: 4000, overlay_port: None },
        Deps {
            connector: Arc::new(PlainConnector),
            secrets: Arc::new(NoSecretBox),
            pages: Arc::new(NoPages),
            powershell: Box::new(SystemPowerShell),
            host: Arc::new(ConsoleHost),
            rt: Rt::production(),
        },
    );
    println!("{}", obj! { "ready" => true, "version" => app.version() }.to_json());
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match json::parse(&line) {
            Ok(v) => match v.get("cmd").as_str() {
                Some(cmd) => match app.invoke(cmd, v.get("args").items()) {
                    Ok(r) => obj! { "ok" => true, "result" => r },
                    Err(e) => obj! { "ok" => false, "error" => e },
                },
                None => obj! { "ok" => false, "error" => "champ « cmd » manquant" },
            },
            Err(e) => obj! { "ok" => false, "error" => e.to_string() },
        };
        println!("{}", reply.to_json());
    }
    app.shutdown();
}
