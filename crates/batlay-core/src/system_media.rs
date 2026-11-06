//! Lecture système (Windows) : ce que joue le PC, tous lecteurs confondus,
//! via les Global System Media Transport Controls (SMTC), sans compte ni clé.
//!
//! Même script PowerShell qu'avant la 0.5.0 (`system_media.ps1`), passé par
//! `-EncodedCommand` (UTF-16LE + Base64 : aucun problème de guillemets), dont
//! la sortie JSON est encodée en Base64 par PowerShell lui-même : aucun « � »
//! possible sur les accents, quelle que soit la page de code de la console.

use crate::json::{self, Value};
use crate::obj;
use crate::util::encoding::{base64_decode, base64_encode, utf16le};
use std::time::Duration;

const SCRIPT: &str = include_str!("system_media.ps1");
/// Au-delà, LastUpdatedTime est jugé aberrant.
const MAX_EXTRAPOLATION_MS: f64 = 12.0 * 60.0 * 60.0 * 1000.0;

/// Lance PowerShell et renvoie sa sortie brute.
pub trait PowerShell: Send + Sync {
    fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, String>;
}

/// `powershell.exe` du système, sans fenêtre, tué au bout du délai.
pub struct SystemPowerShell;

impl PowerShell for SystemPowerShell {
    fn run(&self, args: &[String], timeout: Duration) -> Result<Vec<u8>, String> {
        run_process("powershell.exe", args, timeout).map(|(_, out)| out)
    }
}

/// Lance un programme sans fenêtre ; renvoie (code de sortie, sortie standard). Tué au bout du délai.
pub fn run_process(program: &str, args: &[String], timeout: Duration) -> Result<(i32, Vec<u8>), String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("impossible de lancer {program} : {e}"))?;
    let mut stdout = child.stdout.take().ok_or("sortie indisponible")?;
    // Lecture sur un fil à part : une sortie volumineuse (pochette) ne bloque jamais le processus.
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.by_ref().take(16 * 1024 * 1024).read_to_end(&mut buf);
        buf
    });
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("{program} ne répond pas (délai de {} s dépassé)", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = reader.join().unwrap_or_default();
    Ok((status.code().unwrap_or(1), out))
}

/// Arguments de PowerShell pour un script : `-EncodedCommand` (aucun échappement possible).
pub fn encoded_args(script: &str) -> Vec<String> {
    vec!["-NoProfile".into(), "-NonInteractive".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-EncodedCommand".into(), base64_encode(&utf16le(script))]
}

/// Script complet : paramètres injectés dans `$__params` (JSON entre apostrophes doublées).
pub fn build_script(preferred_app_id: &str, include_artwork: bool, list_only: bool) -> String {
    let params = obj! { "preferredAppId" => preferred_app_id, "includeArtwork" => include_artwork, "listOnly" => list_only }.to_json().replace('\'', "''");
    format!("$__params = '{params}' | ConvertFrom-Json\n{SCRIPT}")
}

/// Sortie PowerShell → texte JSON. Cas nominal : Base64 → UTF-8. Repli : UTF-8
/// strict, sinon Windows-1252 (jamais de « � » créé par un mauvais décodage).
pub fn decode_output(output: &[u8]) -> String {
    let buf = output.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(output);
    let compact: String = buf.iter().filter(|b| !b.is_ascii_whitespace()).map(|&b| b as char).collect();
    if !compact.is_empty() && compact.len() % 4 == 0 && compact.chars().all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=') {
        if let Some(bytes) = base64_decode(&compact) {
            let text = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').trim().to_string();
            if text.starts_with('{') || text.starts_with('[') {
                return text;
            }
        }
    }
    match std::str::from_utf8(buf) {
        Ok(t) => t.trim_start_matches('\u{feff}').trim().to_string(),
        Err(_) => windows_1252(buf).trim().to_string(),
    }
}

fn windows_1252(bytes: &[u8]) -> String {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    bytes.iter().map(|&b| if (0x80..0xA0).contains(&b) { HIGH[(b - 0x80) as usize] } else { b as char }).collect()
}

