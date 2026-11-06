//! Les quelques fonctions de Windows dont Batlay a besoin, déclarées à la main
//! (signatures stables depuis Windows Vista, documentées par Microsoft) :
//! chiffrement DPAPI des jetons, ouverture d'une adresse ou d'un fichier,
//! clés du registre (thème du système, démarrage avec Windows).

use batlay_core::config::SecretBox;
use std::ffi::c_void;
use std::path::Path;
use std::ptr::{null, null_mut};

type Hkey = isize;
/// `HKEY_CURRENT_USER` : la valeur 0x80000001, étendue avec son signe comme le fait windows.h.
const HKEY_CURRENT_USER: Hkey = 0x8000_0001_u32 as i32 as isize;
const RRF_RT_REG_DWORD: u32 = 0x0000_0010;
const RRF_RT_REG_SZ: u32 = 0x0000_0002;
const REG_SZ: u32 = 1;
const ERROR_SUCCESS: i32 = 0;
const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;
const SW_SHOWNORMAL: i32 = 1;
const COINIT_APARTMENTTHREADED: u32 = 0x2;
const COINIT_DISABLE_OLE1DDE: u32 = 0x4;

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

#[link(name = "advapi32")]
extern "system" {
    fn RegGetValueW(hkey: Hkey, sub_key: *const u16, value: *const u16, flags: u32, kind: *mut u32, data: *mut c_void, data_len: *mut u32) -> i32;
    fn RegSetKeyValueW(hkey: Hkey, sub_key: *const u16, value: *const u16, kind: u32, data: *const c_void, data_len: u32) -> i32;
    fn RegDeleteKeyValueW(hkey: Hkey, sub_key: *const u16, value: *const u16) -> i32;
}

#[link(name = "crypt32")]
extern "system" {
    fn CryptProtectData(data_in: *const DataBlob, description: *const u16, entropy: *const DataBlob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, data_out: *mut DataBlob) -> i32;
    fn CryptUnprotectData(data_in: *const DataBlob, description: *mut *mut u16, entropy: *const DataBlob, reserved: *mut c_void, prompt: *mut c_void, flags: u32, data_out: *mut DataBlob) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn LocalFree(mem: *mut c_void) -> *mut c_void;
}

#[link(name = "shell32")]
extern "system" {
    fn ShellExecuteW(hwnd: *mut c_void, operation: *const u16, file: *const u16, parameters: *const u16, directory: *const u16, show: i32) -> *mut c_void;
}

#[link(name = "ole32")]
extern "system" {
    fn CoInitializeEx(reserved: *const c_void, co_init: u32) -> i32;
    fn CoUninitialize();
}

/// Chaîne terminée par un zéro, en UTF-16 (ce qu'attendent les fonctions « W »).
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

// -- DPAPI -------------------------------------------------------------------------------

/// Complément de clé propre à Batlay : un autre programme qui appellerait DPAPI
/// sans le connaître ne déchiffrerait pas les jetons (même session Windows).
const ENTROPY: &[u8] = b"Batlay/0.5 jetons";

/// Jetons Twitch et Spotify chiffrés par Windows pour la session de l'utilisateur.
pub struct Dpapi;

fn blob(data: &[u8]) -> DataBlob {
    DataBlob { cb_data: data.len() as u32, pb_data: data.as_ptr() as *mut u8 }
}

/// Copie le résultat puis le libère (mémoire allouée par Windows avec LocalAlloc).
unsafe fn take_blob(out: DataBlob) -> Vec<u8> {
    if out.pb_data.is_null() {
        return Vec::new();
    }
    let bytes = std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec();
    LocalFree(out.pb_data as *mut c_void);
    bytes
}

