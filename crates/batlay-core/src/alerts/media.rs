//! Images et sons importés pour les alertes et les overlays personnalisés.
//!
//! Un fichier choisi par l'utilisateur est une donnée NON fiable :
//!   - le type est reconnu par ses premiers octets, jamais par son nom ;
//!   - taille plafonnée (8 Mo) ;
//!   - stocké sous un nom calculé (empreinte du contenu + extension déduite) :
//!     aucun chemin fourni par l'utilisateur n'est jamais utilisé.

use super::model::is_media_id;
use crate::json::Value;
use crate::obj;
use crate::util::encoding::hex_encode;
use crate::util::hash::sha256;
use std::path::PathBuf;

pub const MAX_MEDIA_BYTES: usize = 8 * 1024 * 1024;

pub fn mime_of(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        _ => "application/octet-stream",
    }
}

/// Type réel d'après le contenu, ou `None` (non pris en charge).
pub fn detect_media_type(b: &[u8]) -> Option<&'static str> {
    if b.len() < 12 {
        return None;
    }
    if b.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("png");
    }
    if b.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("jpg");
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return Some("gif");
    }
    if b.starts_with(b"RIFF") && &b[8..12] == b"WEBP" {
        return Some("webp");
    }
    if b.starts_with(b"RIFF") && &b[8..12] == b"WAVE" {
        return Some("wav");
    }
    if b.starts_with(b"OggS") {
        return Some("ogg");
    }
    if b.starts_with(b"ID3") {
        return Some("mp3");
    }
    // Trame MPEG sans balise ID3 : synchronisation sur 11 bits, couche III.
    if b[0] == 0xff && (b[1] & 0xe0) == 0xe0 && (b[1] & 0x06) == 0x02 {
        return Some("mp3");
    }
    None
}

fn is_sound_ext(ext: &str) -> bool {
    matches!(ext, "mp3" | "ogg" | "wav")
}

pub struct MediaStore {
    dir: PathBuf,
}

impl MediaStore {
    pub fn new(dir: impl Into<PathBuf>) -> MediaStore {
        MediaStore { dir: dir.into() }
    }

    /// `{ ok: true, id, kind }` ou `{ ok: false, reason }`.
    pub fn save(&self, data: &[u8], expected: &str) -> Value {
        let fail = |reason: &str| obj! { "ok" => false, "reason" => reason };
        if data.is_empty() {
            return fail("Fichier vide.");
        }
        if data.len() > MAX_MEDIA_BYTES {
            return fail("Fichier trop lourd (8 Mo maximum).");
        }
        let Some(ext) = detect_media_type(data) else {
            return fail("Format non reconnu. Images : PNG, JPG, GIF, WebP. Sons : MP3, OGG, WAV.");
        };
        let kind = if is_sound_ext(ext) { "sound" } else { "image" };
        if kind != expected {
            return fail(if expected == "image" { "Ce fichier n'est pas une image." } else { "Ce fichier n'est pas un son." });
        }
        let id = format!("{}.{ext}", &hex_encode(&sha256(data))[..24]);
        let file = self.dir.join(&id);
        if !file.exists() {
            if let Err(e) = std::fs::create_dir_all(&self.dir).and_then(|_| crate::config::write_atomic(&file, data)) {
                crate::log::warn(&format!("[Médias] enregistrement impossible : {e}"));
                return fail("Enregistrement impossible sur le disque.");
            }
        }
        obj! { "ok" => true, "id" => id, "kind" => kind }
    }

    /// Fichiers déjà importés, du plus récent au plus ancien.
    pub fn list(&self, kind: &str, max: usize) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else { return Vec::new() };
        let mut files: Vec<(std::time::SystemTime, String)> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let ext = name.rsplit('.').next().unwrap_or("").to_string();
                if !is_media_id(&name) || (kind == "sound") != is_sound_ext(&ext) {
                    return None;
                }
                let modified = e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH);
                Some((modified, name))
            })
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        files.into_iter().take(max).map(|(_, n)| n).collect()
    }

    /// Contenu + type MIME, ou `None` (identifiant invalide ou inconnu).
    pub fn read(&self, id: &str) -> Option<(Vec<u8>, &'static str)> {
        if !is_media_id(id) {
            return None;
        }
        let data = std::fs::read(self.dir.join(id)).ok()?;
        Some((data, mime_of(id.rsplit('.').next().unwrap_or(""))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png() -> Vec<u8> {
        let mut v = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        v.extend_from_slice(&[0u8; 20]);
        v
    }

    #[test]
    fn detects_by_content_and_stores_by_hash() {
        let dir = std::env::temp_dir().join(format!("batlay-media-{}", crate::util::random::random_hex(4)));
        let store = MediaStore::new(&dir);
        let r = store.save(&png(), "image");
        assert_eq!(r.get("ok").as_bool(), Some(true));
        let id = r.get("id").as_str().unwrap().to_string();
        assert!(id.ends_with(".png") && is_media_id(&id));
        assert_eq!(store.save(&png(), "sound").get("reason").as_str(), Some("Ce fichier n'est pas un son."));
        assert_eq!(store.save(b"<svg onload=alert(1)>....", "image").get("ok").as_bool(), Some(false));
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend_from_slice(&[0; 8]);
        let wav_id = store.save(&wav, "sound").get("id").as_str().unwrap().to_string();
        assert_eq!(store.list("image", 60), vec![id.clone()]);
        assert_eq!(store.list("sound", 60), vec![wav_id]);
        assert_eq!(store.read(&id).unwrap().1, "image/png");
        assert!(store.read("../batlay-config.json").is_none());
        assert_eq!(detect_media_type(&[0xff, 0xfb, 0x90, 0x44, 0, 0, 0, 0, 0, 0, 0, 0]), Some("mp3"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}
