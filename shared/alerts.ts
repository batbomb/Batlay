/**
 * Alertes (nouveau follower, nouvel abonné, réabonnement, abonnement offert)
 * — types et validation PURS, partagés par le tableau de bord et la page
 * overlay. Le cœur de Batlay (crates/batlay-core/src/alerts, en Rust)
 * applique les mêmes règles. Testés dans tests/alerts.test.ts.
 *
 * Sources :
 *   - Twitch : EventSub officiel (channel.follow, channel.subscribe,
 *     channel.subscription.message, channel.subscription.gift) ;
 *   - TikTok : pas d'API officielle pour les événements d'un live. Batlay se
 *     branche lui-même sur le live (page du live invisible, voir
 *     crates/batlay-core/src/tiktok) et expose aussi une adresse locale protégée par une clé
 *     secrète, qu'un outil tiers (TikFinity, Streamer.bot…) peut appeler ;
 *   - test : boutons « Tester » de Batlay.
 * Tout ce qui arrive par le réseau passe par `normalizeIncomingAlert`.
 */

export const ALERT_KINDS = ["follow", "subscribe", "resub", "gift", "tiktokGift"] as const;
export type AlertKind = (typeof ALERT_KINDS)[number];
export const ALERT_SOURCES = ["twitch", "tiktok", "test"] as const;
export type AlertSource = (typeof ALERT_SOURCES)[number];

export const ALERT_KIND_LABELS: Record<AlertKind, string> = {
  follow: "Nouveau follower",
  subscribe: "Nouvel abonné",
  resub: "Réabonnement",
  gift: "Abonnements offerts",
  tiktokGift: "Cadeau TikTok",
};

export interface AlertEvent {
  id: string;
  kind: AlertKind;
  source: AlertSource;
  /** Pseudo affiché. */
  user: string;
  /** Nombre d'abonnements offerts, ou quantité d'un cadeau TikTok. */
  amount?: number;
  /** Nom du cadeau TikTok (« Rose »…). */
  gift?: string;
  /** Mois cumulés (réabonnement). */
  months?: number;
  /** Niveau Twitch : « 1 », « 2 », « 3 », « Prime ». */
  tier?: string;
  /** Message du spectateur (réabonnement). */
  message?: string;
  at: number;
}
export const ALERTS_WS_PATH = "/ws-alerts";
export const ALERT_MEDIA_PREFIX = "/api/alert-media/";

/** Messages poussés à la page overlay des alertes. */
export type AlertsOverlayMessage =
  | { type: "config"; payload: AlertsConfig }
  | { type: "alert"; payload: AlertEvent }
  /** Arrête l'alerte en cours et vide la file (bouton « Passer »). */
  | { type: "skip" };

// ---------------------------------------------------------------------------
// Validation des événements entrants
// ---------------------------------------------------------------------------

type Obj = Record<string, unknown>;
const isObj = (v: unknown): v is Obj => typeof v === "object" && v !== null && !Array.isArray(v);

/** Texte affiché à l'antenne : caractères de contrôle et invisibles retirés, espaces réduits, longueur bornée. */
export function cleanAlertText(v: unknown, max: number): string {
  if (typeof v !== "string" && typeof v !== "number") return "";
  return String(v)
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001f\u007f-\u009f\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .normalize("NFC")
    .slice(0, max);
}

function positiveInt(v: unknown, max: number): number | undefined {
  const n = typeof v === "string" && /^\d{1,6}$/.test(v.trim()) ? Number(v) : v;
  return typeof n === "number" && Number.isInteger(n) && n > 0 ? Math.min(n, max) : undefined;
}

/** Noms acceptés pour le type d'événement (outils tiers : vocabulaire variable). */
const KIND_ALIASES: Record<string, AlertKind> = {
  follow: "follow",
  follower: "follow",
  sub: "subscribe",
  subscribe: "subscribe",
  subscription: "subscribe",
  resub: "resub",
  resubscribe: "resub",
  gift: "gift",
  subgift: "gift",
  giftsub: "gift",
  tiktokgift: "tiktokGift",
  present: "tiktokGift",
  cadeau: "tiktokGift",
};

export function parseAlertKind(v: unknown): AlertKind | null {
  if (typeof v !== "string") return null;
  return KIND_ALIASES[v.trim().toLowerCase().replace(/[\s_.-]/g, "")] ?? null;
}

/**
 * Événement reçu (webhook local, EventSub converti, test) → événement sûr, ou
 * `null` s'il est inexploitable. Champs inconnus ignorés, textes nettoyés.
 */
