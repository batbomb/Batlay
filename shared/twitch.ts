/**
 * Twitch : types de l'état envoyé par le cœur de Batlay
 * (crates/batlay-core/src/twitch, en Rust) et utilitaires de l'interface.
 */

/**
 * Port du serveur Twitch : FIXE. Il n'est stocké dans aucun réglage, n'apparaît
 * dans aucun champ de l'interface et le serveur ne l'accepte pas en paramètre
 * en production. Il doit rester identique à l'URI de redirection enregistrée
 * sur https://dev.twitch.tv/console/apps.
 */
export const TWITCH_PORT = 4000 as const;

export const TWITCH_REDIRECT_URI = `http://localhost:${TWITCH_PORT}` as const;

export const TWITCH_OVERLAY_PATH = "/overlay/chat";
export const TWITCH_WS_PATH = "/ws/chat";
export const TWITCH_OVERLAY_URL = `http://localhost:${TWITCH_PORT}${TWITCH_OVERLAY_PATH}` as const;

// ---------------------------------------------------------------------------
// Erreurs
// ---------------------------------------------------------------------------

export type TwitchErrorCode =
  | "access_denied"
  | "authorization_timeout"
  | "invalid_state"
  | "oauth_error"
  | "token_invalid"
  | "token_expired"
  | "missing_scope"
  | "wrong_client"
  | "network"
  | "port_in_use"
  | "server_error"
  | "subscription_failed"
  | "invalid_channel"
  | "channel_not_found"
  | "storage_unavailable";

export interface TwitchError {
  code: TwitchErrorCode;
  /** Message affichable tel quel à l'utilisateur (français, avec la marche à suivre). */
  message: string;
}

// ---------------------------------------------------------------------------
// État exposé au dashboard
// ---------------------------------------------------------------------------

export interface TwitchAccount {
  id: string;
  login: string;
  displayName: string;
  profileImageUrl: string | null;
}

export interface TwitchState {
  /** Compte : déconnecté, en attente du navigateur, jeton en cours de contrôle, ou connecté. */
  status: "disconnected" | "authorizing" | "validating" | "connected";
  /** Flux de chat EventSub (n'a de sens que si `status === "connected"`). */
  chat: "off" | "connecting" | "live" | "reconnecting";
  account: TwitchAccount | null;
  /**
   * Chaîne dont le chat est affiché, une fois résolue auprès de Twitch. `null` tant que le compte n'est pas
   * connecté, ou si la chaîne demandée est introuvable. Peut être celle du compte connecté ou une autre.
   */
  channel: TwitchAccount | null;
  /** Choix de l'utilisateur (pseudo en minuscules). `null` = sa propre chaîne. Mémorisé entre deux lancements. */
  channelLogin: string | null;
  server: { running: boolean; port: typeof TWITCH_PORT; overlayUrl: string };
  /** Nombre de sources OBS (pages overlay) connectées en ce moment. */
  overlayClients: number;
  /** Fin de validité du jeton (ms epoch), d'après le dernier contrôle /validate. */
  tokenExpiresAt: number | null;
  /** Dernière erreur bloquante (effacée dès qu'une nouvelle connexion réussit). */
  error: TwitchError | null;
  /** Avertissement non bloquant (ex. session non mémorisable). */
  warning: TwitchError | null;
  /** Permissions des alertes accordées par la session en cours (reconnexion nécessaire sinon). */
  alertScopes: { follows: boolean; subscriptions: boolean };
}

export function initialTwitchState(): TwitchState {
  return {
    status: "disconnected",
    chat: "off",
    account: null,
    channel: null,
    channelLogin: null,
    server: { running: false, port: TWITCH_PORT, overlayUrl: TWITCH_OVERLAY_URL },
    overlayClients: 0,
    tokenExpiresAt: null,
    error: null,
    warning: null,
    alertScopes: { follows: false, subscriptions: false },
  };
}

// ---------------------------------------------------------------------------
// Choix de la chaîne : « pseudo » ou lien Twitch
// ---------------------------------------------------------------------------

