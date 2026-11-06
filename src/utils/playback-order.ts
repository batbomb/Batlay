import type { PlaybackState } from "@/types/track";

/**
 * Garde-fou contre les mises à jour tardives (fonction PURE, testée dans
 * tests/overlay-motion.test.ts). `updatedAt` est l'instant de la MESURE, pris
 * sur l'horloge de Batlay (voir docs §6ter) : un état mesuré AVANT celui déjà
 * affiché ne doit jamais le remplacer — sinon l'ancien morceau réapparaîtrait
 * une fraction de seconde, avec sa transition.
 *
 * Un écart de plus de STALE_WINDOW_MS n'est pas une mise à jour tardive mais
 * un saut d'horloge (resynchronisation Windows, Batlay relancé) : l'état est
 * alors accepté, l'overlay ne doit jamais rester figé.
 */
export const STALE_WINDOW_MS = 10_000;

export function isStalePlayback(previous: Pick<PlaybackState, "updatedAt"> | null, next: Pick<PlaybackState, "updatedAt">): boolean {
  if (!previous) return false;
  const a = previous.updatedAt;
  const b = next.updatedAt;
  if (typeof a !== "number" || typeof b !== "number" || !Number.isFinite(a) || !Number.isFinite(b)) return false;
  return b < a && a - b <= STALE_WINDOW_MS;
}
