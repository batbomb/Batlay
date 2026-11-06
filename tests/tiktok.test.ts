import { describe, it, expect } from "vitest";
import { initialTikTokState, normalizeTikTokMode, parseTikTokUsername, TIKTOK_MODES } from "../shared/tiktok";

// Le live TikTok (connexion, décodage du flux, alertes, diagnostic) est dans le
// cœur de Batlay, en Rust, et testé là-bas : crates/batlay-core/src/tiktok.
// Ici, seulement ce que l'interface fait elle-même.

describe("pseudo TikTok", () => {
  it("accepte pseudo, @pseudo et lien du live", () => {
    expect(parseTikTokUsername("Mon.Pseudo_1")).toBe("mon.pseudo_1");
    expect(parseTikTokUsername("  @adil ")).toBe("adil");
    expect(parseTikTokUsername("https://www.tiktok.com/@adil.live/live")).toBe("adil.live");
    expect(parseTikTokUsername("tiktok.com/@adil?lang=fr")).toBe("adil");
  });
  it("refuse le reste", () => {
    for (const bad of ["", "a", "nom avec espace", "x".repeat(25), "<script>", "https://evil.example/@adil", 42, null]) {
      expect(parseTikTokUsername(bad)).toBe(null);
    }
  });
});

describe("méthode de connexion TikTok", () => {
  it("trois méthodes au choix, le navigateur intégré en premier", () => {
    expect(TIKTOK_MODES.map((m) => m.value)).toEqual(["browser", "euler", "local"]);
    expect(initialTikTokState().settings.mode).toBe("browser");
    expect(initialTikTokState().eulerKey).toBe(false);
  });
  it("valeur inconnue = navigateur ; « library » (0.4.x) = Euler", () => {
    expect(normalizeTikTokMode("euler")).toBe("euler");
    expect(normalizeTikTokMode("library")).toBe("euler");
    expect(normalizeTikTokMode("local")).toBe("local");
    for (const other of ["browser", "pirate", undefined, null, 3]) expect(normalizeTikTokMode(other)).toBe("browser");
  });
});