const CHANNEL_LOGIN_RE = /^[a-z0-9_]{1,25}$/;
const TWITCH_URL_RE = /^(?:https?:\/\/)?(?:[a-z0-9-]+\.)?twitch\.tv(?:[/?#]|$)/i;
/** Préfixes de lien qui précèdent le pseudo : twitch.tv/popout/NOM/chat, twitch.tv/moderator/NOM… */
const CHANNEL_URL_PREFIXES = new Set(["popout", "moderator", "embed"]);
/** Pages de twitch.tv qui ne sont pas des chaînes. */
const NON_CHANNEL_PATHS = new Set([
  "directory", "videos", "settings", "downloads", "jobs", "turbo", "subscriptions", "wallet", "friends",
  "messages", "search", "store", "prime", "drops", "inventory", "following", "team", "p", "u", "login", "signup",
]);

/**
 * Extrait le pseudo (login) d'un pseudo saisi à la main ou d'un lien Twitch.
 * Accepte : `ninja`, `@Ninja`, `twitch.tv/ninja`, `https://www.twitch.tv/ninja/videos`,
 * `https://www.twitch.tv/popout/ninja/chat`, `https://m.twitch.tv/ninja`.
 * Renvoie le pseudo en minuscules, ou `null` si l'entrée n'est ni l'un ni l'autre.
 * Ce n'est qu'une vérification de forme : l'existence de la chaîne est contrôlée auprès de Twitch.
 */
export function parseTwitchChannel(input: string): string | null {
  const text = input.trim();
  if (!text || text.length > 300) return null;

  let candidate: string;
  if (TWITCH_URL_RE.test(text)) {
    let url: URL;
    try {
      url = new URL(/^https?:\/\//i.test(text) ? text : `https://${text}`);
    } catch {
      return null;
    }
    const segments = url.pathname.split("/").filter(Boolean);
    if (segments.length > 0 && CHANNEL_URL_PREFIXES.has(segments[0].toLowerCase())) segments.shift();
    if (segments.length === 0) return null;
    candidate = segments[0];
  } else {
    if (text.includes("/") || /\s/.test(text)) return null;
    candidate = text.replace(/^[@#]/, "");
  }

  const login = candidate.toLowerCase();
  if (NON_CHANNEL_PATHS.has(login) || !CHANNEL_LOGIN_RE.test(login)) return null;
  return login;
}

// ---------------------------------------------------------------------------
// Chat : modèle normalisé (ce que reçoit l'overlay, jamais le JSON brut de Twitch)
// ---------------------------------------------------------------------------

export interface ChatBadge {
  setId: string;
  id: string;
}

export type ChatFragment =
  | { type: "text" | "mention" | "cheermote"; text: string }
  | { type: "emote"; text: string; emoteId: string };

export interface ChatMessage {
  id: string;
  userId: string;
  login: string;
  displayName: string;
  /** `#rrggbb`, ou null si l'utilisateur n'a choisi aucune couleur. */
  color: string | null;
  badges: ChatBadge[];
  fragments: ChatFragment[];
  text: string;
  timestamp: number;
  /** Premier message du spectateur, message mis en avant (points de chaîne), etc. */
  highlight: "none" | "first" | "channel_points" | "cheer";
  /** Nom du destinataire si le message est une réponse. */
  replyTo: string | null;
  /** Plateforme d'origine ; absent = Twitch (messages d'avant la 0.3.9). */
  platform?: "twitch" | "tiktok";
}

/** Messages poussés par le serveur Twitch aux pages overlay. */
export type OverlayMessage =
  | { type: "init"; badges: Record<string, string>; backlog: ChatMessage[] }
  | { type: "badges"; badges: Record<string, string> }
  | { type: "chat"; message: ChatMessage }
  | { type: "delete"; messageId: string }
  | { type: "clear_user"; userId: string }
  | { type: "clear" }
  /** Demande aux pages overlay de se recharger (bouton « Redémarrer le lien OBS »). */
  | { type: "reload" };
