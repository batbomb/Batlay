/**
 * Fenêtre « Live » : journal d'activité, chats récents (Twitch et TikTok) et
 * compteurs du live. Types de l'état envoyé par le cœur de Batlay
 * (crates/batlay-core/src/live.rs) et fonctions PURES de l'interface
 * (testées dans tests/live.test.ts).
 */

import { ALERT_KIND_LABELS, type AlertEvent, type AlertKind, type AlertSource } from "./alerts.js";
import type { ChatMessage } from "./twitch.js";

export type LivePlatform = "twitch" | "tiktok";
export const LIVE_PLATFORM_LABELS: Record<LivePlatform | "test" | "batlay", string> = {
  twitch: "Twitch",
  tiktok: "TikTok",
  test: "Test",
  batlay: "Batlay",
};

// ---------------------------------------------------------------------------
// Journal d'activité
// ---------------------------------------------------------------------------

/** Ce qui est arrivé à une alerte : la réponse à « pourquoi je ne l'ai pas vue ? ». */
export type AlertOutcome = "shown" | "filtered" | "rate" | "no-source" | "invalid";

export const ALERT_OUTCOME_LABELS: Record<AlertOutcome, string> = {
  shown: "Affichée",
  filtered: "Non affichée : ce type d'alerte ou cette plateforme est désactivé",
  rate: "Non affichée : trop d'alertes en peu de temps",
  "no-source": "Non affichée : aucune page d'alertes ouverte dans le logiciel de streaming",
  invalid: "Ignorée : événement illisible",
};

export type ActivityEntry =
  | {
      id: string;
      at: number;
      type: "alert";
      source: AlertSource;
      kind: AlertKind;
      user: string;
      /** Résumé lisible : « 3 × Rose », « 12 mois », « 5 abonnements »… */
      detail: string;
      outcome: AlertOutcome;
      /** Événement complet, pour « Rejouer » (absent si illisible). */
      event?: AlertEvent;
    }
  | {
      id: string;
      at: number;
      type: "system";
      source: LivePlatform | "batlay";
      level: "info" | "success" | "warn" | "error";
      text: string;
    };

export function alertTitle(kind: AlertKind): string {
  return ALERT_KIND_LABELS[kind];
}

// ---------------------------------------------------------------------------
// Compteurs du live (depuis l'ouverture de Batlay, ou depuis « Remettre à zéro »)
// ---------------------------------------------------------------------------

export interface PlatformCounters {
  follows: number;
  /** Abonnements, réabonnements et Super Fans. */
  subs: number;
  /** Abonnements offerts (Twitch). */
  giftedSubs: number;
  /** Cadeaux TikTok (quantité). */
  gifts: number;
  messages: number;
  /** Spectateurs en ce moment (TikTok ; 0 hors live). */
  viewers: number;
  /** Likes du live (total donné par TikTok). */
  likes: number;
}

export interface LiveCounters {
  since: number;
  twitch: PlatformCounters;
  tiktok: PlatformCounters;
}

const zero = (): PlatformCounters => ({ follows: 0, subs: 0, giftedSubs: 0, gifts: 0, messages: 0, viewers: 0, likes: 0 });
export const emptyCounters = (now: number): LiveCounters => ({ since: now, twitch: zero(), tiktok: zero() });

// ---------------------------------------------------------------------------
// Chats récents
// ---------------------------------------------------------------------------

export interface LiveChatMessage extends ChatMessage {
  platform: LivePlatform;
}

// ---------------------------------------------------------------------------
// Vue d'ensemble envoyée à la fenêtre Live
// ---------------------------------------------------------------------------

export interface LiveSnapshot {
  activity: ActivityEntry[];
  chat: LiveChatMessage[];
  counters: LiveCounters;
}

export type LiveUpdate =
  | { type: "activity"; entry: ActivityEntry }
  | { type: "chat"; message: LiveChatMessage }
  | { type: "chat-remove"; ids: string[] }
  | { type: "counters"; counters: LiveCounters }
  | { type: "reset"; snapshot: LiveSnapshot };

export const LIVE_LIMITS = { activity: 300, chat: 300 } as const;

/** Filtres du journal dans la fenêtre Live. */
export type ActivityFilter = "all" | "alerts" | "system";

export function filterActivity(entries: ActivityEntry[], filter: ActivityFilter, platform: LivePlatform | "all"): ActivityEntry[] {
  return entries.filter((e) => {
    if (filter === "alerts" && e.type !== "alert") return false;
    if (filter === "system" && e.type !== "system") return false;
    if (platform !== "all" && e.source !== platform) return false;
    return true;
  });
}

// ---------------------------------------------------------------------------
// Infos du live pour les overlays (variables {lastFollower}… et barres d'objectif)
// ---------------------------------------------------------------------------

/** Ce que reçoivent les pages d'overlay : totaux Twitch + TikTok et derniers spectateurs (vrais événements seulement). */
export interface LiveStats {
  followers: number;
  subs: number;
  /** Abonnements offerts (Twitch) + cadeaux TikTok (quantité). */
  gifts: number;
  messages: number;
  /** Spectateurs TikTok en ce moment. */
  viewers: number;
  /** Likes du live TikTok. */
  likes: number;
  lastFollower: string;
  lastSub: string;
  /** « Pseudo (3 × Rose) » ou « Pseudo (5 abonnements) ». */
  lastGift: string;
}

export const EMPTY_LIVE_STATS: LiveStats = { followers: 0, subs: 0, gifts: 0, messages: 0, viewers: 0, likes: 0, lastFollower: "", lastSub: "", lastGift: "" };

/** Valeurs d'exemple (éditeur, miniatures). */
export const SAMPLE_LIVE_STATS: LiveStats = { followers: 12, subs: 3, gifts: 47, messages: 318, viewers: 86, likes: 4210, lastFollower: "Nyx_Live", lastSub: "Kaya", lastGift: "Rémi (5 × Rose)" };

/** Variables du live pour les textes des overlays. */
export const LIVE_VARIABLES = ["lastFollower", "lastSub", "lastGift", "followers", "subs", "gifts", "messages", "viewers", "likes"] as const;
export type LiveVariable = (typeof LIVE_VARIABLES)[number];

/** 4210 → « 4 210 » (espace fine insécable), lisible à l'écran. */
export function formatCount(n: number): string {
  return String(Math.max(0, Math.floor(n))).replace(/\B(?=(\d{3})+(?!\d))/g, "\u202f");
}

export function liveTemplateVars(stats: LiveStats): Record<LiveVariable, string> {
  return {
    lastFollower: stats.lastFollower,
    lastSub: stats.lastSub,
    lastGift: stats.lastGift,
    followers: String(stats.followers),
    subs: String(stats.subs),
    gifts: String(stats.gifts),
    messages: String(stats.messages),
    viewers: String(stats.viewers),
    likes: formatCount(stats.likes),
  };
}

/** Valide des statistiques reçues (page d'overlay : jamais de confiance aveugle dans un message réseau). */
export function normalizeLiveStats(raw: unknown): LiveStats {
  const r = typeof raw === "object" && raw !== null ? (raw as Record<string, unknown>) : {};
  const n = (v: unknown) => (typeof v === "number" && Number.isFinite(v) && v >= 0 ? Math.floor(v) : 0);
  const t = (v: unknown) => (typeof v === "string" ? v.slice(0, 80) : "");
  return { followers: n(r.followers), subs: n(r.subs), gifts: n(r.gifts), messages: n(r.messages), viewers: n(r.viewers), likes: n(r.likes), lastFollower: t(r.lastFollower), lastSub: t(r.lastSub), lastGift: t(r.lastGift) };
}
