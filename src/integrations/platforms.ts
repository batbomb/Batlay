import { PLATFORM_DOCS, PLATFORM_IDS, isPlatformId, type PlatformDocs, type PlatformId } from "../../shared/integrations";
import type { CanvasFormatId } from "./formats";
import { HOSTS_FILE_LINE, LOCAL_ALIAS_HOSTNAME, type LoopbackHostname } from "../../shared/loopback";

/**
 * Registre des logiciels de streaming (données pures, testées dans
 * tests/integrations.test.ts). C'est la SEULE source des textes de la page
 * Intégrations et de docs/INTEGRATIONS.md : les deux doivent dire la même chose.
 *
 * Règle de rédaction : un statut ne dit jamais plus que ce qui a été vérifié.
 *   - reference  : méthode pour laquelle Batlay est conçu (OBS, Source navigateur) ;
 *   - documented : méthode décrite par l'éditeur du logiciel et techniquement
 *                  identique (source Web basée sur Chromium) ;
 *   - partial    : méthode qui existe mais dont des points clés ne sont pas
 *                  documentés par l'éditeur (accès à localhost, transparence…) ;
 *   - unverified : rien n'a été vérifié pour ce logiciel.
 * Aucun logiciel n'a été lancé avec Batlay pendant le développement (voir
 * TESTING_NOTE) : c'est le compteur « sources connectées » qui le prouve chez
 * l'utilisateur.
 */

export type { PlatformId } from "../../shared/integrations";

export type CompatibilityStatus = "reference" | "documented" | "partial" | "unverified";

export const STATUS_LABELS: Record<CompatibilityStatus, string> = {
  reference: "Méthode de référence",
  documented: "Méthode documentée",
  partial: "Compatibilité partielle — à vérifier",
  unverified: "Non vérifiée",
};

/** Ton de pastille associé (mêmes tons que StatusPill). */
export const STATUS_TONES: Record<CompatibilityStatus, "ok" | "warn" | "neutral"> = {
  reference: "ok",
  documented: "ok",
  partial: "warn",
  unverified: "neutral",
};

export type Certainty = "verified" | "expected" | "unknown";

export interface Capability {
  certainty: Certainty;
  detail: string;
}

export interface Alternative {
  title: string;
  steps: string[];
  caveats: string[];
}

export interface IntegrationPlatform {
  id: PlatformId;
  name: string;
  status: CompatibilityStatus;
  /** Une phrase qui justifie le statut. */
  statusDetail: string;
  /** Nom de la source dans le logiciel. */
  sourceName: string;
  /** Méthode d'intégration, en une ligne. */
  method: string;
  /** Étapes de configuration, communes aux overlays Musique et Chat. */
  steps: string[];
  /** Où saisir la taille recommandée. */
  sizeField: string;
  liveUpdates: Capability;
  transparency: Capability;
  localAccess: Capability;
  limitations: string[];
  alternatives: Alternative[];
  defaultFormat: CanvasFormatId;
  /**
   * Vrai quand le logiciel ne permet pas toujours de fixer la taille de la
   * fenêtre de la source : on y conseille l'option « Ajuster à la source ».
   */
  recommendFit: boolean;
  /**
   * Hôte imposé pour ce logiciel, ou `null` : celui que donne Batlay (batlay.test
   * s'il pointe vers ce PC, sinon localhost). TikTok impose batlay.test : son champ
   * « Lien » refuse localhost et 127.0.0.1.
   */
  preferredHost: LoopbackHostname | null;
  docs: PlatformDocs | null;
}

export const TESTING_NOTE =
  "Aucun de ces logiciels n'a été lancé avec Batlay pendant le développement : les overlays ont été validés dans Chromium, " +
  "le moteur des sources Web d'OBS et de Streamlabs. Le compteur « sources connectées » ci-dessous vous permet de vérifier, " +
  "chez vous, que le logiciel charge bien la page et reçoit les mises à jour.";

const COMMON_LIVE_DETAIL =
  "Les changements enregistrés dans l'éditeur et l'état de lecture arrivent par WebSocket, sans recharger la source.";

