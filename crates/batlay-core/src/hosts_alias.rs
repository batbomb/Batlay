//! Nom local `batlay.test` (fichier hosts de Windows) : vérification, ajout et
//! retrait, à la demande de l'utilisateur (TikTok LIVE Studio refuse les URL
//! en localhost / 127.0.0.1).
//!
//! Sécurité : les deux scripts PowerShell sont des CONSTANTES (aucune donnée du
//! tableau de bord ni du réseau n'y entre), ils ne touchent qu'à la ligne
//! `127.0.0.1 batlay.test`, et l'écriture passe par l'élévation standard de
//! Windows (UAC) : sans accord de l'utilisateur, rien n'est modifié.

use crate::json::Value;
use crate::loopback::LOCAL_ALIAS_HOSTNAME;
use crate::obj;
use crate::util::encoding::{base64_encode, utf16le};
use std::net::{IpAddr, ToSocketAddrs};
use std::time::Duration;

/// Ligne écrite (ASCII uniquement : le fichier hosts est écrit en ASCII).
pub const HOSTS_ENTRY: &str = "127.0.0.1 batlay.test # Batlay (overlays TikTok LIVE Studio)";
const ENTRY_PATTERN: &str = r"^\s*127\.0\.0\.1\s+batlay\.test(\s|$)";

const PRELUDE: &str = "\n$ErrorActionPreference = 'Stop'\n$path = Join-Path $env:SystemRoot 'System32\\drivers\\etc\\hosts'\n";
const FLUSH: &str = "\n# Vidage du cache DNS : facultatif, ne doit jamais faire échouer l'opération.\ntry { Clear-DnsClientCache } catch { try { ipconfig /flushdns | Out-Null } catch { } }\n";

pub fn add_script() -> String {
    format!(
        "{PRELUDE}try {{\n  $content = if (Test-Path -LiteralPath $path) {{ [IO.File]::ReadAllText($path) }} else {{ '' }}\n  if ($content -notmatch '(?m){ENTRY_PATTERN}') {{\n    $prefix = if ($content.Length -gt 0 -and -not $content.EndsWith(\"`n\")) {{ \"`r`n\" }} else {{ '' }}\n    [IO.File]::AppendAllText($path, $prefix + '{HOSTS_ENTRY}' + \"`r`n\", [Text.Encoding]::ASCII)\n  }}\n{FLUSH}  exit 0\n}} catch {{ exit 2 }}\n"
    )
}

pub fn remove_script() -> String {
    format!(
        "{PRELUDE}try {{\n  if (-not (Test-Path -LiteralPath $path)) {{ exit 0 }}\n  $lines = [IO.File]::ReadAllLines($path)\n  $kept = @($lines | Where-Object {{ $_ -notmatch '{ENTRY_PATTERN}' }})\n  if ($kept.Count -ne $lines.Count) {{ [IO.File]::WriteAllLines($path, [string[]]$kept, [Text.Encoding]::ASCII) }}\n{FLUSH}  exit 0\n}} catch {{ exit 2 }}\n"
    )
}

/// `powershell -EncodedCommand` : UTF-16LE puis Base64.
pub fn encode_command(script: &str) -> String {
    base64_encode(&utf16le(script))
}

/// Arguments de `powershell.exe` (non élevé) qui relance le script en administrateur
/// et renvoie son code de sortie. Le Base64 ne contient que `[A-Za-z0-9+/=]`.
pub fn elevated_args(script: &str) -> Vec<String> {
    let encoded = encode_command(script);
    let command = format!(
        "$p = Start-Process -FilePath powershell.exe -Verb RunAs -Wait -PassThru -WindowStyle Hidden -ArgumentList '-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{encoded}'; exit $p.ExitCode"
    );
    vec!["-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), command]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AliasStatus {
    Ok,
    Missing,
    WrongAddress(Vec<String>),
    Error(String),
}

impl AliasStatus {
    pub fn to_json(&self) -> Value {
        let base = obj! { "hostname" => LOCAL_ALIAS_HOSTNAME };
        match self {
            AliasStatus::Ok => base.with("state", "ok"),
            AliasStatus::Missing => base.with("state", "missing"),
            AliasStatus::WrongAddress(a) => base.with("state", "wrong-address").with("addresses", Value::from(a.clone())),
            AliasStatus::Error(m) => base.with("state", "error").with("message", m.clone()),
        }
    }
    pub fn is_ok(&self) -> bool {
        *self == AliasStatus::Ok
    }
}

/// `batlay.test` pointe-t-il vers CE PC ? (résolveur du système, fichier hosts compris ; 3 s au plus).
/// Les serveurs n'écoutent que sur 127.0.0.1 : un `::1` seul ne suffit pas.
pub fn check_local_alias() -> AliasStatus {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send((LOCAL_ALIAS_HOSTNAME, 80).to_socket_addrs().map(|a| a.map(|s| s.ip()).collect::<Vec<IpAddr>>()));
    });
    match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Ok(addresses)) => classify(&addresses),
        Ok(Err(_)) => AliasStatus::Missing,
        Err(_) => AliasStatus::Error("délai dépassé".into()),
    }
}

