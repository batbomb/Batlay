//! Pages que Batlay affiche dans le navigateur (retour de connexion Spotify et
//! Twitch) : même identité que l'application 0.5 (logo, encre profonde, lueur
//! indigo et cyan). Tout est en ligne : rien n'est chargé d'Internet.

/// Logo de Batlay (tuile dégradée, chauve-souris, égaliseur en creux), en SVG autonome.
pub const LOGO_SVG: &str = r##"<svg class="logo" viewBox="0 0 512 512" aria-hidden="true"><defs><linearGradient id="bt" x1=".1" y1="0" x2=".9" y2="1"><stop offset="0" stop-color="#8A6BFF"/><stop offset=".52" stop-color="#5B47F5"/><stop offset="1" stop-color="#1E9BFF"/></linearGradient><mask id="bc"><rect width="512" height="512" fill="#fff"/><rect x="226" y="240" width="16" height="58" rx="8"/><rect x="248" y="222" width="16" height="76" rx="8"/><rect x="270" y="252" width="16" height="46" rx="8"/></mask></defs><rect x="16" y="16" width="480" height="480" rx="116" fill="url(#bt)"/><g fill="#fff" mask="url(#bc)" transform="translate(256 260) scale(1.12) translate(-256 -250)"><path d="M232 214 C 196 178 128 162 58 188 C 74 204 84 222 86 246 C 104 238 122 244 132 262 C 148 252 168 258 178 278 C 192 270 208 276 216 294 L 232 300 Z"/><path transform="translate(512,0) scale(-1,1)" d="M232 214 C 196 178 128 162 58 188 C 74 204 84 222 86 246 C 104 238 122 244 132 262 C 148 252 168 258 178 278 C 192 270 208 276 216 294 L 232 300 Z"/><path d="M212 206 L 226 150 L 246 186 L 266 186 L 286 150 L 300 206 C 304 250 300 300 256 350 C 212 300 208 250 212 206 Z"/></g></svg>"##;

/// Feuille de style commune (sans `<style>` : la page Twitch y ajoute son nonce CSP).
pub const CSS: &str = r#"
  :root { color-scheme: dark; }
  * { box-sizing: border-box; }
  body { margin: 0; min-height: 100vh; display: grid; place-items: center; color: #F5F5FA;
         background: radial-gradient(48rem 30rem at 15% -10%, rgb(102 80 245 / .28), transparent 62%),
                     radial-gradient(40rem 26rem at 90% -15%, rgb(38 198 243 / .14), transparent 60%), #07070C;
         font: 15px/1.55 "Segoe UI Variable", "Segoe UI", system-ui, sans-serif; }
  main { width: min(26rem, 100% - 3rem); text-align: center; padding: 2.5rem 2rem 2.25rem; border-radius: 22px;
         border: 1px solid #23232F; background: linear-gradient(180deg, rgb(15 15 23 / .94), rgb(15 15 23 / .8));
         box-shadow: inset 0 1px 0 rgb(255 255 255 / .06), 0 30px 70px -30px rgb(0 0 0 / .9); }
  .logo { width: 3.5rem; height: 3.5rem; display: block; margin: 0 auto 1.25rem; filter: drop-shadow(0 10px 22px rgb(91 71 245 / .55)); }
  .brand { margin: 0 0 1.5rem; font-size: .72rem; font-weight: 600; letter-spacing: .14em; text-transform: uppercase; color: #B4A5FF; }
  .mark { width: 3rem; height: 3rem; margin: 0 auto 1rem; display: block; }
  .mark circle { fill: none; stroke: #8877F7; stroke-width: 3; }
  .mark path { fill: none; stroke: #fff; stroke-width: 4; stroke-linecap: round; stroke-linejoin: round; display: none; }
  body[data-state="working"] .mark circle { stroke-dasharray: 60 140; animation: spin .9s linear infinite; transform-origin: 50% 50%; }
  body[data-state="success"] .mark circle { stroke: #3EE0A0; }
  body[data-state="success"] .ok { display: block; }
  body[data-state="error"] .mark circle { stroke: #FF7B86; }
  body[data-state="error"] .ko { display: block; }
  h1 { margin: 0 0 .5rem; font-size: 1.45rem; font-weight: 650; letter-spacing: -.015em; }
  p { margin: 0; color: #9898AC; }
  @keyframes spin { to { transform: rotate(360deg); } }
  @media (prefers-reduced-motion: reduce) { body[data-state="working"] .mark circle { animation: none; } }
"#;

/// Repère d'état (cercle, coche ou croix selon `body[data-state]`).
pub const STATUS_MARK: &str = r#"<svg class="mark" viewBox="0 0 72 72" aria-hidden="true"><circle cx="36" cy="36" r="32" /><path class="ok" d="M23 37l9 9 18-20" /><path class="ko" d="M26 26l20 20M46 26L26 46" /></svg>"#;

/// Page fixe : réussite (`ok`) ou échec. `title`, `heading` et `message` sont du texte de Batlay (jamais de l'utilisateur).
pub fn status_page(title: &str, heading: &str, message: &str, ok: bool) -> String {
    format!(
        "<!DOCTYPE html><html lang=\"fr\"><head><meta charset=\"utf-8\" /><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" /><title>{title}</title><style>{CSS}</style></head><body data-state=\"{state}\"><main role=\"status\">{LOGO_SVG}<p class=\"brand\">Batlay</p>{STATUS_MARK}<h1>{heading}</h1><p>{message}</p></main></body></html>",
        state = if ok { "success" } else { "error" }
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn page_is_self_contained() {
        let page = super::status_page("Batlay · Spotify", "Spotify connecté", "Vous pouvez fermer cet onglet.", true);
        assert!(page.contains("data-state=\"success\"") && page.contains("<h1>Spotify connecté</h1>"));
        // Rien d'externe : ni adresse http(s) chargée, ni police distante.
        assert!(!page.contains("src=\"http") && !page.contains("url(http") && !page.contains("@import"));
        assert!(super::status_page("t", "h", "m", false).contains("data-state=\"error\""));
    }
}