/// Position à l'instant `sampled_at` : Windows donne la position au moment de
/// `last_updated` ; en lecture on y ajoute le temps écoulé (comme le volet de Windows).
pub fn resolve_position(position_ms: f64, last_updated_ms: Option<f64>, sampled_at_ms: Option<f64>, duration_ms: f64, is_playing: bool) -> f64 {
    let mut position = if position_ms.is_finite() { position_ms.max(0.0) } else { 0.0 };
    if let (true, Some(lu), Some(sa)) = (is_playing && duration_ms > 0.0, last_updated_ms, sampled_at_ms) {
        let age = sa - lu;
        if age >= 0.0 && age <= MAX_EXTRAPOLATION_MS.min(duration_ms) {
            position += age;
        }
    }
    if duration_ms > 0.0 {
        position.min(duration_ms)
    } else {
        position
    }
}

pub fn is_supported() -> bool {
    cfg!(windows)
}

pub struct SystemMedia {
    shell: Box<dyn PowerShell>,
}

impl SystemMedia {
    pub fn new(shell: Box<dyn PowerShell>) -> SystemMedia {
        SystemMedia { shell }
    }

    fn run(&self, script: &str) -> Result<Value, String> {
        if !is_supported() && std::env::var_os("BATLAY_FAKE_SMTC").is_none() {
            return Err("La lecture système n'est disponible que sur Windows (Global System Media Transport Controls).".into());
        }
        let out = self.shell.run(&encoded_args(script), Duration::from_secs(5)).map_err(|e| format!("Impossible d'interroger les contrôles multimédias de Windows : {e}"))?;
        json::parse(&decode_output(&out)).map_err(|_| "Réponse inattendue de PowerShell lors de la lecture des contrôles multimédias.".to_string())
    }

    /// Lecteurs actifs (une session par application).
    pub fn list_sessions(&self) -> Result<Value, String> {
        let parsed = self.run(&build_script("", false, true))?;
        if parsed.get("supported").as_bool() != Some(true) {
            return Err(parsed.get("error").as_str().unwrap_or("Impossible de lister les lecteurs actifs sur ce système.").to_string());
        }
        // ConvertTo-Json écrit un tableau d'un seul élément comme un objet.
        let sessions = match parsed.get("sessions") {
            Value::Array(items) => items.clone(),
            v if v.is_object() => vec![v.clone()],
            _ => Vec::new(),
        };
        Ok(Value::Array(
            sessions
                .iter()
                .map(|s| obj! { "sourceAppId" => s.get("sourceAppId").non_empty_str(), "title" => s.get("title").as_str().unwrap_or(""), "artist" => s.get("artist").as_str().unwrap_or("") })
                .collect(),
        ))
    }

