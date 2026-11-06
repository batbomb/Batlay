//! Nettoyage du texte affiché à l'antenne ou dans le tableau de bord.

/// Caractère de contrôle ou invisible (U+0000–001F, U+007F–009F, U+200B–200F,
/// U+2028–202E, U+2060–206F, U+FEFF) : jamais affiché tel quel.
pub fn is_hidden_char(c: char) -> bool {
    let u = c as u32;
    u <= 0x1f || (0x7f..=0x9f).contains(&u) || (0x200b..=0x200f).contains(&u) || (0x2028..=0x202e).contains(&u) || (0x2060..=0x206f).contains(&u) || u == 0xfeff
}

/// Invisibles → espace, blancs réduits à un espace, bords retirés, `max` caractères au plus.
pub fn clean_text(text: &str, max: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max * 4));
    let mut pending_space = false;
    let mut count = 0;
    for c in text.chars() {
        let c = if is_hidden_char(c) { ' ' } else { c };
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            if count >= max {
                break;
            }
            out.push(' ');
            count += 1;
            pending_space = false;
        }
        if count >= max {
            break;
        }
        out.push(c);
        count += 1;
    }
    out
}

/// Remplace seulement les caractères de contrôle C0 et DEL par un espace (sans réduire les blancs).
pub fn replace_controls(text: &str) -> String {
    text.chars().map(|c| if (c as u32) < 0x20 || c as u32 == 0x7f { ' ' } else { c }).collect()
}

/// Les `max` premiers caractères.
pub fn truncate_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// `#rrggbb` (6 chiffres hexadécimaux).
pub fn is_hex_color(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleans_like_the_typescript_version() {
        assert_eq!(clean_text("  Ro\u{200b}se\n  rouge\u{0}! ", 40), "Ro se rouge !");
        assert_eq!(clean_text("abcdef", 3), "abc");
        assert_eq!(clean_text("ab  cd", 3), "ab ");
        assert_eq!(clean_text("\u{feff}\u{2028}", 10), "");
        assert_eq!(replace_controls("a\u{7}b\nc"), "a b c");
        assert!(is_hex_color("#9B6BFF") && !is_hex_color("#9B6BF") && !is_hex_color("9B6BFFF"));
    }
}
