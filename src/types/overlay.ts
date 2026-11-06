export type OverlayComponentType =
  | "artwork"
  | "title"
  | "artist"
  | "album"
  | "progressBar"
  | "elapsedTime"
  | "remainingTime"
  | "duration"
  | "source"
  /** Texte libre, avec variables {title} {artist} {album} {source}. */
  | "text"
  /** Rectangle décoratif : carte, séparateur, pastille... */
  | "shape"
  /** Image importée (PNG, JPG, GIF animé, WebP) : logo, photo, cadre, sticker… */
  | "image"
  /** Heure et/ou date, mises à jour en direct. */
  | "clock"
  /** Compte à rebours (« Le live commence dans 04:59 »), lancé à l'ouverture de la source. */
  | "countdown"
  /** Barre d'objectif du live (followers, abonnés, cadeaux, messages). */
  | "goal";

export type GoalMetric = "followers" | "subs" | "gifts" | "messages" | "likes" | "viewers";

/** Format du composant « clock ». */
export type ClockFormat = "time" | "timeSeconds" | "date" | "dateTime";

import type { ComponentAnimationSettings, EnterEffectId, ExitEffectId, LoopSpec } from "@/animation/config";

/** Effets historiques (v0.1 → v0.3.6) : toujours valides, voir src/animation/config.ts. */
export type AnimationKind =
  | "fade"
  | "slideLeft"
  | "slideRight"
  | "slideUp"
  | "slideDown"
  | "scale";

/**
 * Animations de l'overlay entier (transition à chaque changement de morceau).
 * Les quatre premiers champs existent depuis la v0.1 ; les suivants sont
 * FACULTATIFS : un overlay enregistré avant leur ajout reste valide, chaque
 * champ absent prend sa valeur par défaut (normalizeOverlayAnimation).
 */
export interface AnimationConfig {
  enter: EnterEffectId;
  exit: ExitEffectId;
  durationMs: number;
  easing: string; // ex. "ease-out", "cubic-bezier(0.16, 1, 0.3, 1)"
  /** Absent = activé. */
  enabled?: boolean;
  delayMs?: number;
  /** 0 → 1 : amplitude des glissements, zooms, flous. */
  intensity?: number;
  /** Décalage entre composants, dans l'ordre d'empilement. */
  staggerMs?: number;
  /** Absent = respecte « réduire les animations » du système. */
  respectReducedMotion?: boolean;
}

/** Animation propre à un composant. Absent = celle de l'overlay. */
export type ComponentAnimation = Partial<Omit<ComponentAnimationSettings, "loop">> & { loop?: Partial<LoopSpec> };

export interface ComponentTransform {
  x: number;
  y: number;
  width: number;
  height: number;
}

// --- Briques de style réutilisables (composants ET fond de l'overlay) ---

export interface GradientConfig {
  type: "linear" | "radial";
  /** Degrés CSS : 0 = vers le haut, 90 = vers la droite, 180 = vers le bas. */
  angle: number;
  from: string;
  to: string;
}

/** Remplissage d'un fond : aucun, uni ou dégradé, avec transparence. */
/** Fond « pochette » : la pochette du morceau en cours, agrandie et floutée. */
export interface ArtworkFillConfig {
  /** Flou en px (0 → 60). */
  blur: number;
  /** Agrandissement (1 → 2,5) : cache les bords adoucis par le flou. */
  zoom: number;
  /** Assombrissement (0 → 0,9) : garde le texte lisible sur une pochette claire. */
  dim: number;
}

export interface FillConfig {
  /** `artwork` : pochette du morceau floutée ; `color` sert alors de fond de secours (pas de pochette). */
  mode: "none" | "solid" | "gradient" | "artwork";
  color: string;
  /** Transparence : 0 = invisible, 1 = opaque. S'applique à l'uni comme au dégradé. */
  opacity: number;
  gradient: GradientConfig;
  /** Réglages du mode `artwork` (facultatif : valeurs par défaut si absent). */
  artwork?: ArtworkFillConfig;
}

export interface BorderConfig {
  width: number;
  color: string;
  style: "solid" | "dashed" | "dotted";
}

export interface ShadowConfig {
  enabled: boolean;
  x: number;
  y: number;
  blur: number;
  spread: number;
  color: string;
  /** 0 à 1, appliqué à `color`. */
  opacity: number;
}

