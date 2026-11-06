/**
 * Live TikTok : types de l'état envoyé par le cœur de Batlay
 * (crates/batlay-core/src/tiktok, en Rust) et petits utilitaires de
 * l'interface. Connexion, décodage du flux et alertes sont dans le cœur.
 */

// ---------------------------------------------------------------------------
// Réglages
// ---------------------------------------------------------------------------

export interface TikTokLiveSettings {
  /** Pseudo TikTok (sans @), ou `null`. */
  username: string | null;
  /** Vrai tant que l'utilisateur n'a pas cliqué « Déconnecter » : Batlay attend le live et s'y reconnecte. */
  enabled: boolean;
  /** Cadeaux : valeur minimale (en diamants, quantité comprise) pour afficher une alerte. 1 = tous. */
  giftMinDiamonds: number;
  /** Moyen de connexion au live, au choix de l'utilisateur (voir `TIKTOK_MODES`). */
  mode: TikTokMode;
}

/**
 * - `browser` (défaut) : la page du live est ouverte dans une fenêtre invisible
 *   de Batlay et son flux est lu tel que TikTok l'envoie, sans service tiers ;
 * - `euler` : le service tiers Euler Stream signe la connexion, puis Batlay lit
 *   le flux lui-même (comptes publics seulement : la session n'est jamais envoyée) ;
 * - `local` : Batlay ne se connecte pas ; un autre outil (TikFinity,
 *   Streamer.bot…) envoie les événements à l'adresse locale des alertes.
 */
export type TikTokMode = "browser" | "euler" | "local";

export const TIKTOK_MODES: readonly { value: TikTokMode; label: string; hint: string }[] = [
  { value: "browser", label: "Navigateur intégré", hint: "Recommandé. Sans service tiers, marche aussi avec un compte privé." },
  { value: "euler", label: "Euler Stream", hint: "Service tiers de signature. Comptes publics seulement, aucune fenêtre ouverte." },
  { value: "local", label: "Adresse locale", hint: "Pour TikFinity, Streamer.bot… : votre outil envoie les événements à Batlay." },
];

/** Méthode reçue du cœur (ou d'une ancienne version) → une des trois. `library` = Euler en 0.4.x. */
export function normalizeTikTokMode(mode: unknown): TikTokMode {
  if (mode === "euler" || mode === "library") return "euler";
  return mode === "local" ? "local" : "browser";
}

export const DEFAULT_TIKTOK_SETTINGS: TikTokLiveSettings = { username: null, enabled: false, giftMinDiamonds: 1, mode: "browser" };

/** Pseudo TikTok : lettres, chiffres, « _ » et « . », 2 à 24 caractères. */
const USERNAME_RE = /^[A-Za-z0-9_.]{2,24}$/;

/**
 * Accepte `pseudo`, `@pseudo` ou une adresse `https://www.tiktok.com/@pseudo/live`.
 * Renvoie le pseudo en minuscules, ou `null` s'il n'est pas valable.
 */
export function parseTikTokUsername(input: unknown): string | null {
  if (typeof input !== "string") return null;
  let v = input.trim();
  const fromUrl = /^(?:https?:\/\/)?(?:www\.|m\.)?tiktok\.com\/@([^/?#\s]+)/i.exec(v);
  if (fromUrl) v = fromUrl[1];
  v = v.replace(/^@/, "");
  return USERNAME_RE.test(v) ? v.toLowerCase() : null;
}

// ---------------------------------------------------------------------------
// État affiché dans le tableau de bord
// ---------------------------------------------------------------------------

export type TikTokLiveStatus =
  /** Déconnecté (aucun pseudo, ou « Déconnecter »). */
  | "idle"
  | "connecting"
  /** Branché sur le live : les événements arrivent. */
  | "live"
  /** Pas en live (ou service de signature saturé) : nouvel essai programmé. */
  | "waiting"
  /** En live, mais le live est caché aux visiteurs non connectés (compte privé) : nouvel essai programmé. */
  | "hidden"
  /** Échec sans nouvel essai automatique (compte inexistant, module absent). */
  | "error";

export interface TikTokLiveState {
  status: TikTokLiveStatus;
  settings: TikTokLiveSettings;
  /** Message pour l'utilisateur (raison de l'attente ou de l'erreur). */
  message: string | null;
  /** Texte brut de la dernière erreur de la bibliothèque (« Détails techniques »), ou `null`. */
  detail: string | null;
  /** Prochain essai (horodatage ms), en attente. */
  retryAt: number | null;
  /** Depuis quand le live est suivi. */
  since: number | null;
  /** Compte vérifié (même hors live), ou `null` si pas encore vérifié / vérification impossible. */
  account: TikTokAccount | null;
  /** Compte TikTok connecté dans Batlay (cookie de session présent dans la partition TikTok). */
  loggedIn: boolean;
  /** Clé API d'Euler Stream enregistrée (chiffrée par Windows). Sa valeur ne quitte jamais le cœur. */
  eulerKey: boolean;
}

export interface TikTokAccount {
  username: string;
  /** Nom affiché du profil. */
  displayName: string;
}

export function initialTikTokState(settings: TikTokLiveSettings = DEFAULT_TIKTOK_SETTINGS): TikTokLiveState {
  return { status: "idle", settings, message: null, detail: null, retryAt: null, since: null, account: null, loggedIn: false, eulerKey: false };
}

// ---------------------------------------------------------------------------
// Diagnostic (bouton « Diagnostic » de la carte TikTok)
// ---------------------------------------------------------------------------

export interface TikTokDiagnosticStep {
  label: string;
  status: "ok" | "warn" | "fail" | "skip";
  detail: string;
}

export interface TikTokDiagnostic {
  at: number;
  steps: TikTokDiagnosticStep[];
}
