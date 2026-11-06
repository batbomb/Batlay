//! La commande unique du tableau de bord : `batlay(cmd, args)`, transmise telle
//! quelle au cœur (`App::invoke`), qui tient la liste fermée des commandes et
//! revalide chaque argument. Remplace le preload et les `ipcMain.handle` d'Electron.

use crate::host::{LIVE, MAIN};
use batlay_core::app::App;
use std::sync::Arc;

/// Cœur de Batlay, partagé avec Tauri (`app.manage`).
pub struct Core(pub Arc<App>);

/// Taille maximale des arguments d'une commande (une image importée fait au plus 12 Mo, en base64).
const MAX_ARGS_BYTES: usize = 24 * 1024 * 1024;

#[tauri::command]
pub async fn batlay(webview: tauri::Webview, core: tauri::State<'_, Core>, cmd: String, args: serde_json::Value) -> Result<tauri::ipc::Response, String> {
    // Double garde : capabilities/main.json ne l'autorise déjà qu'à ces deux fenêtres.
    if webview.label() != MAIN && webview.label() != LIVE {
        return Err("Commande refusée.".into());
    }
    if cmd.len() > 64 {
        return Err("Commande inconnue.".into());
    }
    let text = serde_json::to_string(&args).map_err(|e| e.to_string())?;
    if text.len() > MAX_ARGS_BYTES {
        return Err("Arguments trop volumineux.".into());
    }
    let args = match batlay_core::json::parse(&text).map_err(|e| e.to_string())? {
        batlay_core::json::Value::Array(list) => list,
        batlay_core::json::Value::Null => Vec::new(),
        _ => return Err("Arguments invalides.".into()),
    };
    let app = core.0.clone();
    // Le cœur peut attendre le réseau ou une fenêtre : jamais sur le fil principal.
    let result = tauri::async_runtime::spawn_blocking(move || app.invoke(&cmd, &args)).await.map_err(|e| format!("Commande interrompue : {e}"))?;
    // Réponse déjà en JSON : transmise sans conversion.
    result.map(|value| tauri::ipc::Response::new(value.to_json()))
}
