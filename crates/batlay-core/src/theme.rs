//! Thème de l'interface : réglage (sombre, clair, système) et fond natif des fenêtres.

/// Fond des fenêtres avant le premier rendu (= `base-950` de chaque thème) : aucun flash.
pub fn window_background(dark: bool) -> (u8, u8, u8) {
    if dark {
        (0x07, 0x07, 0x0C)
    } else {
        (0xF3, 0xF3, 0xF8)
    }
}

/// Thème effectif : « system » suit Windows.
pub fn resolve(preference: &str, system_dark: bool) -> &'static str {
    match preference {
        "light" => "light",
        "system" => {
            if system_dark {
                "dark"
            } else {
                "light"
            }
        }
        _ => "dark",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn resolves() {
        assert_eq!(super::resolve("system", false), "light");
        assert_eq!(super::resolve("system", true), "dark");
        assert_eq!(super::resolve("n'importe", false), "dark");
        assert_eq!(super::window_background(true), (7, 7, 12));
        assert_eq!(super::window_background(false), (243, 243, 248));
    }
}
