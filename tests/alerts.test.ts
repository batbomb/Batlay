import { describe, it, expect } from "vitest";
import {
  DEFAULT_ALERTS_CONFIG,
  normalizeAlertsConfig,
  normalizeIncomingAlert,
  parseAlertKind,
  renderAlertText,
  sampleAlert,
  shouldShowAlert,
  type AlertEvent,
} from "../shared/alerts";
import { alertQueueReducer, emptyQueue, MAX_PENDING } from "@/alerts/queue";
import { alertMotion } from "@/alerts/AlertPlayer";
import { artworkBackdropStyle, fillToCss, resolveArtworkFill, defaultFill } from "@/utils/overlay-style";
import { componentContentKey } from "@/utils/overlay-motion";

let n = 0;
const id = () => `id${++n}`;

describe("événements entrants (webhook TikTok, tests)", () => {
  it("type reconnu sous plusieurs noms ; inconnu refusé", () => {
    for (const [raw, kind] of [["follow", "follow"], ["Follower", "follow"], ["sub", "subscribe"], ["subscription", "subscribe"], ["re-sub", "resub"], ["sub_gift", "gift"], ["GIFT", "gift"]] as const) {
      expect(parseAlertKind(raw), raw).toBe(kind);
    }
    for (const raw of ["like", "share", "", 3, null, "follow; drop"]) expect(parseAlertKind(raw), String(raw)).toBeNull();
  });

  it("textes nettoyés et bornés, nombres validés, champs inconnus ignorés", () => {
    const e = normalizeIncomingAlert(
      { type: "gift", user: "  Zoé\u0000​  la\nreine  ", amount: "5", months: -3, message: "x".repeat(500), __proto__: { evil: 1 }, html: "<b>" },
      "tiktok",
      id,
      1000
    )!;
    expect(e).toEqual({ id: e.id, kind: "gift", source: "tiktok", user: "Zoé la reine", amount: 5, message: "x".repeat(200), at: 1000 });
    expect(normalizeIncomingAlert({ type: "follow" }, "tiktok", id, 0)!.user).toBe("Quelqu'un");
    expect(normalizeIncomingAlert({ type: "follow", user: "A".repeat(80) }, "tiktok", id, 0)!.user).toHaveLength(40);
    expect(normalizeIncomingAlert({ type: "gift", user: "x", amount: 99999 }, "tiktok", id, 0)!.amount).toBe(1000);
    expect(normalizeIncomingAlert("follow", "tiktok", id, 0)).toBeNull();
    expect(normalizeIncomingAlert({ user: "x" }, "tiktok", id, 0)).toBeNull();
  });

  it("variables du texte ; le résultat reste du texte (jamais du HTML)", () => {
    const e: AlertEvent = { id: "1", kind: "resub", source: "twitch", user: "<img src=x onerror=alert(1)>", months: 14, message: "Salut", at: 0 };
    expect(renderAlertText("{user} — {months} mois sur {platform} : {message} {inconnu}", e)).toBe("<img src=x onerror=alert(1)> — 14 mois sur Twitch : Salut {inconnu}");
    expect(renderAlertText("{amount}", { ...e, kind: "gift" })).toBe("1");
  });
});

describe("réglages des alertes", () => {
  it("absents ou illisibles → valeurs par défaut", () => {
    for (const raw of [undefined, null, 3, "x", {}]) expect(normalizeAlertsConfig(raw)).toEqual(DEFAULT_ALERTS_CONFIG);
  });
  it("valeurs invalides corrigées champ par champ", () => {
    const c = normalizeAlertsConfig({
      kinds: {
        follow: { enabled: false, title: "T".repeat(300), volume: 7, durationMs: -1, accentColor: "red", imageId: "../../etc/passwd", sound: { type: "file", mediaId: "x.exe" }, enter: "<script>", layout: "diagonal" },
      },
      sources: { tiktok: false },
      gapMs: 99999,
    });
    expect(c.kinds.follow).toMatchObject({ enabled: false, volume: 1, durationMs: 1000, accentColor: "#9B6BFF", imageId: null, sound: { type: "chime" }, enter: "pop", layout: "stack" });
    expect(c.kinds.follow.title).toHaveLength(120);
    expect(c.kinds.subscribe).toEqual(DEFAULT_ALERTS_CONFIG.kinds.subscribe);
    expect(c.sources).toEqual({ twitch: true, tiktok: false });
    expect(c.gapMs).toBe(5000);
  });
  it("fichiers importés : seuls les identifiants « hash.extension » sont acceptés", () => {
    const good = "0123456789abcdef01234567";
    const c = normalizeAlertsConfig({ kinds: { gift: { imageId: `${good}.gif`, sound: { type: "file", mediaId: `${good}.mp3` } } } });
    expect(c.kinds.gift.imageId).toBe(`${good}.gif`);
    expect(c.kinds.gift.sound).toEqual({ type: "file", mediaId: `${good}.mp3` });
    // Un son ne peut pas servir d'image et inversement.
    expect(normalizeAlertsConfig({ kinds: { gift: { imageId: `${good}.mp3` } } }).kinds.gift.imageId).toBeNull();
  });
  it("filtrage : type désactivé, source refusée ; un test passe si le type est activé", () => {
    const c = normalizeAlertsConfig({ kinds: { resub: { enabled: false } }, sources: { tiktok: false } });
    const ev = (kind: AlertEvent["kind"], source: AlertEvent["source"]): AlertEvent => ({ id: "x", kind, source, user: "u", at: 0 });
    expect(shouldShowAlert(c, ev("follow", "twitch"))).toBe(true);
    expect(shouldShowAlert(c, ev("follow", "tiktok"))).toBe(false);
    expect(shouldShowAlert(c, ev("follow", "test"))).toBe(true);
    expect(shouldShowAlert(c, ev("resub", "test"))).toBe(false);
  });
  it("animations des alertes : effets revalidés, mouvement réduit respecté", () => {
    const s = { ...DEFAULT_ALERTS_CONFIG.kinds.follow, enter: "inexistant", exit: "none", animMs: 300 };
    expect(alertMotion(s, false).enter?.effect).toBe("fade");
    expect(alertMotion(s, false).exit).toBeNull();
    expect(alertMotion(DEFAULT_ALERTS_CONFIG.kinds.follow, true)).toEqual({ enter: null, exit: null });
  });
});