export type TextOverflowMode =
  /** Coupe avec « … » (comportement historique). */
  | "ellipsis"
  /** Passe à la ligne, dans la limite de la hauteur du bloc. */
  | "wrap"
  /** Défile en boucle SEULEMENT si le texte est plus large que le bloc. */
  | "marquee";

export interface ComponentStyle {
  // --- Texte ---
  fontFamily?: string;
  fontSize?: number;
  fontWeight?: number;
  color?: string;
  textAlign?: "left" | "center" | "right";
  verticalAlign?: "top" | "middle" | "bottom";
  italic?: boolean;
  letterSpacing?: number;
  lineHeight?: number;
  opacity?: number;
  maxWidth?: number;
  textTransform?: "none" | "uppercase" | "lowercase" | "capitalize";
  textShadow?: ShadowConfig;
  /** Comportement quand le texte est plus large que le bloc. Défaut : « ellipsis » (« marquee » pour l'artiste). */
  overflow?: TextOverflowMode;
  /** Vitesse du défilement, en px/s. */
  marqueeSpeed?: number;
  /** Séparateur des artistes (défaut « , »). Ne s'applique qu'au composant Artiste. */
  artistSeparator?: string;

  // --- Bloc (tous les composants) ---
  /** Fond du bloc : couleur, transparence, dégradé. */
  background?: FillConfig;
  border?: BorderConfig;
  boxShadow?: ShadowConfig;
  borderRadius?: number;
  /** Marge intérieure (px), utile pour un texte sur fond coloré. */
  padding?: number;

  // --- Artwork ---
  shape?: "square" | "circle";
  objectFit?: "cover" | "contain";

  // --- Horloge ---
  clockFormat?: ClockFormat;

  // --- Compte à rebours ---
  /** Durée en secondes (défaut 300). Le texte de fin est `content`. */
  countdownSeconds?: number;

  // --- Objectif ---
  goalMetric?: GoalMetric;
  goalTarget?: number;

  // --- Progress bar ---
  trackColor?: string;
  fillColor?: string;

  // --- Formats hérités (antérieurs à l'éditeur v2) : toujours lus, plus écrits ---
  /** @deprecated Remplacé par `border` (lu en repli pour les anciens overlays et les presets). */
  borderWidth?: number;
  /** @deprecated Remplacé par `border`. */
  borderColor?: string;
  /** @deprecated Remplacé par `boxShadow` ; `true` = ombre douce par défaut. */
  shadow?: boolean;
  /** @deprecated Remplacé par `background`. */
  backgroundColor?: string;
  /** @deprecated Remplacé par `background.opacity`. */
  backgroundOpacity?: number;
}

export interface OverlayComponentConfig {
  id: string;
  type: OverlayComponentType;
  visible: boolean;
  transform: ComponentTransform;
  style: ComponentStyle;
  /** Ordre d'empilement : le plus grand est dessiné au-dessus. */
  order: number;
  /** Nom du calque affiché dans l'éditeur. */
  name?: string;
  /** Verrouillé : ne peut plus être déplacé ni redimensionné à la souris. */
  locked?: boolean;
  /** Contenu du composant « text » (avec variables). */
  content?: string;
  /** Image importée du composant « image » (identifiant du fichier dans le dossier de Batlay). */
  mediaId?: string;
  /** Animations propres au composant (facultatif : absent = celles de l'overlay). */
  animation?: ComponentAnimation;
}

export interface OverlayTheme {
  canvasWidth: number;
  canvasHeight: number;
  padding: number;
  gap: number;
  /** Fond de l'overlay entier (le canvas). Absent = transparent. */
  background?: FillConfig;
  border?: BorderConfig;
  borderRadius?: number;
  boxShadow?: ShadowConfig;
}

/**
 * `music` (défaut, absent) : overlay musical, masqué quand rien ne joue.
 * `custom` : overlay personnalisé (écran d'attente, cadre webcam…) : images,
 * formes, textes et horloge restent affichés même sans musique.
 */
export type OverlayKind = "music" | "custom";

export interface OverlayConfig {
  schemaVersion: 1;
  id: string;
  name: string;
  kind?: OverlayKind;
  presetId: PresetId;
  components: OverlayComponentConfig[];
  theme: OverlayTheme;
  animations: AnimationConfig;
  createdAt: number;
  updatedAt: number;
}

export type PresetId =
  | "minimal"
  | "modern"
  | "glass"
  | "neon"
  | "compact"
  | "large"
  | "blank";
