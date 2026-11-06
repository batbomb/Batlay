//! Mises à jour depuis les « Releases » GitHub.
//!
//! Même principe qu'avant la 0.5.0 : Batlay ne télécharge JAMAIS rien sans
//! clic. Une recherche (automatique au démarrage, ou à la demande) signale
//! seulement qu'une version existe ; téléchargement puis installation sont
//! deux clics. Le fichier `latest.yml` de la release (format electron-builder,
//! conservé : c'est lui que lisent aussi les versions 0.4.x pour migrer)
//! donne le nom de l'installateur et son empreinte SHA-512, vérifiée avant
//! de lancer quoi que ce soit.

use crate::json::Value;
use crate::net::client::{Connector, HttpClient, HttpRequest};
use crate::net::http::{read_response_head, Headers};
use crate::obj;
use crate::util::encoding::base64_decode;
use crate::util::hash::Sha512;
use crate::util::url::Url;
use std::io::{BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const REPO_OWNER: &str = "BatStream-off";
pub const REPO_NAME: &str = "Batlay";

pub fn releases_url() -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases")
}

pub fn latest_yml_url() -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/latest/download/latest.yml")
}

pub fn asset_url(version: &str, file: &str) -> String {
    format!("https://github.com/{REPO_OWNER}/{REPO_NAME}/releases/download/v{version}/{}", crate::util::url::encode_component(file))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    pub path: String,
    /// SHA-512 en Base64 (format electron-builder).
    pub sha512: String,
    pub size: Option<u64>,
}

/// Lecture du sous-ensemble YAML de `latest.yml` (clés simples et liste `files`).
pub fn parse_latest_yml(text: &str) -> Option<Release> {
    let mut top: Vec<(String, String)> = Vec::new();
    let mut files: Vec<Vec<(String, String)>> = Vec::new();
    let mut in_files = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indented = line.starts_with(' ');
        let trimmed = line.trim_start();
        if !indented {
            in_files = trimmed.starts_with("files:");
            if let Some((k, v)) = trimmed.split_once(':') {
                top.push((k.trim().into(), unquote(v.trim())));
            }
            continue;
        }
        if in_files {
            let item = trimmed.strip_prefix("- ");
            if item.is_some() {
                files.push(Vec::new());
            }
            let kv = item.unwrap_or(trimmed);
            if let (Some(last), Some((k, v))) = (files.last_mut(), kv.split_once(':')) {
                last.push((k.trim().into(), unquote(v.trim())));
            }
        }
    }
    let get = |list: &[(String, String)], key: &str| list.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()).filter(|v| !v.is_empty());
    let version = get(&top, "version")?;
    let exe = files.iter().find(|f| get(f, "url").is_some_and(|u| u.to_ascii_lowercase().ends_with(".exe")));
    let (path, sha512, size) = match exe {
        Some(f) => (get(f, "url")?, get(f, "sha512")?, get(f, "size").and_then(|s| s.parse().ok())),
        None => (get(&top, "path")?, get(&top, "sha512")?, None),
    };
    // Nom de fichier simple uniquement (jamais de chemin ni d'adresse).
    if path.contains(['/', '\\']) || path.contains("..") || !path.to_ascii_lowercase().ends_with(".exe") {
        return None;
    }
    Some(Release { version, path, sha512, size })
}

fn unquote(v: &str) -> String {
    let v = v.trim();
    if v.len() >= 2 && ((v.starts_with('\'') && v.ends_with('\'')) || (v.starts_with('"') && v.ends_with('"'))) {
        v[1..v.len() - 1].to_string()
    } else {
        v.to_string()
    }
}

