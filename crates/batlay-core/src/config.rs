//! Réglages persistants : `%APPDATA%\Batlay\batlay-config.json`, le MÊME fichier
//! qu'utilisaient les versions Electron (0.4.x et avant). La migration vers
//! 0.5.0 est donc transparente : overlays, alertes, réglages, pseudo TikTok…
//! sont relus tels quels.
//!
//! Écriture atomique (fichier temporaire puis renommage) : une coupure de
//! courant pendant l'écriture ne laisse jamais un fichier à moitié écrit. Un
//! fichier illisible est mis de côté (`.illisible-<date>.json`) et Batlay
//! repart des valeurs par défaut au lieu de refuser de démarrer.

use crate::json::{self, Map, Value};
use crate::obj;
use crate::util::text::is_hex_color;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Port du serveur Twitch : fixe (URI de redirection déclarée chez Twitch).
pub const TWITCH_PORT: u16 = 4000;
pub const DEFAULT_OVERLAY_PORT: u16 = 3000;

/// Chiffrement lié à la session Windows (DPAPI). `None` = indisponible : on
/// refuse alors d'écrire un secret en clair.
pub trait SecretBox: Send + Sync {
    fn protect(&self, data: &[u8]) -> Option<Vec<u8>>;
    fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>>;
}

/// Aucun chiffrement disponible (tests, plateformes non Windows).
pub struct NoSecretBox;
impl SecretBox for NoSecretBox {
    fn protect(&self, _: &[u8]) -> Option<Vec<u8>> {
        None
    }
    fn unprotect(&self, _: &[u8]) -> Option<Vec<u8>> {
        None
    }
}

/// Fichier JSON clé → valeur, partagé par tout Batlay.
pub struct Store {
    path: PathBuf,
    data: Mutex<Map>,
}

impl Store {
    /// Ouvre (ou crée) le fichier ; les clés absentes prennent la valeur par défaut.
    pub fn open(path: impl Into<PathBuf>, defaults: Value) -> Store {
        let path = path.into();
        let mut data = defaults.as_object().cloned().unwrap_or_default();
        match std::fs::read(&path) {
            Ok(bytes) => match json::parse_bytes(&bytes) {
                Ok(Value::Object(stored)) => data.merge(&stored),
                _ => {
                    let stamp = crate::util::now_ms();
                    let aside = path.with_extension(format!("illisible-{stamp}.json"));
                    let _ = std::fs::rename(&path, aside);
                }
            },
            Err(_) => {}
        }
        Store { path, data: Mutex::new(data) }
    }

