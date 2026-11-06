/**
 * Petites fonctions pures de l'interface (aucune dépendance React/DOM), pour
 * pouvoir les tester sans rendu. Testées dans tests/ui-helpers.test.ts.
 */

import { TWITCH_PORT } from "../../shared/twitch";

// ---------------------------------------------------------------------------
// Zoom de l'éditeur
// ---------------------------------------------------------------------------

/** Paliers de zoom proposés par l'éditeur (boutons +/-, raccourcis, liste). */
export const ZOOM_STEPS = [0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4] as const;

export const ZOOM_MIN = 0.1;
export const ZOOM_MAX = 4;

/** Borne un zoom quelconque (molette, mesure « ajusté »...) et le ramène à 2 décimales. */
export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom)) return 1;
  return Math.round(Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom)) * 100) / 100;
}

/**
 * Palier suivant (+1) ou précédent (-1) par rapport au zoom courant, même si
 * celui-ci n'est pas un palier (mode « ajusté » à 87 %, molette...) : depuis
 * 87 %, « zoomer » donne 100 % et « dézoomer » 75 %, jamais un saut absurde.
 */
export function stepZoom(current: number, direction: 1 | -1): number {
  const EPS = 1e-6;
  if (direction === 1) {
    const next = ZOOM_STEPS.find((z) => z > current + EPS);
    return next ?? ZOOM_STEPS[ZOOM_STEPS.length - 1];
  }
  const previous = [...ZOOM_STEPS].reverse().find((z) => z < current - EPS);
  return previous ?? ZOOM_STEPS[0];
}

// ---------------------------------------------------------------------------
// Réglages
// ---------------------------------------------------------------------------

export type PortValidation = { ok: true; port: number } | { ok: false; reason: string };

/**
 * Valide un port saisi à la main. Les ports < 1024 sont réservés au système :
 * un utilisateur qui tape « 80 » obtiendrait un serveur qui ne démarre pas.
 */
export function validatePort(raw: string | number): PortValidation {
  const text = String(raw).trim();
  if (!/^\d+$/.test(text)) return { ok: false, reason: "Le port doit être un nombre entier." };
  const port = Number(text);
  if (port < 1024 || port > 65535) return { ok: false, reason: "Choisissez un port entre 1024 et 65535." };
  // Port fixe du serveur Twitch : l'overlay musical ne peut pas l'utiliser.
  if (port === TWITCH_PORT) return { ok: false, reason: `Le port ${TWITCH_PORT} est réservé au chat Twitch.` };
  return { ok: true, port };
}

// ---------------------------------------------------------------------------
// Overlays
// ---------------------------------------------------------------------------

/**
 * Overlay « principal » : celui dont l'URL OBS est proposée partout (barre
 * latérale, tableau de bord). Si l'overlay marqué n'existe plus (supprimé,
 * import...), on retombe sur le premier plutôt que de n'en proposer aucun.
 */
export function pickMainOverlay<T extends { id: string }>(overlays: T[], activeId: string | null): T | null {
  return overlays.find((o) => o.id === activeId) ?? overlays[0] ?? null;
}

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** « il y a 5 min », « hier »... Chaîne vide pour un horodatage absent (miniatures de preset). */
export function formatRelativeDate(timestamp: number, now: number = Date.now()): string {
  if (!(timestamp > 0)) return "";
  const diff = Math.max(0, now - timestamp);
  if (diff < MINUTE) return "à l'instant";
  if (diff < HOUR) return `il y a ${Math.floor(diff / MINUTE)} min`;
  if (diff < DAY) return `il y a ${Math.floor(diff / HOUR)} h`;
  if (diff < 2 * DAY) return "hier";
  if (diff < 30 * DAY) return `il y a ${Math.floor(diff / DAY)} j`;
  return new Date(timestamp).toLocaleDateString("fr-FR");
}

// ---------------------------------------------------------------------------
// Menus déroulants
// ---------------------------------------------------------------------------

/** Hauteur maximale (px) de la liste d'un menu déroulant : au-delà, la liste défile au lieu de s'allonger. */
export const SELECT_MENU_MAX_HEIGHT = 224;

export interface MenuPlacement {
  openUp: boolean;
  maxHeight: number;
}

/**
 * Où ouvrir la liste d'un menu déroulant par rapport à son bouton : en dessous
 * par défaut, au-dessus seulement s'il n'y a pas la place dessous et davantage
 * dessus. La hauteur est plafonnée à `SELECT_MENU_MAX_HEIGHT` et réduite si
 * l'espace disponible est plus petit, pour ne jamais sortir de la fenêtre.
 */
export function computeMenuPlacement(
  triggerTop: number,
  triggerBottom: number,
  viewportHeight: number,
  maxHeight: number = SELECT_MENU_MAX_HEIGHT,
  margin = 8
): MenuPlacement {
  const below = viewportHeight - triggerBottom - margin;
  const above = triggerTop - margin;
  const openUp = below < maxHeight && above > below;
  const space = openUp ? above : below;
  // Plancher : une liste écrasée à quelques pixels serait inutilisable.
  return { openUp, maxHeight: Math.max(80, Math.min(maxHeight, Math.floor(space))) };
}
