//! Journal : console + fichier `%APPDATA%\Batlay\logs\batlay.log` (1 Mo, puis
//! `batlay.old.log`). Jamais de jeton ni de mot de passe dans ces lignes.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

static FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
const MAX_BYTES: u64 = 1024 * 1024;

pub fn init(dir: PathBuf) {
    let _ = std::fs::create_dir_all(&dir);
    *FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.join("batlay.log"));
}

fn timestamp() -> String {
    let ms = crate::util::now_ms();
    let secs = ms / 1000;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    format!("{} {h:02}:{m:02}:{s:02}.{:03} UTC", civil_date(secs / 86400), ms % 1000)
}

/// Jours depuis 1970 → « AAAA-MM-JJ » (algorithme de Howard Hinnant).
pub fn civil_date(days: u64) -> String {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{:04}-{:02}-{:02}", if m <= 2 { y + 1 } else { y }, m, d)
}

/// Instant (ms) → « 2026-10-28T04:56:57.471Z » (comme `toISOString`).
pub fn iso_time(ms: u64) -> String {
    let secs = ms / 1000;
    format!("{}T{:02}:{:02}:{:02}.{:03}Z", civil_date(secs / 86400), (secs / 3600) % 24, (secs / 60) % 60, secs % 60, ms % 1000)
}

fn write(level: &str, message: &str) {
    let line = format!("{} [{level}] {message}", timestamp());
    eprintln!("[Batlay] {message}");
    let guard = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(path) = guard.as_ref() {
        if std::fs::metadata(path).map(|m| m.len() > MAX_BYTES).unwrap_or(false) {
            let _ = std::fs::rename(path, path.with_file_name("batlay.old.log"));
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{line}");
        }
    }
}

pub fn info(message: &str) {
    write("info", message)
}
pub fn warn(message: &str) {
    write("warn", message)
}
pub fn error(message: &str) {
    write("erreur", message)
}

#[cfg(test)]
mod tests {
    #[test]
    fn dates() {
        assert_eq!(super::civil_date(0), "1970-01-01");
        assert_eq!(super::iso_time(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(super::iso_time(1_793_163_417_471), "2026-10-28T04:56:57.471Z");
        assert_eq!(super::civil_date(11_016), "2000-02-29");
    }
}