impl SecretBox for Dpapi {
    fn protect(&self, data: &[u8]) -> Option<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = DataBlob { cb_data: 0, pb_data: null_mut() };
        let description = wide("Batlay");
        // SAFETY : toutes les structures vivent pendant l'appel ; la sortie est libérée par take_blob.
        let ok = unsafe { CryptProtectData(&input, description.as_ptr(), &entropy, null_mut(), null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
        (ok != 0).then(|| unsafe { take_blob(out) })
    }

    fn unprotect(&self, data: &[u8]) -> Option<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(ENTROPY);
        let mut out = DataBlob { cb_data: 0, pb_data: null_mut() };
        // SAFETY : idem ; la description n'est pas demandée (pointeur nul).
        let ok = unsafe { CryptUnprotectData(&input, null_mut(), &entropy, null_mut(), null_mut(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) };
        (ok != 0).then(|| unsafe { take_blob(out) })
    }
}

// -- Ouverture d'une adresse / d'un fichier ------------------------------------------------

/// Ouvre `target` avec le programme associé par Windows (navigateur par défaut pour
/// une adresse https, installateur pour un .exe). Le cœur a déjà vérifié la cible.
pub fn shell_open(target: &str, parameters: Option<&str>) -> Result<(), String> {
    let target = target.to_string();
    let parameters = parameters.map(str::to_string);
    // Fil à part : ShellExecute veut COM initialisé, et peut prendre un moment.
    std::thread::spawn(move || {
        // SAFETY : appels Windows documentés ; les chaînes vivent jusqu'à la fin du fil.
        unsafe {
            let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            let operation = wide("open");
            let file = wide(&target);
            let params = parameters.as_deref().map(wide);
            let result = ShellExecuteW(null_mut(), operation.as_ptr(), file.as_ptr(), params.as_ref().map_or(null(), |p| p.as_ptr()), null(), SW_SHOWNORMAL);
            if com >= 0 {
                CoUninitialize();
            }
            // Valeur > 32 : réussite (convention historique de ShellExecute).
            if result as usize > 32 {
                Ok(())
            } else {
                Err(format!("Windows n'a pas pu ouvrir l'adresse (code {})", result as usize))
            }
        }
    })
    .join()
    .unwrap_or_else(|_| Err("ouverture impossible".into()))
}

// -- Registre ------------------------------------------------------------------------------

const PERSONALIZE: &str = r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// Nom de l'entrée « démarrer avec Windows » de Batlay 0.5.
const RUN_VALUE: &str = "Batlay";
/// Entrées écrites par l'ancienne version (Electron) : elles visent un programme désinstallé.
const LEGACY_RUN_VALUES: [&str; 2] = ["com.adil.batlay", "electron.app.Batlay"];

fn read_dword(sub_key: &str, value: &str) -> Option<u32> {
    let (sub_key, value) = (wide(sub_key), wide(value));
    let mut data: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as u32;
    // SAFETY : tampon de 4 octets, taille passée.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, sub_key.as_ptr(), value.as_ptr(), RRF_RT_REG_DWORD, null_mut(), &mut data as *mut u32 as *mut c_void, &mut len) };
    (status == ERROR_SUCCESS).then_some(data)
}

fn read_string(sub_key: &str, value: &str) -> Option<String> {
    let (sub_key, value) = (wide(sub_key), wide(value));
    let mut buffer = vec![0u16; 2048];
    let mut len = (buffer.len() * 2) as u32;
    // SAFETY : tampon de 2048 caractères, taille en octets passée.
    let status = unsafe { RegGetValueW(HKEY_CURRENT_USER, sub_key.as_ptr(), value.as_ptr(), RRF_RT_REG_SZ, null_mut(), buffer.as_mut_ptr() as *mut c_void, &mut len) };
    if status != ERROR_SUCCESS {
        return None;
    }
    let chars = (len as usize / 2).min(buffer.len());
    let text = String::from_utf16_lossy(&buffer[..chars]);
    Some(text.trim_end_matches('\0').to_string())
}

/// Windows est-il en mode sombre (applications) ? Clair si la valeur est illisible.
pub fn system_prefers_dark() -> bool {
    read_dword(PERSONALIZE, "AppsUseLightTheme") == Some(0)
}

/// Démarrage avec Windows : valeur « Batlay » de HKCU\…\Run, qui vise ce programme.
/// Retire au passage les entrées laissées par l'ancienne version.
pub fn set_run_at_login(enabled: bool, exe: &Path) {
    let run = wide(RUN);
    for legacy in LEGACY_RUN_VALUES {
        let name = wide(legacy);
        // SAFETY : chaînes terminées par zéro.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, run.as_ptr(), name.as_ptr()) };
    }
    let name = wide(RUN_VALUE);
    if enabled {
        let command = format!("\"{}\"", exe.display());
        if read_string(RUN, RUN_VALUE).as_deref() == Some(command.as_str()) {
            return;
        }
        let data = wide(&command);
        // SAFETY : REG_SZ avec son zéro final, taille en octets.
        let status = unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, run.as_ptr(), name.as_ptr(), REG_SZ, data.as_ptr() as *const c_void, (data.len() * 2) as u32) };
        if status != ERROR_SUCCESS {
            batlay_core::log::warn(&format!("Démarrage avec Windows : écriture refusée (code {status})"));
        }
    } else {
        // SAFETY : chaînes terminées par zéro.
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, run.as_ptr(), name.as_ptr()) };
    }
}
