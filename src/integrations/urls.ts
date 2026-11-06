import { isLoopbackHttpUrl, type LoopbackHostname } from "../../shared/loopback";

/**
 * URL d'overlays pour les logiciels de streaming (fonctions pures, testées dans
 * tests/integrations.test.ts).
 *
 * Aucune nouvelle URL n'est inventée : on part de l'URL existante (fournie par
 * le cœur de Batlay pour la musique, par l'état Twitch pour le chat) et on
 * n'y change que deux choses, toutes deux facultatives :
 *   - l'hôte, `localhost` ↔ `127.0.0.1` (même serveur, même port) ;
 *   - le paramètre d'affichage `fit=contain` (overlay musical uniquement).
 * Les anciennes URL restent donc valables telles quelles, et un même overlay
 * sert dans tous les logiciels.
 */

export type FitMode = "none" | "contain";

export interface OverlayDisplayParams {
  /**
   * `none` (défaut, comportement historique) : canvas à sa taille native, en haut à gauche.
   * `contain` : canvas réduit/agrandi pour tenir entièrement dans la source, proportions gardées.
   */
  fit: FitMode;
}

/** Lit les paramètres d'affichage de la page overlay. Toute valeur inconnue = comportement historique. */
export function parseOverlayDisplayParams(search: string): OverlayDisplayParams {
  let params: URLSearchParams;
  try {
    params = new URLSearchParams(search);
  } catch {
    return { fit: "none" };
  }
  const raw = params.get("fit")?.trim().toLowerCase();
  return { fit: raw === "contain" || raw === "1" || raw === "true" ? "contain" : "none" };
}

/** Bornes du facteur d'échelle : une source à 0×0 (bug signalé dans certains logiciels) ne doit pas produire 0 ni l'infini. */
const MIN_FIT_SCALE = 0.05;
const MAX_FIT_SCALE = 8;

/** Facteur qui fait tenir le canvas dans la fenêtre de la source. 1 si une des tailles est inexploitable. */
export function computeFitScale(viewportWidth: number, viewportHeight: number, canvasWidth: number, canvasHeight: number): number {
  const values = [viewportWidth, viewportHeight, canvasWidth, canvasHeight];
  if (!values.every((v) => Number.isFinite(v) && v > 0)) return 1;
  const scale = Math.min(viewportWidth / canvasWidth, viewportHeight / canvasHeight);
  return Math.min(MAX_FIT_SCALE, Math.max(MIN_FIT_SCALE, scale));
}

export interface UrlVariantOptions {
  /** Hôte de l'URL. Absent = celui de l'URL d'origine. */
  host?: LoopbackHostname;
  /** Ajoute (`true`) ou retire (`false`) `fit=contain`. Absent = inchangé. */
  fit?: boolean;
}

/**
 * Variante d'une URL d'overlay locale. Renvoie `null` si l'URL de départ n'est
 * pas une adresse http locale de Batlay : l'interface ne copie alors rien,
 * plutôt qu'une adresse qu'elle ne maîtrise pas.
 * Les autres paramètres (réglages du chat) sont conservés, dans le même ordre.
 */
export function buildOverlayUrlVariant(baseUrl: string, options: UrlVariantOptions = {}): string | null {
  if (!isLoopbackHttpUrl(baseUrl)) return null;
  const url = new URL(baseUrl);
  if (options.host) url.hostname = options.host;
  if (options.fit === true) url.searchParams.set("fit", "contain");
  if (options.fit === false) url.searchParams.delete("fit");
  // URLSearchParams laisse un « ? » vide quand tous les paramètres sont retirés.
  return url.search === "?" ? url.toString().replace(/\?$/, "") : url.toString();
}

/** Identifiants d'overlay acceptés (nanoid : lettres, chiffres, « _ » et « - »). */
const OVERLAY_ID_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;

export function isValidOverlayId(id: unknown): id is string {
  return typeof id === "string" && OVERLAY_ID_PATTERN.test(id);
}

export type IdentifiedUrl =
  | { kind: "music"; overlayId: string; overlayName: string | null; host: string; fit: FitMode }
  | { kind: "chat"; host: string; search: string }
  | { kind: "invalid"; reason: string };

export interface IdentifyContext {
  musicPort: number;
  chatPort: number;
  chatPath: string;
  overlays: readonly { id: string; name: string }[];
}

/**
 * « À quel overlay correspond cette URL ? » — l'utilisateur colle l'adresse
 * trouvée dans son logiciel de streaming. `overlayName: null` = lien révoqué
 * (régénéré) ou overlay supprimé : la source n'affichera plus rien.
 */
export function identifyOverlayUrl(raw: string, ctx: IdentifyContext): IdentifiedUrl {
  const text = raw.trim();
  if (text === "") return { kind: "invalid", reason: "Collez une URL d'overlay." };
  if (!isLoopbackHttpUrl(text)) {
    return {
      kind: "invalid",
      reason: "Ce n'est pas une URL locale de Batlay (elle doit commencer par http://localhost: ou http://127.0.0.1:).",
    };
  }
  const url = new URL(text);
  const port = Number(url.port);
  if (port === ctx.chatPort) {
    if (url.pathname.replace(/\/+$/, "") !== ctx.chatPath) {
      return { kind: "invalid", reason: `Le port ${ctx.chatPort} est celui du chat, mais le chemin attendu est ${ctx.chatPath}.` };
    }
    return { kind: "chat", host: url.hostname, search: url.search };
  }
  if (port !== ctx.musicPort) {
    return {
      kind: "invalid",
      reason: `Port ${port} inconnu : le serveur d'overlay musical de Batlay utilise le port ${ctx.musicPort} (Paramètres), le chat le port ${ctx.chatPort}.`,
    };
  }
  const match = url.pathname.match(/^\/overlay\/([^/]+)\/?$/);
  let id: string | null = null;
  if (match) {
    try {
      id = decodeURIComponent(match[1]);
    } catch {
      id = null; // séquence %XX invalide
    }
  }
  if (!isValidOverlayId(id)) {
    return { kind: "invalid", reason: "Chemin inattendu : une URL d'overlay musical a la forme /overlay/<identifiant>." };
  }
  const overlay = ctx.overlays.find((o) => o.id === id);
  return {
    kind: "music",
    overlayId: id,
    overlayName: overlay ? overlay.name : null,
    host: url.hostname,
    fit: parseOverlayDisplayParams(url.search).fit,
  };
}
