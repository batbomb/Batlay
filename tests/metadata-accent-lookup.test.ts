import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { cleanMetadataText } from "@/utils/text-encoding";

const { lookupArtwork, normalizeMetadata, buildQueryCandidates, __clearArtworkCache } = await import(
  "@/services/artwork-lookup"
);

const json = (body: unknown) => ({ ok: true, text: async () => JSON.stringify(body) });
const itunesHit = (trackName: string, artistName: string) =>
  json({ results: [{ artworkUrl100: "https://is1.mzstatic.com/z/100x100bb.jpg", trackName, artistName }] });

function mockNetwork(handler: (url: string) => unknown): string[] {
  const calls: string[] = [];
  vi.stubGlobal("fetch", async (url: string) => {
    calls.push(url);
    return handler(url);
  });
  return calls;
}

/**
 * Chemin complet d'un artiste accentué, comme dans l'app :
 *   cœur de Batlay (sortie PowerShell décodée en Rust, testée dans
 *   crates/batlay-core) -> JSON -> cleanMetadataText (system-media-provider)
 *   -> lookupArtwork.
 */
function artistAsSeenBySearch(artistFromWindows: string): string {
  const reply = JSON.stringify({ session: { artist: artistFromWindows } });
  const parsed = JSON.parse(reply);
  return cleanMetadataText(parsed.session.artist);
}

describe("artiste accentué -> recherche de pochette", () => {
  beforeEach(() => __clearArtworkCache());
  afterEach(() => vi.unstubAllGlobals());

  it("la normalisation de recherche conserve les accents (elle ne les retire que pour COMPARER)", () => {
    const meta = normalizeMetadata("Angèle", "Balance ton quoi");
    expect(meta.artist).toBe("Angèle");
    const candidates = buildQueryCandidates(meta);
    expect(candidates[0].artist).toBe("Angèle");
  });

  it("la requête envoyée à iTunes contient l'artiste encodé en UTF-8 (Ang%C3%A8le)", async () => {
    const calls = mockNetwork((url) => (url.includes("itunes") ? itunesHit("Balance ton quoi", "Angèle") : json({})));
    const result = await lookupArtwork(artistAsSeenBySearch("Angèle"), "Balance ton quoi");
    expect(result.url).not.toBeNull();
    expect(calls[0]).toContain("Ang%C3%A8le");
    expect(calls[0]).not.toContain("%EF%BF%BD"); // jamais de U+FFFD
  });

  it("un « è » DÉCOMPOSÉ envoyé par la source est recomposé avant la recherche", async () => {
    const calls = mockNetwork((url) => (url.includes("itunes") ? itunesHit("Balance ton quoi", "Angèle") : json({})));
    await lookupArtwork(artistAsSeenBySearch("Ange\u0300le"), "Balance ton quoi");
    expect(calls[0]).toContain("Ang%C3%A8le");
    expect(calls[0]).not.toContain("e%CC%80");
  });

  it("un artiste que la SOURCE a mal décodé (mojibake « AngÃ¨le ») est réparé avant la recherche", async () => {
    const calls = mockNetwork((url) => (url.includes("itunes") ? itunesHit("Balance ton quoi", "Angèle") : json({})));
    await lookupArtwork(artistAsSeenBySearch("AngÃ¨le"), "Balance ton quoi");
    expect(calls[0]).toContain("Ang%C3%A8le");
  });

  it("multi-artistes accentués : la chaîne complète part à la recherche, non découpée par l'affichage", () => {
    expect(artistAsSeenBySearch("Angèle, Stromae, Tyler, The Creator")).toBe("Angèle, Stromae, Tyler, The Creator");
  });
});
