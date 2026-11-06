//! Batlay 0.5 : application Tauri. Le fonctionnement est dans le cœur
//! (`batlay-core`) ; ici, seulement ce qui touche à Windows et aux fenêtres.

// Pas de console derrière l'application installée.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("Batlay (src-tauri) ne vise que Windows : le cœur (crates/batlay-core), lui, se compile et se teste partout.");

mod bridge;
mod cdp;
mod host;
mod tiktok_pages;
mod tls;
mod win32;

use batlay_core::app::{App, AppConfig, Deps};
use batlay_core::runtime::Rt;
use batlay_core::system_media::SystemPowerShell;
use bridge::Core;
use host::TauriHost;
use std::sync::Arc;
use std::time::Duration;
use tauri::{Manager, RunEvent};
use tiktok_pages::TikTokPages;

fn setup(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    let packaged = !cfg!(debug_assertions);
    // Profil WebView2 réservé aux pages TikTok (cookies isolés du tableau de bord).
    let tiktok_profile = handle.path().app_local_data_dir()?.join("TikTokWebView");
    let pages = Arc::new(TikTokPages::new(handle.clone(), tiktok_profile));
    let host = Arc::new(TauriHost::new(handle.clone(), pages.clone()));
    let connector = tls::TlsConnector::new()?;

    let core = App::start(
        AppConfig {
            // %APPDATA%\Batlay : le même dossier que Batlay 0.4 (réglages, overlays, images repris).
            data_dir: batlay_core::config::data_dir(),
            static_dir: host::static_dir(&handle),
            version: env!("CARGO_PKG_VERSION").into(),
            packaged,
            twitch_port: batlay_core::config::TWITCH_PORT,
            overlay_port: None,
        },
        Deps {
            connector: Arc::new(connector),
            secrets: Arc::new(win32::Dpapi),
            pages: pages.clone(),
            powershell: Box::new(SystemPowerShell),
            host: host.clone(),
            rt: Rt::production(),
        },
    );
    pages.set_core(&core);
    host.set_core(&core);
    app.manage(Core(core.clone()));

    let settings = core.invoke("config:get", &[]).map(|c| c.get("settings").clone()).unwrap_or_default();
    if packaged {
        // Aligne l'entrée « démarrer avec Windows » sur le réglage (et retire celle de l'ancienne version).
        use batlay_core::app::Host;
        host.set_launch_on_startup(settings.get("launchOnStartup").as_bool() == Some(true));
    }
    host.create_main_window(settings.get("startMinimized").as_bool() == Some(true))?;
    core.after_window();
    Ok(())
}

fn main() {
    let app = tauri::Builder::default()
        // En premier : une deuxième instance se contente de ramener la fenêtre de la première.
        .plugin(tauri_plugin_single_instance::init(|handle, _args, _cwd| host::focus_main(handle)))
        .invoke_handler(tauri::generate_handler![bridge::batlay])
        .setup(setup)
        .build(tauri::generate_context!())
        .expect("démarrage de Batlay impossible");

    app.run(|handle, event| {
        if let RunEvent::Exit = event {
            if let Some(core) = handle.try_state::<Core>() {
                let core = core.0.clone();
                // Sur un fil à part, 3 s au plus : l'arrêt ne doit jamais bloquer la fermeture.
                let (done_tx, done_rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    core.shutdown();
                    let _ = done_tx.send(());
                });
                let _ = done_rx.recv_timeout(Duration::from_secs(3));
            }
        }
    });
}
