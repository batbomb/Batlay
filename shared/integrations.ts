/**
 * Logiciels de streaming pris en compte par la page « Intégrations ».
 *
 * Le tableau de bord ne transmet jamais d'URL à ouvrir, seulement un
 * identifiant de logiciel ; le cœur de Batlay lit l'adresse de la
 * documentation dans sa propre liste fermée
 * (crates/batlay-core/src/integrations.rs). Une page compromise ne peut donc
 * pas faire ouvrir une adresse arbitraire.
 */

export const PLATFORM_IDS = ["obs", "tiktok-live-studio", "streamlabs", "xsplit", "other"] as const;
export type PlatformId = (typeof PLATFORM_IDS)[number];

export interface PlatformDocs {
  url: string;
  /** Libellé du bouton. */
  label: string;
  /**
   * `vendor` : page publiée par l'éditeur du logiciel et consultée lors de la
   * rédaction. `vendor-download` : page officielle de l'éditeur, mais qui ne
   * documente pas la méthode d'intégration (l'éditeur n'en publie pas).
   */
  kind: "vendor" | "vendor-download";
}

/**
 * Adresses officielles relevées en octobre 2026. Aucune n'est ajoutée sans
 * provenir du site de l'éditeur ; « Autre logiciel » n'en a pas, par définition.
 */
export const PLATFORM_DOCS: Record<PlatformId, PlatformDocs | null> = {
  obs: {
    url: "https://obsproject.com/kb/browser-source",
    label: "Documentation OBS : Source navigateur",
    kind: "vendor",
  },
  "tiktok-live-studio": {
    // TikTok ne publie pas de documentation publique de la source « Link » :
    // seule la page officielle de téléchargement est proposée.
    url: "https://www.tiktok.com/studio/download",
    label: "Page officielle TikTok LIVE Studio",
    kind: "vendor-download",
  },
  streamlabs: {
    url: "https://streamlabs.com/content-hub/post/introducing-browser-source-interaction-for-streamlabs-desktop",
    label: "Streamlabs : Browser Source",
    kind: "vendor",
  },
  xsplit: {
    url: "https://support.xsplit.com/en/article/webpage-1y1l6v",
    label: "Support XSplit : source Webpage",
    kind: "vendor",
  },
  other: null,
};

export function isPlatformId(value: unknown): value is PlatformId {
  return typeof value === "string" && (PLATFORM_IDS as readonly string[]).includes(value);
}