const PLATFORMS: readonly IntegrationPlatform[] = [
  {
    id: "obs",
    name: "OBS Studio",
    status: "reference",
    statusDetail: "Batlay est conçu pour la Source navigateur d'OBS (moteur Chromium intégré) : c'est la méthode historique, inchangée.",
    sourceName: "Navigateur (Browser Source)",
    method: "Source navigateur avec l'URL de l'overlay",
    steps: [
      "Dans Sources, cliquez sur « + » puis « Navigateur ».",
      "Collez l'URL copiée depuis Batlay (décochez « Fichier local »).",
      "Saisissez la largeur et la hauteur indiquées par Batlay.",
      "Laissez le CSS personnalisé par défaut : il rend le fond transparent.",
      "Validez, puis placez la source dans la scène (réduisez-la si Batlay le conseille pour votre format).",
    ],
    sizeField: "Largeur / Hauteur de la source",
    liveUpdates: { certainty: "verified", detail: `${COMMON_LIVE_DETAIL} Vérifié dans Chromium.` },
    transparency: { certainty: "verified", detail: "Fond transparent par défaut (CSS de la source + page overlay transparente)." },
    localAccess: { certainty: "verified", detail: "La Source navigateur charge http://localhost sans réglage particulier." },
    limitations: [
      "Polices : OBS n'utilise que celles installées sur le PC qui diffuse.",
      "« Actualiser le cache de la page actuelle » recharge la source si elle semble figée.",
    ],
    alternatives: [],
    defaultFormat: "landscape",
    recommendFit: false,
    preferredHost: null,
    docs: PLATFORM_DOCS.obs,
  },
  {
    id: "tiktok-live-studio",
    name: "TikTok LIVE Studio",
    status: "partial",
    statusDetail:
      "LIVE Studio propose une source « Link » qui affiche une page Web, mais TikTok ne la documente pas publiquement. " +
      `Son champ « Lien » refuse les adresses localhost et 127.0.0.1 (« Saisis l'URL correcte ») : Batlay utilise donc le nom local ${LOCAL_ALIAS_HOSTNAME}, ` +
      "qu'il configure lui-même (une autorisation Windows, une seule fois). Le chargement réel de la page, la transparence et la connexion temps réel restent à confirmer " +
      "avec le compteur « sources connectées ».",
    sourceName: "Link (selon les versions : « Lien »)",
    method: "Source « Link » avec l'URL de l'overlay — ou, en secours, caméra virtuelle d'OBS",
    steps: [
      `Une seule fois : acceptez la demande de Windows au premier lancement de Batlay, ou cliquez sur « Configurer automatiquement » ci-dessous. Batlay ajoute alors la ligne « ${HOSTS_FILE_LINE} » au fichier hosts.`,
      "Ouvrez TikTok LIVE Studio sur Windows et choisissez l'orientation du live (portrait conseillé).",
      "Dans la scène, cliquez sur « + Ajouter une source » puis choisissez « Lien » (ou « Link »).",
      `Collez l'URL copiée depuis Batlay (elle commence par http://${LOCAL_ALIAS_HOSTNAME}:) et validez.`,
      "Si une résolution personnalisée est proposée, saisissez la taille indiquée par Batlay. Sinon, ou si l'overlay apparaît coupé, activez « Ajuster à la source » dans Batlay et recollez l'URL.",
      "Placez et redimensionnez la source dans l'aperçu. En portrait, évitez le tiers inférieur et le bord droit, recouverts par l'interface de TikTok (commentaires, cadeaux).",
      "Revenez dans Batlay → Intégrations : « Sources connectées » doit passer à 1 ou plus. Sinon, utilisez la méthode de secours (caméra virtuelle d'OBS).",
    ],
    sizeField: "Résolution personnalisée de la source Link (si proposée)",
    liveUpdates: {
      certainty: "unknown",
      detail: `${COMMON_LIVE_DETAIL} Le navigateur de la source Link devrait le permettre, mais ce n'est pas documenté : le compteur de sources connectées le confirme.`,
    },
    transparency: {
      certainty: "unknown",
      detail:
        "La page de Batlay est transparente, mais les guides tiers se contredisent sur le rendu de la source Link. " +
        "Si un fond apparaît, donnez à l'overlay son propre fond (carte) dans l'éditeur, ou passez par la caméra virtuelle d'OBS.",
    },
    localAccess: {
      certainty: "unknown",
      detail:
        `Le champ « Lien » refuse localhost et 127.0.0.1 mais accepte http://${LOCAL_ALIAS_HOSTNAME}:3000 (vérifié sur LIVE Studio, octobre 2026). ` +
        `${LOCAL_ALIAS_HOSTNAME} est un nom réservé qui ne pointe vers ce PC que grâce au fichier hosts : rien n'est exposé sur Internet.`,
    },
    limitations: [
      "Procédure relevée dans des guides tiers (octobre 2026) : TikTok peut renommer ou déplacer la source « Link ».",
      "L'accès à TikTok LIVE Studio et à la diffusion depuis un PC dépend de l'éligibilité de votre compte, fixée par TikTok.",
      "Des éditeurs tiers signalent des sources Link qui s'affichent vides (taille 0 × 0) : définissez la résolution, retirez puis rajoutez la source, ou utilisez la méthode de secours.",
      "Chaque source Link lance son propre navigateur : limitez leur nombre (un overlay Musique + un Chat suffisent).",
      "Overlay Chat : ses réglages sont dans l'URL. Après un changement dans Overlays → Chat, modifiez l'URL de la source Link.",
    ],
    alternatives: [
      {
        title: "Caméra virtuelle d'OBS (méthode de secours)",
        steps: [
          "Dans OBS : Paramètres → Vidéo, résolution de base et de sortie 1080 × 1920 (portrait).",
          "Ajoutez vos overlays Batlay comme sources « Navigateur » (méthode OBS ci-dessus), avec votre caméra et vos autres sources.",
          "Cliquez sur « Démarrer la caméra virtuelle ».",
          "Dans TikTok LIVE Studio, ajoutez une source « Caméra » (pas « Link ») et choisissez « OBS Virtual Camera ». Faites-lui occuper toute la scène.",
        ],
        caveats: [
          "La caméra virtuelle transmet une image opaque : composez toute la scène dans OBS, rien ne doit se trouver derrière elle dans LIVE Studio.",
          "Démarrez la caméra virtuelle avant d'ouvrir LIVE Studio (sinon, redémarrez LIVE Studio).",
          "Des guides tiers indiquent que TikTok peut verrouiller les caméras virtuelles sur les comptes récents.",
        ],
      },
      {
        title: "Diffuser depuis OBS avec une clé de stream TikTok",
        steps: [
          "Si TikTok vous fournit une clé de stream (accès décidé par TikTok, il n'existe aucun moyen légitime de l'obtenir autrement), diffusez directement depuis OBS.",
          "Les overlays s'ajoutent alors exactement comme dans la méthode OBS.",
        ],
        caveats: ["Batlay ne fournit, ne génère ni ne contourne aucune clé de stream."],
      },
    ],
    defaultFormat: "portrait",
    recommendFit: true,
    preferredHost: LOCAL_ALIAS_HOSTNAME,
    docs: PLATFORM_DOCS["tiktok-live-studio"],
  },
  {
    id: "streamlabs",
    name: "Streamlabs Desktop",
    status: "documented",
    statusDetail: "Streamlabs Desktop est bâti sur OBS : sa « Browser Source » utilise le même moteur que celui d'OBS.",
    sourceName: "Browser Source",
    method: "Browser Source avec l'URL de l'overlay",
    steps: [
      "Dans l'onglet Studio, section Sources, cliquez sur « + ».",
      "Choisissez « Browser Source », nommez-la (ex. « Batlay Musique ») puis « Add Source ».",
      "Collez l'URL copiée depuis Batlay dans le champ URL.",
      "Saisissez la largeur (Width) et la hauteur (Height) indiquées par Batlay, puis « Done ».",
    ],
    sizeField: "Width / Height de la Browser Source",
    liveUpdates: { certainty: "expected", detail: `${COMMON_LIVE_DETAIL} Même moteur qu'OBS.` },
    transparency: { certainty: "expected", detail: "Fond transparent par défaut, comme dans OBS." },
    localAccess: { certainty: "expected", detail: "Même comportement qu'OBS pour http://localhost." },
    limitations: ["Les widgets Streamlabs et les overlays Batlay cohabitent : ce sont des sources indépendantes."],
    alternatives: [],
    defaultFormat: "landscape",
    recommendFit: false,
    preferredHost: null,
    docs: PLATFORM_DOCS.streamlabs,
  },
  {
    id: "xsplit",
    name: "XSplit Broadcaster",
    status: "documented",
    statusDetail: "XSplit documente une source « Webpage » qui affiche n'importe quelle URL, avec transparence et barres de défilement réglables.",
    sourceName: "Webpage",
    method: "Source Webpage avec l'URL de l'overlay",
    steps: [
      "Cliquez sur « Add Source » puis « Webpage ».",
      "Collez l'URL copiée depuis Batlay et cliquez sur « OK ».",
      "Ouvrez les propriétés de la source, section Display : réglez la zone d'affichage (screen area) sur la taille indiquée par Batlay, gardez la transparence et masquez les barres de défilement.",
      "Placez et redimensionnez la source dans la scène.",
    ],
    sizeField: "Propriétés → Display (zone d'affichage)",
    liveUpdates: { certainty: "expected", detail: `${COMMON_LIVE_DETAIL} Le moteur Web d'XSplit gère les WebSocket ; non vérifié avec Batlay.` },
    transparency: { certainty: "expected", detail: "Réglable dans Propriétés → Display (documenté par XSplit)." },
    localAccess: { certainty: "expected", detail: "Les URL http locales sont acceptées comme n'importe quelle adresse ; non vérifié avec Batlay." },
    limitations: [
      "Si la page apparaît décalée ou coupée, activez « Ajuster à la source » dans Batlay et recollez l'URL.",
      "Le CSS/JavaScript personnalisé d'XSplit (Custom Code) n'est pas nécessaire ; laissez-le vide.",
    ],
    alternatives: [],
    defaultFormat: "landscape",
    recommendFit: false,
    preferredHost: null,
    docs: PLATFORM_DOCS.xsplit,
  },
  {
    id: "other",
    name: "Autre logiciel (source Web)",
    status: "unverified",
    statusDetail:
      "Tout logiciel qui affiche une page Web dans une scène peut fonctionner, à condition de remplir les conditions ci-dessous. Rien n'a été vérifié au cas par cas.",
    sourceName: "Navigateur, Web, Webpage, Link, URL… (selon le logiciel)",
    method: "Source Web avec l'URL de l'overlay",
    steps: [
      "Cherchez une source de type Navigateur / Web / Webpage / Link / URL.",
      "Collez l'URL copiée depuis Batlay.",
      "Fixez la taille de la source sur celle indiquée par Batlay ; si le logiciel ne le permet pas, activez « Ajuster à la source ».",
      "Vérifiez dans Batlay → Intégrations que « Sources connectées » passe à 1 ou plus.",
    ],
    sizeField: "Taille / résolution de la source Web",
    liveUpdates: { certainty: "unknown", detail: `${COMMON_LIVE_DETAIL} Il faut un moteur récent qui gère les WebSocket.` },
    transparency: { certainty: "unknown", detail: "Il faut que la source respecte un fond transparent (cas des moteurs Chromium)." },
    localAccess: { certainty: "unknown", detail: "Il faut que le logiciel accepte une adresse http (pas seulement https) sur 127.0.0.1." },
    limitations: [
      "Conditions : adresse http locale acceptée, JavaScript et WebSocket actifs, fond transparent respecté.",
      "Si le logiciel n'accepte qu'une caméra ou une capture de fenêtre : composez la scène dans OBS et utilisez sa caméra virtuelle.",
    ],
    alternatives: [],
    defaultFormat: "landscape",
    recommendFit: true,
    preferredHost: null,
    docs: PLATFORM_DOCS.other,
  },
];

export function listPlatforms(): readonly IntegrationPlatform[] {
  return PLATFORMS;
}

/** Logiciel demandé, ou « Autre logiciel » pour un identifiant inconnu (ancien réglage, valeur corrompue). */
export function getPlatform(id: unknown): IntegrationPlatform {
  const wanted = isPlatformId(id) ? id : "other";
  return PLATFORMS.find((p) => p.id === wanted) ?? PLATFORMS[PLATFORMS.length - 1];
}

/** Choix mémorisé → identifiant valide. Par défaut : TikTok LIVE Studio, priorité de cette page. */
export function normalizePlatformId(raw: unknown, fallback: PlatformId = "tiktok-live-studio"): PlatformId {
  return isPlatformId(raw) ? raw : fallback;
}

export { PLATFORM_IDS };