pub fn classify(addresses: &[IpAddr]) -> AliasStatus {
    if addresses.iter().any(|a| a.to_string() == "127.0.0.1") {
        AliasStatus::Ok
    } else if addresses.is_empty() {
        AliasStatus::Missing
    } else {
        AliasStatus::WrongAddress(addresses.iter().map(|a| a.to_string()).collect())
    }
}

/// Résultat d'un ajout / retrait.
pub fn outcome_json(result: &str, status: &AliasStatus, message: Option<&str>) -> Value {
    let mut v = obj! { "result" => result, "status" => status.to_json() };
    if let Some(m) = message {
        v.set("message", m);
    }
    v
}

/// Ajout (`add = true`) ou retrait de la ligne. `run` lance PowerShell et renvoie son code de sortie.
pub fn apply(add: bool, check: &dyn Fn() -> AliasStatus, run: &dyn Fn(Vec<String>) -> i32) -> Value {
    let before = check();
    if !cfg!(windows) && std::env::var_os("BATLAY_FAKE_HOSTS").is_none() {
        return outcome_json("unsupported", &before, None);
    }
    if add && before.is_ok() {
        return outcome_json("already", &before, None);
    }
    if !add && before == AliasStatus::Missing {
        return outcome_json("already", &before, None);
    }
    let code = run(elevated_args(&if add { add_script() } else { remove_script() }));
    let after = check();
    let success = if add { after.is_ok() } else { !after.is_ok() };
    if success {
        return outcome_json("done", &after, None);
    }
    // Start-Process échoue (code 1) quand l'utilisateur refuse l'élévation ; 2 = écriture refusée.
    if code == 1 {
        return outcome_json("cancelled", &after, None);
    }
    let message = if code == 2 {
        "Windows a refusé l'écriture du fichier hosts (antivirus ou fichier verrouillé). Ajoutez la ligne à la main."
    } else {
        "La modification n'a pas pris effet. Redémarrez Batlay, ou ajoutez la ligne à la main."
    };
    outcome_json("failed", &after, Some(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn scripts_are_constant_and_ascii_entry() {
        let add = add_script();
        assert!(add.contains("[IO.File]::AppendAllText($path, $prefix + '127.0.0.1 batlay.test # Batlay (overlays TikTok LIVE Studio)' + \"`r`n\", [Text.Encoding]::ASCII)"));
        assert!(add.contains("-notmatch '(?m)^\\s*127\\.0\\.0\\.1\\s+batlay\\.test(\\s|$)'"));
        assert!(remove_script().contains("Where-Object { $_ -notmatch '^\\s*127\\.0\\.0\\.1\\s+batlay\\.test(\\s|$)' }"));
        assert!(HOSTS_ENTRY.is_ascii());
        let args = elevated_args("exit 0");
        assert_eq!(args[2], "-Command");
        assert!(args[3].starts_with("$p = Start-Process -FilePath powershell.exe -Verb RunAs"));
        assert!(args[3].contains(&encode_command("exit 0")));
    }

    #[test]
    fn outcomes() {
        std::env::set_var("BATLAY_FAKE_HOSTS", "1");
        let state = RefCell::new(AliasStatus::Missing);
        let check = || state.borrow().clone();
        let r = apply(true, &check, &|_| {
            *state.borrow_mut() = AliasStatus::Ok;
            0
        });
        assert_eq!(r.get("result").as_str(), Some("done"));
        assert_eq!(apply(true, &check, &|_| 0).get("result").as_str(), Some("already"));
        let r = apply(false, &check, &|_| 1);
        assert_eq!(r.get("result").as_str(), Some("cancelled"));
        let r = apply(false, &check, &|_| 2);
        assert!(r.get("message").as_str().unwrap().contains("antivirus"));
        assert_eq!(classify(&["::1".parse().unwrap()]), AliasStatus::WrongAddress(vec!["::1".into()]));
        assert_eq!(AliasStatus::Ok.to_json().to_json(), r#"{"hostname":"batlay.test","state":"ok"}"#);
    }
}
