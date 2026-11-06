import type { TwitchState } from "../../shared/twitch";

/**
 * Textes d'état de la page Twitch, sans React (testés dans tests/twitch-ui.test.ts).
 */

/** « encore 58 jours », « encore 5 h »... à partir de la fin de validité annoncée par Twitch. */
export function describeTokenExpiry(expiresAt: number | null, now: number): string {
  if (expiresAt === null) return "Twitch n'annonce aucune date d'expiration pour cette session.";
  const ms = expiresAt - now;
  if (ms <= 0) return "La session a expiré : reconnectez votre compte.";
  const minutes = Math.floor(ms / 60_000);
  const hours = Math.floor(minutes / 60);
  const days = Math.floor(hours / 24);
  if (days >= 1) return `Session valide encore ${days} jour${days > 1 ? "s" : ""}.`;
  if (hours >= 1) return `Session valide encore ${hours} h.`;
  return `Session valide encore ${Math.max(1, minutes)} min.`;
}

export type StatusTone = "ok" | "warn" | "neutral";

export function accountStatus(state: Pick<TwitchState, "status">): { label: string; tone: StatusTone; busy: boolean } {
  switch (state.status) {
    case "connected":
      return { label: "Connecté", tone: "ok", busy: false };
    case "authorizing":
      return { label: "En attente de votre autorisation dans le navigateur…", tone: "warn", busy: true };
    case "validating":
      return { label: "Vérification de la session…", tone: "warn", busy: true };
    case "disconnected":
      return { label: "Non connecté", tone: "neutral", busy: false };
  }
}

export function chatStatus(state: Pick<TwitchState, "status" | "chat">): { label: string; tone: StatusTone } {
  if (state.status !== "connected") return { label: "Chat inactif", tone: "neutral" };
  switch (state.chat) {
    case "live":
      return { label: "Chat en direct", tone: "ok" };
    case "connecting":
      return { label: "Connexion au chat…", tone: "warn" };
    case "reconnecting":
      return { label: "Reconnexion au chat…", tone: "warn" };
    case "off":
      return { label: "Chat inactif", tone: "neutral" };
  }
}

/**
 * Nom de la chaîne dont le chat est affiché, pour les cartes et pastilles.
 * Chaîne résolue si on l'a, sinon le pseudo demandé, sinon « votre chaîne ».
 */
export function channelLabel(state: Pick<TwitchState, "channel" | "channelLogin" | "account">): string {
  if (state.channel) return state.channel.displayName;
  if (state.channelLogin) return state.channelLogin;
  return state.account ? state.account.displayName : "Votre chaîne";
}

/** Vrai si le chat affiché est celui d'une autre chaîne que celle du compte connecté. */
export function isOtherChannel(state: Pick<TwitchState, "channel" | "account">): boolean {
  return !!state.channel && !!state.account && state.channel.id !== state.account.id;
}