    /// Store en mémoire seulement (tests).
    pub fn memory(defaults: Value) -> Store {
        Store { path: PathBuf::new(), data: Mutex::new(defaults.as_object().cloned().unwrap_or_default()) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn get(&self, key: &str) -> Value {
        self.data.lock().unwrap_or_else(|e| e.into_inner()).get(key).cloned().unwrap_or(Value::Null)
    }

    pub fn set(&self, key: &str, value: Value) {
        let snapshot = {
            let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
            data.insert(key, value);
            Value::Object(data.clone())
        };
        self.write(&snapshot);
    }

    /// Modifie un objet en place (`f` reçoit l'objet, créé vide si besoin).
    pub fn update(&self, key: &str, f: impl FnOnce(&mut Map)) {
        let snapshot = {
            let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
            let mut current = data.get(key).and_then(|v| v.as_object().cloned()).unwrap_or_default();
            f(&mut current);
            data.insert(key, Value::Object(current));
            Value::Object(data.clone())
        };
        self.write(&snapshot);
    }

    pub fn snapshot(&self) -> Value {
        Value::Object(self.data.lock().unwrap_or_else(|e| e.into_inner()).clone())
    }

    fn write(&self, snapshot: &Value) {
        if self.path.as_os_str().is_empty() {
            return;
        }
        if let Err(e) = write_atomic(&self.path, snapshot.to_json_pretty().as_bytes()) {
            crate::log::warn(&format!("Réglages non enregistrés ({}) : {e}", self.path.display()));
        }
    }
}

/// Écrit `bytes` dans `path` sans jamais laisser de fichier à moitié écrit.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Valeurs par défaut de `batlay-config.json` (mêmes clés qu'en 0.4.x).
pub fn default_config() -> Value {
    obj! {
        "overlays" => Value::Array(Vec::new()),
        "activeOverlayId" => Value::Null,
        "settings" => default_settings(),
        "spotify" => obj! { "clientId" => Value::Null },
        "alerts" => Value::Null,
        "alertsHookKey" => Value::Null,
        "tiktokLive" => Value::Null,
        "localAlias" => obj! { "autoSetupAttempted" => false },
    }
}

pub fn default_settings() -> Value {
    obj! {
        "launchOnStartup" => false,
        "startMinimized" => false,
        "theme" => "dark",
        "language" => "fr",
        "overlayServerPort" => DEFAULT_OVERLAY_PORT,
        "preferredSystemMediaAppId" => Value::Null,
        "accentColor" => Value::Null,
        "density" => "comfortable",
        "uiMotion" => "full",
    }
}

pub fn normalize_theme(v: &Value) -> &'static str {
    match v.as_str() {
        Some("light") => "light",
        Some("system") => "system",
        _ => "dark",
    }
}

/// Port d'overlay valide : 1024–65535, jamais celui de Twitch.
pub fn normalize_port(v: &Value, fallback: u16) -> u16 {
    match v.as_u64() {
        Some(p) if (1024..=65535).contains(&p) && p != u64::from(TWITCH_PORT) => p as u16,
        _ => fallback,
    }
}

/// Réglages relus du disque ou reçus du tableau de bord : seuls les champs
/// connus, chacun revalidé (le tableau de bord est de confiance limitée).
pub fn normalize_settings(raw: &Value) -> Value {
    let d = default_settings();
    let pick_bool = |key: &str| raw.get(key).as_bool().unwrap_or_else(|| d.get(key).as_bool().unwrap_or(false));
    let app_id = raw.get("preferredSystemMediaAppId").as_str().filter(|s| !s.is_empty() && s.len() <= 300 && !s.chars().any(|c| c.is_control())).map(str::to_string);
    let accent = raw.get("accentColor").as_str().filter(|s| is_hex_color(s)).map(str::to_string);
    obj! {
        "launchOnStartup" => pick_bool("launchOnStartup"),
        "startMinimized" => pick_bool("startMinimized"),
        "theme" => normalize_theme(raw.get("theme")),
        "language" => if raw.get("language").as_str() == Some("en") { "en" } else { "fr" },
        "overlayServerPort" => normalize_port(raw.get("overlayServerPort"), DEFAULT_OVERLAY_PORT),
        "preferredSystemMediaAppId" => app_id,
        "accentColor" => accent,
        "density" => if raw.get("density").as_str() == Some("compact") { "compact" } else { "comfortable" },
        "uiMotion" => if raw.get("uiMotion").as_str() == Some("reduced") { "reduced" } else { "full" },
    }
}

/// Applique un changement venu du tableau de bord : `{ ...avant, ...changement }`
/// puis revalidation. Le port de Twitch n'est jamais accepté pour l'overlay.
pub fn merge_settings(previous: &Value, patch: &Value) -> Value {
    let mut merged = previous.clone();
    if let Some(p) = patch.as_object() {
        for (k, v) in p.iter() {
            if k == "overlayServerPort" && v.as_u64() == Some(u64::from(TWITCH_PORT)) {
                continue;
            }
            merged.set(k.clone(), v.clone());
        }
    }
    normalize_settings(&merged)
}

/// Dossier des données de Batlay : `%APPDATA%\Batlay` (identique aux versions Electron).
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BATLAY_DATA_DIR") {
        return PathBuf::from(dir);
    }
    #[cfg(windows)]
    if let Some(appdata) = std::env::var_os("APPDATA") {
        return PathBuf::from(appdata).join("Batlay");
    }
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg).join("Batlay");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config").join("Batlay");
    }
    PathBuf::from("Batlay")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("batlay-test-{name}-{}", crate::util::random::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn reads_an_electron_config_file_as_is() {
        let dir = temp_dir("cfg");
        let path = dir.join("batlay-config.json");
        // Extrait d'un vrai fichier electron-store (0.4.1).
        std::fs::write(
            &path,
            r##"{"overlays":[{"id":"abc","name":"Glass"}],"activeOverlayId":"abc","settings":{"launchOnStartup":false,"startMinimized":false,"theme":"system","language":"fr","overlayServerPort":3000,"preferredSystemMediaAppId":"Spotify.exe","accentColor":"#37E29A"},"spotify":{"clientId":"0123","encryptedRefreshToken":"djEw..."},"alertsHookKey":"0123456789abcdef0123456789abcdef","tiktokLive":{"username":"adil.bli","enabled":true,"giftMinDiamonds":1,"mode":"browser"}}"##,
        )
        .unwrap();
        let store = Store::open(&path, default_config());
        assert_eq!(store.get("overlays").at(0).get("name").as_str(), Some("Glass"));
        assert_eq!(store.get("tiktokLive").get("username").as_str(), Some("adil.bli"));
        assert!(store.get("localAlias").is_object(), "clé absente = valeur par défaut");
        let settings = normalize_settings(&store.get("settings"));
        assert_eq!(settings.get("theme").as_str(), Some("system"));
        assert_eq!(settings.get("density").as_str(), Some("comfortable"), "réglage ajouté en 0.3.9 : défaut");
        store.set("activeOverlayId", Value::Null);
        let reread = Store::open(&path, default_config());
        assert!(reread.get("activeOverlayId").is_null());
        assert_eq!(reread.get("alertsHookKey").as_str(), Some("0123456789abcdef0123456789abcdef"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_file_is_set_aside() {
        let dir = temp_dir("corrupt");
        let path = dir.join("batlay-config.json");
        std::fs::write(&path, "{ pas du json").unwrap();
        let store = Store::open(&path, default_config());
        assert_eq!(store.get("settings").get("overlayServerPort").as_u64(), Some(3000));
        let aside: Vec<_> = std::fs::read_dir(&dir).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().contains("illisible")).collect();
        assert_eq!(aside.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_from_the_dashboard_are_revalidated() {
        let before = default_settings();
        let next = merge_settings(&before, &obj! { "theme" => "<script>", "overlayServerPort" => 4000, "density" => "compact", "accentColor" => "red", "evil" => true });
        assert_eq!(next.get("theme").as_str(), Some("dark"));
        assert_eq!(next.get("overlayServerPort").as_u64(), Some(3000));
        assert_eq!(next.get("density").as_str(), Some("compact"));
        assert!(next.get("accentColor").is_null());
        assert!(next.get("evil").is_null(), "champ inconnu ignoré");
        let next = merge_settings(&next, &obj! { "overlayServerPort" => 3100, "accentColor" => "#9B6BFF" });
        assert_eq!((next.get("overlayServerPort").as_u64(), next.get("accentColor").as_str()), (Some(3100), Some("#9B6BFF")));
        assert_eq!(normalize_port(&Value::from(80), 3000), 3000);
    }
}