    /// État courant (`null` si rien ne joue : ce n'est pas une erreur).
    pub fn current(&self, preferred_app_id: Option<&str>, include_artwork: bool, now: u64) -> Result<Value, String> {
        let preferred = preferred_app_id.filter(|s| s.len() <= 300 && !s.chars().any(char::is_control)).unwrap_or("");
        let parsed = self.run(&build_script(preferred, include_artwork, false))?;
        if parsed.get("supported").as_bool() != Some(true) {
            return Err(parsed.get("error").as_str().unwrap_or("Les Global System Media Transport Controls ne sont pas disponibles sur ce système.").to_string());
        }
        let s = parsed.get("session");
        if !s.is_object() {
            return Ok(Value::Null);
        }
        // Durée aberrante (TimeSpan.MaxValue pour un direct) ramenée à 0.
        let duration = s.get("durationMs").as_f64().filter(|d| *d < 24.0 * 3600.0 * 1000.0).unwrap_or(0.0);
        let is_playing = s.get("isPlaying").as_bool().unwrap_or(false);
        let sampled = s.get("sampledAtMs").as_f64().unwrap_or(now as f64);
        let text = |k: &str| s.get(k).non_empty_str().map(str::to_string);
        Ok(obj! {
            "title" => s.get("title").as_str().unwrap_or(""),
            "artist" => s.get("artist").as_str().unwrap_or(""),
            "albumArtist" => text("albumArtist"),
            "album" => text("album"),
            "windowTitle" => text("windowTitle"),
            "sourceAppId" => text("sourceAppId"),
            "isPlaying" => is_playing,
            "playbackStatus" => s.get("playbackStatus").as_str().unwrap_or(if is_playing { "playing" } else { "paused" }),
            "positionMs" => resolve_position(s.get("positionMs").as_f64().unwrap_or(0.0), s.get("lastUpdatedMs").as_f64(), Some(sampled), duration, is_playing),
            "sampledAtMs" => sampled,
            "durationMs" => duration,
            "artwork" => text("artwork"),
            "hasThumbnail" => s.get("hasThumbnail").as_bool().unwrap_or(false),
            "artworkError" => text("artworkError"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Fake(Mutex<Vec<Vec<String>>>, Vec<u8>);
    impl PowerShell for Fake {
        fn run(&self, args: &[String], _t: Duration) -> Result<Vec<u8>, String> {
            self.0.lock().unwrap().push(args.to_vec());
            Ok(self.1.clone())
        }
    }

    #[test]
    fn decodes_base64_utf8_and_fallbacks() {
        let json = r#"{"supported":true,"session":{"title":"Été indien","artist":"Joe Dassin"}}"#;
        let b64 = format!("{}\r\n", base64_encode(json.as_bytes()));
        assert_eq!(decode_output(b64.as_bytes()), json);
        assert_eq!(decode_output("\u{feff}{\"a\":\"é\"}".as_bytes()), "{\"a\":\"é\"}");
        assert_eq!(decode_output(&[b'"', 0xE9, b't', 0xE9, b'"']), "\"été\"", "repli Windows-1252");
        assert_eq!(decode_output(b"null"), "null", "un mot Base64 valide n'est pas notre JSON");
    }

    #[test]
    fn script_and_arguments() {
        let script = build_script("Spotify'.exe", true, false);
        assert!(script.starts_with("$__params = '{\"preferredAppId\":\"Spotify''.exe\",\"includeArtwork\":true,\"listOnly\":false}' | ConvertFrom-Json\n"));
        assert!(script.contains("IAsyncOperation`1"));
        let args = encoded_args("Write-Output 'é'");
        assert_eq!(args[4], "-EncodedCommand");
        assert_eq!(crate::util::encoding::base64_decode(&args[5]).unwrap(), utf16le("Write-Output 'é'"));
    }

    #[test]
    fn position_extrapolation() {
        assert_eq!(resolve_position(10_000.0, Some(1_000.0), Some(4_000.0), 200_000.0, true), 13_000.0);
        assert_eq!(resolve_position(10_000.0, Some(1_000.0), Some(4_000.0), 200_000.0, false), 10_000.0);
        assert_eq!(resolve_position(10_000.0, Some(5_000.0), Some(4_000.0), 200_000.0, true), 10_000.0, "écart négatif ignoré");
        assert_eq!(resolve_position(199_000.0, Some(0.0), Some(5_000.0), 200_000.0, true), 200_000.0, "jamais au-delà de la durée");
    }

    #[test]
    fn current_session_via_fake_powershell() {
        std::env::set_var("BATLAY_FAKE_SMTC", "1");
        let out = base64_encode(br#"{"supported":true,"preferredMissing":false,"session":{"title":"Starboy","artist":"The Weeknd","album":"","sourceAppId":"Spotify.exe","isPlaying":true,"playbackStatus":"playing","positionMs":1000,"lastUpdatedMs":1000,"sampledAtMs":3000,"durationMs":9.2233720368547758E+14,"artwork":null,"hasThumbnail":false}}"#);
        let fake = Fake(Mutex::new(Vec::new()), out.into_bytes());
        let media = SystemMedia::new(Box::new(fake));
        let v = media.current(Some("Spotify.exe"), false, 0).unwrap();
        assert_eq!((v.get("title").as_str(), v.get("durationMs").as_f64(), v.get("positionMs").as_f64()), (Some("Starboy"), Some(0.0), Some(1000.0)));
        assert!(v.get("album").is_null());
        let list = SystemMedia::new(Box::new(Fake(Mutex::new(Vec::new()), base64_encode(br#"{"supported":true,"sessions":{"sourceAppId":"VLC","title":"x","artist":"y"}}"#).into_bytes())));
        assert_eq!(list.list_sessions().unwrap().items().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn process_runner_and_timeout() {
        let (code, out) = run_process("sh", &["-c".into(), "printf bonjour; exit 3".into()], Duration::from_secs(5)).unwrap();
        assert_eq!((code, out.as_slice()), (3, &b"bonjour"[..]));
        let err = run_process("sh", &["-c".into(), "sleep 5".into()], Duration::from_millis(200)).unwrap_err();
        assert!(err.contains("délai"));
    }
}
