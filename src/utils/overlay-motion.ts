import type { OverlayComponentConfig } from "@/types/overlay";
import type { Track } from "@/types/track";
import { formatArtists } from "@/utils/artists";
import { formatTime } from "@/utils/format-time";
import { renderTemplate } from "@/utils/overlay-style";

/**
 * Identité du contenu affiché par un composant (fonction PURE, testée dans
 * tests/overlay-motion.test.ts). Une nouvelle identité déclenche la transition
 * du composant ; la même identité avec des données plus récentes met à jour
 * l'affichage SANS animation (src/animation/swap.ts).
 *
 * Chaque composant a SA clé : quand seule la pochette arrive (Lecture
 * système : titre d'abord, image ensuite), seule la pochette s'anime ; un
 * changement de morceau anime le titre, l'artiste, la pochette… mais pas un
 * fond décoratif ni la barre de progression, qui restent en place.
 */
export const NO_TRACK_KEY = "∅";

export function componentContentKey(
  component: Pick<OverlayComponentConfig, "type" | "content" | "style" | "mediaId">,
  track: Track | null,
  /** Variables du live : un nouveau « dernier follower » rejoue la transition du texte qui l'affiche. */
  live?: Partial<Record<string, string>>
): string {
  if (!track) return NO_TRACK_KEY;
  switch (component.type) {
    // Image : nouvelle image = nouvelle transition. Horloge : l'heure qui passe ne rejoue pas l'entrée.
    case "image":
      return `image:${component.mediaId ?? ""}`;
    case "clock":
      return `clock:${component.style.clockFormat ?? "time"}`;
    // Compte à rebours : une nouvelle durée le relance ; Objectif : la barre progresse sans rejouer l'entrée.
    case "countdown":
      return `countdown:${component.style.countdownSeconds ?? 300}`;
    case "goal":
      return `goal:${component.style.goalMetric ?? "followers"}`;
    case "artwork":
      return `artwork:${track.artwork ?? ""}`;
    case "title":
      return `title:${track.title}`;
    case "artist":
      return `artist:${formatArtists(track, component.style.artistSeparator)}`;
    case "album":
      return `album:${track.album ?? ""}`;
    case "source":
      return `source:${track.source ?? ""}`;
    case "duration":
      return `duration:${formatTime(track.duration)}`;
    case "text":
      return `text:${renderTemplate(component.content ?? "", {
        title: track.title,
        artist: formatArtists(track, component.style.artistSeparator),
        album: track.album ?? "",
        source: track.source ?? "",
        // Spectateurs et likes changent sans arrêt : ils mettent le texte à jour sans rejouer la transition.
      }, live ? { ...live, viewers: "", likes: "" } : live)}`;
    // Barre, temps écoulé / restant, formes : continus, ils ne « changent » pas d'un morceau à l'autre.
    case "progressBar":
    case "elapsedTime":
    case "remainingTime":
    case "shape":
    default:
      return "present";
  }
}