describe("file d'alertes (page overlay)", () => {
  const ev = (i: string): AlertEvent => ({ id: i, kind: "follow", source: "twitch", user: i, at: 0 });
  it("une à la fois, dans l'ordre, sans doublon", () => {
    let q = emptyQueue();
    for (const i of ["a", "b", "a", "c", "b"]) q = alertQueueReducer(q, { type: "enqueue", event: ev(i) });
    expect(q.current?.id).toBe("a");
    expect(q.pending.map((e) => e.id)).toEqual(["b", "c"]);
    q = alertQueueReducer(q, { type: "done" });
    expect(q.current?.id).toBe("b");
    q = alertQueueReducer(alertQueueReducer(q, { type: "done" }), { type: "done" });
    expect(q.current).toBeNull();
    // Un événement déjà affiché ne revient pas.
    expect(alertQueueReducer(q, { type: "enqueue", event: ev("a") })).toBe(q);
  });
  it("rafale : file bornée, les plus anciennes en attente abandonnées", () => {
    let q = emptyQueue();
    for (let i = 0; i < 100; i++) q = alertQueueReducer(q, { type: "enqueue", event: ev(String(i)) });
    expect(q.current?.id).toBe("0");
    expect(q.pending).toHaveLength(MAX_PENDING);
    expect(q.pending.at(-1)?.id).toBe("99");
  });
  it("« Passer » vide tout", () => {
    let q = alertQueueReducer(alertQueueReducer(emptyQueue(), { type: "enqueue", event: ev("a") }), { type: "enqueue", event: ev("b") });
    q = alertQueueReducer(q, { type: "skip" });
    expect(q.current).toBeNull();
    expect(q.pending).toEqual([]);
  });
});

function hub(overrides: { pages?: number; now?: () => number } = {}) {
  const sent: AlertEvent[] = [];
  let saved: unknown = null;
  let key: string | null = null;
  const h = new AlertHub({
    loadConfig: () => saved,
    saveConfig: (c) => (saved = c),
    loadSecret: () => key,
    saveSecret: (k) => (key = k),
    broadcast: (e) => {
      sent.push(e);
      return overrides.pages ?? 1;
    },
    pushConfig: () => undefined,
    now: overrides.now,
    makeId: id,
  });
  return { h, sent, key: () => key };
}

describe("fond « pochette floutée »", () => {
  const fill = defaultFill({ mode: "artwork", color: "#101018", opacity: 0.9, artwork: { blur: 30, zoom: 1.6, dim: 0.4 } });
  it("réglages validés (fichier importé, ancienne version)", () => {
    expect(resolveArtworkFill(defaultFill({ mode: "artwork" }))).toEqual({ blur: 24, zoom: 1.4, dim: 0.35 });
    expect(resolveArtworkFill({ ...fill, artwork: { blur: 999, zoom: 0, dim: -1 } })).toEqual({ blur: 60, zoom: 1, dim: 0 });
  });
  it("image floutée, agrandie, assombrie ; couleur de secours sans pochette", () => {
    expect(artworkBackdropStyle(fill)).toMatchObject({ transform: "scale(1.6)", filter: "blur(30px) brightness(0.6)", opacity: 0.9, objectFit: "cover" });
    expect(fillToCss(fill)).toBe("rgba(16, 16, 24, 0.9)");
  });
  it("fondu d'une pochette à l'autre : la clé d'un composant à fond pochette suit la pochette", () => {
    const shape = { type: "shape" as const, style: { background: fill } };
    // La clé de contenu reste la même ; c'est OverlayPreview qui y ajoute la pochette (testé dans le navigateur).
    expect(componentContentKey(shape, { id: "a", title: "t", artist: "a", duration: 1, progress: 0, isPlaying: true, artwork: "/a" })).toBe("present");
  });
});