/// `a` plus récente que `b` ? (x.y.z, suffixes de préversion ignorés sauf à égalité).
pub fn is_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> (Vec<u64>, bool) {
        let s = s.trim().trim_start_matches('v');
        let (core, pre) = match s.split_once('-') {
            Some((c, _)) => (c, true),
            None => (s, false),
        };
        (core.split('.').map(|p| p.parse().unwrap_or(0)).collect(), pre)
    };
    let (va, pa) = parse(a);
    let (vb, pb) = parse(b);
    for i in 0..va.len().max(vb.len()) {
        let (x, y) = (va.get(i).copied().unwrap_or(0), vb.get(i).copied().unwrap_or(0));
        if x != y {
            return x > y;
        }
    }
    !pa && pb
}

/// Message lisible pour une erreur réseau / GitHub.
pub fn describe_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if ["connexion impossible", "adresse introuvable", "délai dépassé", "erreur réseau", "dns", "refused"].iter().any(|k| lower.contains(k)) {
        return "Impossible de joindre GitHub. Vérifiez votre connexion Internet.".into();
    }
    if lower.contains("404") || lower.contains("latest.yml") {
        return "Aucune version publiée trouvée sur GitHub pour ce dépôt.".into();
    }
    if lower.contains("403") || lower.contains("rate limit") {
        return "GitHub limite temporairement les requêtes. Réessayez dans quelques minutes.".into();
    }
    let first = raw.lines().next().unwrap_or("").trim();
    if first.is_empty() {
        "Erreur inconnue pendant la mise à jour.".into()
    } else {
        first.chars().take(200).collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Unavailable(&'static str),
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(u8),
    Downloaded(Release, PathBuf),
    Error(String),
}

impl Status {
    pub fn to_json(&self) -> Value {
        match self {
            Status::Unavailable(reason) => obj! { "state" => "unavailable", "reason" => *reason },
            Status::Idle => obj! { "state" => "idle" },
            Status::Checking => obj! { "state" => "checking" },
            Status::UpToDate => obj! { "state" => "up-to-date" },
            Status::Available(r) => obj! { "state" => "available", "version" => r.version.clone() },
            Status::Downloading(p) => obj! { "state" => "downloading", "percent" => *p },
            Status::Downloaded(r, _) => obj! { "state" => "downloaded", "version" => r.version.clone() },
            Status::Error(m) => obj! { "state" => "error", "message" => m.clone() },
        }
    }
}

/// Téléchargement en flux (barre de progression, empreinte calculée au fil de l'eau).
pub fn download(connector: &dyn Connector, url: &str, dest: &Path, expected_sha512: &str, mut progress: impl FnMut(u8)) -> Result<(), String> {
    let expected = base64_decode(expected_sha512).filter(|e| e.len() == 64).ok_or("Empreinte de l'installateur absente ou invalide.")?;
    let mut url = Url::parse(url).ok_or("Adresse invalide.")?;
    for _ in 0..6 {
        let mut stream = connector.connect(&url.host, url.port, url.is_tls(), Duration::from_secs(15)).map_err(|e| format!("connexion impossible ({e})"))?;
        stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| e.to_string())?;
        let head = format!("GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: Batlay-Updater\r\nAccept: application/octet-stream\r\n\r\n", url.path_and_query, url.host_header());
        stream.write_all(head.as_bytes()).map_err(|e| format!("erreur réseau ({e})"))?;
        let mut reader = BufReader::new(stream);
        let (status, headers): (u16, Headers) = read_response_head(&mut reader).map_err(|_| "erreur réseau (réponse illisible)".to_string())?;
        if matches!(status, 301 | 302 | 303 | 307 | 308) {
            let next = headers.get("Location").and_then(|l| url.join(l)).ok_or("redirection invalide")?;
            if !next.is_tls() {
                return Err("redirection vers http refusée".into());
            }
            url = next;
            continue;
        }
        if status != 200 {
            return Err(format!("HTTP {status}"));
        }
        if headers.has_token("Transfer-Encoding", "chunked") {
            return Err("réponse inattendue de GitHub (chunked)".into());
        }
        let total: Option<u64> = headers.get("Content-Length").and_then(|l| l.trim().parse().ok());
        if total.is_some_and(|t| t > 500 * 1024 * 1024) {
            return Err("installateur trop volumineux".into());
        }
        let tmp = dest.with_extension("part");
        let mut file = std::fs::File::create(&tmp).map_err(|e| format!("écriture impossible ({e})"))?;
        let mut hasher = Sha512::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut done: u64 = 0;
        let mut last_percent = 255u8;
        loop {
            let n = reader.read(&mut buf).map_err(|e| format!("erreur réseau ({e})"))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| format!("écriture impossible ({e})"))?;
            done += n as u64;
            if let Some(t) = total.filter(|t| *t > 0) {
                let p = ((done * 100) / t).min(100) as u8;
                if p != last_percent {
                    last_percent = p;
                    progress(p);
                }
            }
        }
        file.sync_all().map_err(|e| e.to_string())?;
        drop(file);
        if total.is_some_and(|t| t != done) {
            let _ = std::fs::remove_file(&tmp);
            return Err("téléchargement incomplet".into());
        }
        if hasher.finish()[..] != expected[..] {
            let _ = std::fs::remove_file(&tmp);
            return Err("L'installateur téléchargé ne correspond pas à l'empreinte publiée : il a été supprimé.".into());
        }
        std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
        return Ok(());
    }
    Err("trop de redirections".into())
}

