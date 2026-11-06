/**
 * Source de vérité UNIQUE du thème de Batlay (module pur, sans dépendance).
 *
 * Partagé par :
 *   - l'application (couleur de fond des fenêtres : crates/batlay-core/src/theme.rs) ;
 *   - le renderer (variables CSS appliquées sur <html>, voir
 *     src/theme/apply-theme.ts) ;
 *   - Tailwind, qui référence ces mêmes noms de variables (tailwind.config.js) ;
 *   - les tests (contraste WCAG, cohérence Tailwind <-> tokens).
 *
 * Les couleurs sont stockées en hexadécimal ici, puis exposées au CSS sous
 * forme de triplets RGB (« 10 10 15 »). C'est ce format qui permet à Tailwind
 * de composer l'opacité : `bg-base-800/60` -> `rgb(var(--base-800) / 0.6)`.
 *
 * Identité visuelle de Batlay 0.5 (« Nova ») : encre plus profonde, textes
 * secondaires et indices plus lisibles, accent indigo + cyan (voir
 * src/theme/accent.ts). Ces valeurs sont figées par tests/theme.test.ts :
 * ne les changer qu'en le décidant explicitement (nouvelle identité).
 *
 * NE PAS thématiser : la page overlay servie à OBS, OverlayPreview, et les
 * fonds d'aperçu de l'éditeur (Damier / Sombre / Clair / Scène). Ce sont des
 * rendus de ce que verra le spectateur, pas de l'interface de Batlay.
 */

export type ThemePreference = "dark" | "light" | "system";
export type ResolvedTheme = "dark" | "light";

export const THEME_PREFERENCES: readonly ThemePreference[] = ["dark", "light", "system"];

/**
 * Valeur par défaut = sombre : c'est l'apparence historique, et un ancien
 * fichier de config qui contient déjà "dark" reste valide tel quel.
 */
export const DEFAULT_THEME_PREFERENCE: ThemePreference = "dark";

export function isThemePreference(value: unknown): value is ThemePreference {
  return typeof value === "string" && (THEME_PREFERENCES as readonly string[]).includes(value);
}

/** Toute valeur inconnue (config corrompue, ancienne version) retombe sur le défaut. */
export function normalizeThemePreference(value: unknown): ThemePreference {
  return isThemePreference(value) ? value : DEFAULT_THEME_PREFERENCE;
}

/** « system » suit Windows ; « dark » et « light » sont imposés. */
export function resolveTheme(preference: ThemePreference, systemPrefersDark: boolean): ResolvedTheme {
  if (preference === "system") return systemPrefersDark ? "dark" : "light";
  return preference;
}

// ---------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------

/**
 * Noms des tokens. Chacun devient la variable CSS `--<nom>`.
 *
 * Surfaces et bordures
 *   base-950  fond de l'application            base-900  cartes, barre latérale
 *   base-800  champs, survols, éléments actifs base-700  bordures de champs, pistes, boutons inactifs
 *   base-600  décoration (pastilles, bordure de toast)
 *   line      bordures et séparateurs discrets des cartes
 * Texte
 *   fg        texte principal   muted  texte secondaire   faint  indices très discrets
 * Statuts (texte lisible SUR les surfaces ci-dessus)
 *   accent    liens / éléments actifs violets   ok  succès   danger / danger-soft  erreurs
 *   warn / warn-strong  avertissements
 * Divers (variables CSS seulement, pas de couleur Tailwind)
 *   grid      points de la grille de l'éditeur
 *   scrollbar barre de défilement
 *   checker-a / checker-b   damier de transparence des pastilles de couleur
 */
export const THEME_TOKEN_NAMES = [
  "base-950",
  "base-900",
  "base-800",
  "base-700",
  "base-600",
  "line",
  "fg",
  "muted",
  "faint",
  "accent",
  "ok",
  "danger",
  "danger-soft",
  "warn",
  "warn-strong",
  "grid",
  "scrollbar",
  "checker-a",
  "checker-b",
] as const;

export type ThemeTokenName = (typeof THEME_TOKEN_NAMES)[number];

/** Tokens qui ont une couleur Tailwind (les autres ne servent qu'en CSS en ligne). */
export const TAILWIND_TOKEN_NAMES = THEME_TOKEN_NAMES.filter(
  (name) => !["grid", "scrollbar", "checker-a", "checker-b"].includes(name)
) as readonly ThemeTokenName[];

export const THEME_TOKENS: Record<ResolvedTheme, Record<ThemeTokenName, string>> = {
  dark: {
    "base-950": "#07070C", // encre profonde (fond de l'application)
    "base-900": "#0F0F17",
    "base-800": "#171721",
    "base-700": "#23232F",
    "base-600": "#32323F",
    line: "#1D1D28",
    fg: "#F5F5FA",
    muted: "#9898AC",
    // Indices : ≥ 3:1 sur les surfaces (la 0.4 restait à ~1,6:1).
    faint: "#5E5E73",
    accent: "#B4A5FF",
    ok: "#3EE0A0",
    danger: "#FF7B86",
    "danger-soft": "#FFA8AF",
    warn: "#FDE68A",
    "warn-strong": "#FCD34D",
    grid: "#1A1A24",
    scrollbar: "#2A2A37",
    "checker-a": "#3A3A46",
    "checker-b": "#25252F",
  },
  light: {
    "base-950": "#F3F3F8",
    "base-900": "#FFFFFF",
    "base-800": "#ECECF3",
    "base-700": "#DDDDE7",
    "base-600": "#C7C7D4",
    line: "#E3E3EC",
    fg: "#11111A",
    muted: "#575769", // >= 4,5:1 sur base-950/900/800/700
    faint: "#7A7A8E", // >= 3:1 (indices et icônes décoratives)
    accent: "#4B36D6",
    ok: "#08704A",
    danger: "#B42318",
    "danger-soft": "#B42318",
    warn: "#8A3B0C",
    "warn-strong": "#8A3B0C",
    grid: "#DDDDE7",
    scrollbar: "#C4C4D2",
    "checker-a": "#D8D8E0",
    "checker-b": "#F2F2F7",
  },
};

/**
 * Couleur de fond de la fenêtre native, affichée avant le premier rendu :
 * alignée sur `base-950` pour qu'aucun flash sombre/clair n'apparaisse.
 */
export const WINDOW_BACKGROUND: Record<ResolvedTheme, string> = {
  dark: THEME_TOKENS.dark["base-950"],
  light: THEME_TOKENS.light["base-950"],
};

export function cssVarName(token: ThemeTokenName): string {
  return `--${token}`;
}

/** "#0A0A0F" -> "10 10 15" (format attendu par `rgb(var(--x) / <alpha>)`). */
export function hexToRgbTriplet(hex: string): string {
  const match = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!match) throw new Error(`Couleur hexadécimale invalide : "${hex}"`);
  const value = parseInt(match[1], 16);
  return `${(value >> 16) & 255} ${(value >> 8) & 255} ${value & 255}`;
}

/** Variables CSS complètes d'un thème : { "--base-950": "10 10 15", ... }. */
export function themeCssVariables(theme: ResolvedTheme): Record<string, string> {
  const out: Record<string, string> = {};
  for (const token of THEME_TOKEN_NAMES) {
    out[cssVarName(token)] = hexToRgbTriplet(THEME_TOKENS[theme][token]);
  }
  return out;
}
