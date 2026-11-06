import { describe, it, expect } from "vitest";
// Le hub de la fenêtre Live (journal, compteurs, chats) est dans le cœur de
// Batlay, en Rust : crates/batlay-core/src/live.rs. Ici, ce que fait l'interface.
import { acceptsMessage, createFeedReducer, parseOverlayOptions, buildOverlaySearch, DEFAULT_OPTIONS } from "../twitch-overlay/chat-feed";
import type { ChatMessage } from "../shared/twitch";

const twitchMsg = (id: string, userId = "u1"): ChatMessage => ({
  id, userId, login: "nyx", displayName: "Nyx", color: "#ff0000", badges: [], fragments: [{ type: "text", text: "salut" }], text: "salut", timestamp: 1, highlight: "none", replyTo: null,
});

describe("overlay du chat : plateformes", () => {
  it("src= choisit la plateforme ; absent = les deux (les anciens messages sont Twitch)", () => {
    expect(parseOverlayOptions("").sources).toBe("all");
    expect(parseOverlayOptions("?src=tiktok&pico=1")).toMatchObject({ sources: "tiktok", platformIcon: true });
    expect(parseOverlayOptions("?src=youtube").sources).toBe("all");
    expect(buildOverlaySearch({ ...DEFAULT_OPTIONS, sources: "twitch", platformIcon: true })).toBe("?src=twitch&pico=1");
    expect(acceptsMessage({ sources: "twitch" }, {})).toBe(true);
    expect(acceptsMessage({ sources: "twitch" }, { platform: "tiktok" })).toBe(false);
    const reduce = createFeedReducer({ max: 10, fadeMs: 0, sources: "tiktok" });
    const state = reduce([], { type: "add", message: twitchMsg("a"), now: 0 });
    expect(state).toHaveLength(0);
    expect(reduce(state, { type: "add", message: { ...twitchMsg("b"), platform: "tiktok" }, now: 0 })).toHaveLength(1);
  });
});

describe("préférences d'affichage de l'interface", async () => {
  const { applyUiPrefs, normalizeDensity, normalizeMotion, ACCENT_PRESETS } = await import("../src/theme/ui-prefs");
  it("valeurs inconnues → valeurs par défaut ; attributs posés sur <html>", () => {
    expect(normalizeDensity("compact")).toBe("compact");
    expect(normalizeDensity("énorme")).toBe("comfortable");
    expect(normalizeMotion(undefined)).toBe("full");
    const attrs: Record<string, string> = {};
    applyUiPrefs({ setAttribute: (k, v) => (attrs[k] = v) }, { density: "compact", uiMotion: "reduced" });
    expect(attrs).toEqual({ "data-density": "compact", "data-motion": "reduced" });
    applyUiPrefs({ setAttribute: (k, v) => (attrs[k] = v) }, {});
    expect(attrs).toEqual({ "data-density": "comfortable", "data-motion": "full" });
    for (const p of ACCENT_PRESETS) expect(p.hex).toMatch(/^#[0-9A-F]{6}$/);
  });
});
