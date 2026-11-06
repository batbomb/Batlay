/**
 * Préférences d'affichage de l'interface (pas des overlays) : densité et
 * animations. Posées en attributs sur <html> ; les règles sont dans
 * src/styles/globals.css (html[data-density], html[data-motion]).
 */

export type UiDensity = "comfortable" | "compact";
export type UiMotion = "full" | "reduced";

export const UI_DENSITIES: readonly UiDensity[] = ["comfortable", "compact"];
export const UI_MOTIONS: readonly UiMotion[] = ["full", "reduced"];

export const normalizeDensity = (v: unknown): UiDensity => (v === "compact" ? "compact" : "comfortable");
export const normalizeMotion = (v: unknown): UiMotion => (v === "reduced" ? "reduced" : "full");

/** Sous-ensemble de HTMLElement (testable sans DOM). */
export interface UiPrefsRoot {
  setAttribute(name: string, value: string): void;
}

export function applyUiPrefs(root: UiPrefsRoot, prefs: { density?: unknown; uiMotion?: unknown }): void {
  root.setAttribute("data-density", normalizeDensity(prefs.density));
  root.setAttribute("data-motion", normalizeMotion(prefs.uiMotion));
}

/** Teintes proposées en un clic pour la couleur d'accent (la pipette reste disponible). */
export const ACCENT_PRESETS: { hex: string; label: string }[] = [
  { hex: "#6650F5", label: "Nova (Batlay 0.5)" },
  { hex: "#7C4DFF", label: "Violet (Batlay 0.4)" },
  { hex: "#3B82F6", label: "Bleu" },
  { hex: "#06B6D4", label: "Cyan" },
  { hex: "#10B981", label: "Vert" },
  { hex: "#F59E0B", label: "Ambre" },
  { hex: "#F43F5E", label: "Rose" },
  { hex: "#EC4899", label: "Magenta" },
  { hex: "#64748B", label: "Ardoise" },
];