export function normalizeIncomingAlert(raw: unknown, source: AlertSource, makeId: () => string, now: number): AlertEvent | null {
  if (!isObj(raw)) return null;
  const kind = parseAlertKind(raw.kind ?? raw.type ?? raw.event);
  if (!kind) return null;
  const user = cleanAlertText(raw.user ?? raw.username ?? raw.nickname ?? raw.name, 40) || "Quelqu'un";
  const event: AlertEvent = { id: makeId(), kind, source, user, at: now };
  const amount = positiveInt(raw.amount ?? raw.count ?? raw.total, 1000);
  const months = positiveInt(raw.months ?? raw.cumulative_months, 1000);
  const tier = cleanAlertText(raw.tier, 10);
  const message = cleanAlertText(raw.message ?? raw.comment, 200);
  const gift = cleanAlertText(raw.gift ?? raw.giftName, 40);
  if (amount) event.amount = amount;
  if (months) event.months = months;
  if (tier) event.tier = tier;
  if (message) event.message = message;
  if (gift) event.gift = gift;
  return event;
}

// ---------------------------------------------------------------------------
// Réglages des alertes (stockés par le cœur de Batlay, poussés à la page)
// ---------------------------------------------------------------------------

/** Identifiant d'un fichier importé : hash du contenu + extension (voir alert-media-store.ts). */
export const MEDIA_ID_RE = /^[0-9a-f]{24}\.(png|jpg|gif|webp|mp3|ogg|wav)$/;
export const isMediaId = (v: unknown): v is string => typeof v === "string" && MEDIA_ID_RE.test(v);
export const isImageMediaId = (v: unknown): v is string => isMediaId(v) && /\.(png|jpg|gif|webp)$/.test(v);
export const isSoundMediaId = (v: unknown): v is string => isMediaId(v) && /\.(mp3|ogg|wav)$/.test(v);

/** Son : aucun, carillon intégré (synthétisé, aucun fichier) ou fichier importé. */
export type AlertSound = { type: "none" } | { type: "chime" } | { type: "file"; mediaId: string };

export interface AlertStyle {
  enabled: boolean;
  /** Titre, avec variables {user} {amount} {months} {tier} {message} {gift} {platform}. */
  title: string;
  /** Sous-titre (même variables). */
  subtitle: string;
  imageId: string | null;
  sound: AlertSound;
  /** 0 → 1. */
  volume: number;
  /** Durée d'affichage (hors animations), ms. */
  durationMs: number;
  enter: string;
  exit: string;
  animMs: number;
  layout: "stack" | "side";
  accentColor: string;
  textColor: string;
  fontSize: number;
  /** Carte sombre derrière le texte. */
  card: boolean;
}

export interface AlertsConfig {
  version: 1;
  kinds: Record<AlertKind, AlertStyle>;
  /** Sources acceptées (une source désactivée n'affiche rien). */
  sources: Record<Exclude<AlertSource, "test">, boolean>;
  /** Pause entre deux alertes de la file, ms. */
  gapMs: number;
}

const base = (title: string, subtitle: string, accentColor: string): AlertStyle => ({
  enabled: true,
  title,
  subtitle,
  imageId: null,
  sound: { type: "chime" },
  volume: 0.6,
  durationMs: 5000,
  enter: "pop",
  exit: "fade",
  animMs: 500,
  layout: "stack",
  accentColor,
  textColor: "#FFFFFF",
  fontSize: 40,
  card: true,
});

export const DEFAULT_ALERTS_CONFIG: AlertsConfig = {
  version: 1,
  kinds: {
    follow: base("{user}", "nous suit maintenant !", "#9B6BFF"),
    subscribe: base("{user}", "vient de s'abonner !", "#37E29A"),
    resub: base("{user}", "est abonné depuis {months} mois !", "#37E29A"),
    gift: base("{user}", "offre {amount} abonnement(s) !", "#F5C542"),
    tiktokGift: base("{user}", "envoie {amount} × {gift} !", "#FF3B6B"),
  },
  sources: { twitch: true, tiktok: true },
  gapMs: 600,
};

const HEX = /^#[0-9a-fA-F]{6}$/;
const EFFECT = /^[a-zA-Z]{2,20}$/;
const num = (v: unknown, min: number, max: number, fallback: number) =>
  typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(min, v)) : fallback;