pub struct Updater {
    current_version: String,
    connector: Arc<dyn Connector>,
    download_dir: PathBuf,
    status: Mutex<Status>,
    on_status: Arc<dyn Fn(&Status) + Send + Sync>,
}

impl Updater {
    /// `packaged` faux (développement) : rien à mettre à jour.
    pub fn new(current_version: &str, packaged: bool, connector: Arc<dyn Connector>, download_dir: PathBuf, on_status: Arc<dyn Fn(&Status) + Send + Sync>) -> Updater {
        let status = if packaged { Status::Idle } else { Status::Unavailable("dev") };
        Updater { current_version: current_version.into(), connector, download_dir, status: Mutex::new(status), on_status }
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, s: Status) {
        *self.status.lock().unwrap_or_else(|e| e.into_inner()) = s.clone();
        (self.on_status)(&s);
    }

    /// `silent` : recherche de fond (démarrage), une erreur réseau n'est pas montrée.
    pub fn check(&self, silent: bool) -> Status {
        let previous = self.status();
        if matches!(previous, Status::Unavailable(_) | Status::Checking | Status::Downloading(_) | Status::Downloaded(..)) {
            return previous;
        }
        self.set(Status::Checking);
        let client = HttpClient::new(self.connector.clone(), "Batlay-Updater");
        let result = client.send(&HttpRequest::get(latest_yml_url()).timeout(Duration::from_secs(15)).max_body(64 * 1024));
        let next = match result {
            Ok(r) if r.status == 200 => match parse_latest_yml(&r.text()) {
                Some(release) if is_newer(&release.version, &self.current_version) => Status::Available(release),
                Some(_) => Status::UpToDate,
                None => Status::Error("Fichier latest.yml illisible dans la dernière release.".into()),
            },
            Ok(r) => Status::Error(describe_error(&format!("HTTP {}", r.status))),
            Err(e) => Status::Error(describe_error(&e.to_string())),
        };
        if silent && matches!(next, Status::Error(_)) {
            crate::log::warn(&format!("Recherche de mise à jour échouée : {next:?}"));
            self.set(previous);
        } else {
            self.set(next);
        }
        self.status()
    }

    pub fn download(&self) -> Status {
        let Status::Available(release) = self.status() else { return self.status() };
        self.set(Status::Downloading(0));
        let _ = std::fs::create_dir_all(&self.download_dir);
        let dest = self.download_dir.join(&release.path);
        let result = download(self.connector.as_ref(), &asset_url(&release.version, &release.path), &dest, &release.sha512, |p| self.set(Status::Downloading(p)));
        match result {
            Ok(()) => self.set(Status::Downloaded(release, dest)),
            Err(e) => {
                crate::log::warn(&format!("Téléchargement de mise à jour échoué : {e}"));
                self.set(Status::Error(describe_error(&e)));
            }
        }
        self.status()
    }