function normalizeSound(v: unknown, fallback: AlertSound): AlertSound {
  if (!isObj(v)) return fallback;
  if (v.type === "none") return { type: "none" };
  if (v.type === "chime") return { type: "chime" };
  if (v.type === "file" && isSoundMediaId(v.mediaId)) return { type: "file", mediaId: v.mediaId };
  return fallback;
}

function normalizeStyle(v: unknown, d: AlertStyle): AlertStyle {
  const s = isObj(v) ? v : {};
  return {
    enabled: typeof s.enabled === "boolean" ? s.enabled : d.enabled,
    title: typeof s.title === "string" ? s.title.slice(0, 120) : d.title,
    subtitle: typeof s.subtitle === "string" ? s.subtitle.slice(0, 200) : d.subtitle,
    imageId: s.imageId === null ? null : isImageMediaId(s.imageId) ? s.imageId : d.imageId,
    sound: normalizeSound(s.sound, d.sound),
    volume: num(s.volume, 0, 1, d.volume),
    durationMs: Math.round(num(s.durationMs, 1000, 30000, d.durationMs)),
    // L'effet est revérifié au rendu par le moteur d'animations (src/animation/config.ts).
    enter: typeof s.enter === "string" && EFFECT.test(s.enter) ? s.enter : d.enter,
    exit: typeof s.exit === "string" && EFFECT.test(s.exit) ? s.exit : d.exit,
    animMs: Math.round(num(s.animMs, 50, 3000, d.animMs)),
    layout: s.layout === "side" ? "side" : s.layout === "stack" ? "stack" : d.layout,
    accentColor: typeof s.accentColor === "string" && HEX.test(s.accentColor) ? s.accentColor : d.accentColor,
    textColor: typeof s.textColor === "string" && HEX.test(s.textColor) ? s.textColor : d.textColor,
    fontSize: Math.round(num(s.fontSize, 16, 120, d.fontSize)),
    card: typeof s.card === "boolean" ? s.card : d.card,
  };
}

export function normalizeAlertsConfig(raw: unknown): AlertsConfig {
  const r = isObj(raw) ? raw : {};
  const kinds = isObj(r.kinds) ? r.kinds : {};
  const sources = isObj(r.sources) ? r.sources : {};
  const d = DEFAULT_ALERTS_CONFIG;
  return {
    version: 1,
    kinds: Object.fromEntries(ALERT_KINDS.map((k) => [k, normalizeStyle(kinds[k], d.kinds[k])])) as Record<AlertKind, AlertStyle>,
    sources: {
      twitch: typeof sources.twitch === "boolean" ? sources.twitch : d.sources.twitch,
      tiktok: typeof sources.tiktok === "boolean" ? sources.tiktok : d.sources.tiktok,
    },
    gapMs: Math.round(num(r.gapMs, 0, 5000, d.gapMs)),
  };
}

/** L'alerte doit-elle s'afficher ? (type activé, source acceptée ; un test passe toujours si le type est activé). */
export function shouldShowAlert(config: AlertsConfig, event: AlertEvent): boolean {
  if (!config.kinds[event.kind].enabled) return false;
  return event.source === "test" || config.sources[event.source];
}

const PLATFORM_LABELS: Record<AlertSource, string> = { twitch: "Twitch", tiktok: "TikTok", test: "Test" };

/** Remplace les variables. Le résultat est du TEXTE (rendu par React, jamais en HTML). */
export function renderAlertText(template: string, event: AlertEvent): string {
  const vars: Record<string, string> = {
    user: event.user,
    amount: event.amount !== undefined ? String(event.amount) : "1",
    months: event.months !== undefined ? String(event.months) : "1",
    tier: event.tier ?? "",
    message: event.message ?? "",
    gift: event.gift ?? "cadeau",
    platform: PLATFORM_LABELS[event.source],
  };
  return template.replace(/\{(user|amount|months|tier|message|gift|platform)\}/g, (_, key: string) => vars[key]);
}

/** Événement d'exemple pour les boutons « Tester » et l'aperçu. */
export function sampleAlert(kind: AlertKind, id: string, now: number): AlertEvent {
  const samples: Record<AlertKind, Omit<AlertEvent, "id" | "at" | "kind" | "source">> = {
    follow: { user: "Nyx_Live" },
    subscribe: { user: "Kaya", tier: "1" },
    resub: { user: "Rémi_Été", months: 14, tier: "1", message: "Toujours là, merci pour les lives !" },
    gift: { user: "Batlay_Fan", amount: 5, tier: "1" },
    tiktokGift: { user: "Lina.mp4", amount: 3, gift: "Rose" },
  };
  return { id, kind, source: "test", at: now, ...samples[kind] };
}