    /// Chemin de l'installateur vérifié, prêt à être lancé (l'application se ferme ensuite).
    pub fn installer(&self) -> Option<PathBuf> {
        match self.status() {
            Status::Downloaded(_, path) if path.exists() => Some(path),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::client::PlainConnector;
    use crate::net::http::{Request, Response};
    use crate::net::server::{Handler, HttpServer};
    use crate::util::encoding::base64_encode;
    use crate::util::hash::sha512;

    const YML: &str = "version: 0.5.0\nfiles:\n  - url: Batlay_0.5.0_x64-setup.exe\n    sha512: SHA\n    size: 1234\npath: Batlay_0.5.0_x64-setup.exe\nsha512: SHA\nreleaseDate: '2026-10-28T10:00:00.000Z'\n";

    #[test]
    fn parses_electron_builder_latest_yml() {
        let r = parse_latest_yml(YML).unwrap();
        assert_eq!((r.version.as_str(), r.path.as_str(), r.sha512.as_str(), r.size), ("0.5.0", "Batlay_0.5.0_x64-setup.exe", "SHA", Some(1234)));
        assert!(parse_latest_yml("version: 1.0.0\npath: ../evil.exe\nsha512: x\n").is_none());
        assert!(parse_latest_yml("version: 1.0.0\npath: https://evil.example/x.exe\nsha512: x\n").is_none());
        assert!(parse_latest_yml("nope").is_none());
    }

    #[test]
    fn versions() {
        assert!(is_newer("0.5.0", "0.4.1"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(!is_newer("0.4.1", "0.4.1"));
        assert!(!is_newer("0.4.0", "0.4.1"));
        assert!(is_newer("1.0.0", "1.0.0-beta.1"));
        assert!(!is_newer("1.0.0-beta.2", "1.0.0"));
    }

    struct Files(Vec<u8>);
    impl Handler for Files {
        fn handle(&self, req: &Request) -> Response {
            match req.path() {
                "/redir" => Response::new(302).header("Location", "/setup.exe"),
                "/setup.exe" => Response::with_body(200, "application/octet-stream", self.0.clone()),
                _ => Response::new(404),
            }
        }
    }

    #[test]
    fn streaming_download_verifies_sha512() {
        let data: Vec<u8> = (0..300_000u32).map(|i| (i % 251) as u8).collect();
        let server = HttpServer::start(0, Arc::new(Files(data.clone()))).unwrap();
        let dir = std::env::temp_dir().join(format!("batlay-upd-{}", crate::util::random::random_hex(4)));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("setup.exe");
        let good = base64_encode(&sha512(&data));
        let mut seen = Vec::new();
        let url = format!("http://127.0.0.1:{}/setup.exe", server.port());
        download(&PlainConnector, &url, &dest, &good, |p| seen.push(p)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        assert_eq!(seen.last(), Some(&100));
        let bad = base64_encode(&sha512(b"autre chose"));
        let err = download(&PlainConnector, &url, &dir.join("x.exe"), &bad, |_| {}).unwrap_err();
        assert!(err.contains("ne correspond pas"));
        assert!(!dir.join("x.exe").exists() && !dir.join("x.part").exists());
        assert!(download(&PlainConnector, &format!("http://127.0.0.1:{}/redir", server.port()), &dir.join("y.exe"), &good, |_| {}).unwrap_err().contains("http refusée"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn dev_build_is_inert() {
        let u = Updater::new("0.5.0", false, Arc::new(PlainConnector), std::env::temp_dir(), Arc::new(|_| {}));
        assert_eq!(u.check(false), Status::Unavailable("dev"));
        assert_eq!(u.status().to_json().to_json(), r#"{"state":"unavailable","reason":"dev"}"#);
    }
}
