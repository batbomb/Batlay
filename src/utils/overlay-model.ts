import { nanoid } from "nanoid";
import type { OverlayComponentConfig, OverlayComponentType } from "@/types/overlay";
import { defaultFill } from "@/utils/overlay-style";

/**
 * Opérations d'édition sur la liste de composants d'un overlay, en fonctions
 * pures (elles renvoient de nouvelles listes, sans jamais muter l'entrée).
 * L'ordre d'empilement est le champ `order` : plus grand = dessiné au-dessus.
 * Après chaque opération les `order` sont renumérotés 0..n-1 sans trou.
 */

export const COMPONENT_LABELS: Record<OverlayComponentType, string> = {
  artwork: "Pochette",
  title: "Titre",
  artist: "Artiste(s)",
  album: "Album",
  progressBar: "Barre de progression",
  elapsedTime: "Temps écoulé",
  remainingTime: "Temps restant",
  duration: "Durée",
  source: "Source",
  text: "Texte libre",
  shape: "Forme / fond",
  image: "Image",
  clock: "Horloge",
  countdown: "Compte à rebours",
  goal: "Objectif",
};

export const COMPONENT_DESCRIPTIONS: Record<OverlayComponentType, string> = {
  artwork: "Image de l'album",
  title: "Nom du morceau",
  artist: "Tous les artistes du morceau",
  album: "Nom de l'album",
  progressBar: "Avancement dans le morceau",
  elapsedTime: "ex. 1:24",
  remainingTime: "ex. -2:26",
  duration: "Durée totale",
  source: "Spotify, VLC, Chrome…",
  text: "Texte avec {title} {artist}…",
  shape: "Rectangle, carte, séparateur",
  image: "Logo, photo, GIF animé…",
  clock: "Heure ou date en direct",
  countdown: "« Début dans 04:59 »",
  goal: "Barre : followers, abonnés, cadeaux…",
};

/**
 * Tailles de canvas courantes, partagées entre l'éditeur (Taille du canvas) et
 * la fenêtre de création : une seule liste, pour que les deux proposent
 * toujours les mêmes formats.
 */
export const CANVAS_SIZES = [
  { label: "Compact 420 × 96", w: 420, h: 96 },
  { label: "Standard 780 × 220", w: 780, h: 220 },
  { label: "Large 960 × 300", w: 960, h: 300 },
  { label: "Plein écran 1920 × 1080", w: 1920, h: 1080 },
] as const;

export const COMPONENT_TYPES = Object.keys(COMPONENT_LABELS) as OverlayComponentType[];

export function layerName(component: OverlayComponentConfig): string {
  return component.name?.trim() || COMPONENT_LABELS[component.type];
}

type Defaults = Pick<OverlayComponentConfig, "style"> & { size: { width: number; height: number }; content?: string };

const DEFAULTS: Record<OverlayComponentType, Defaults> = {
  artwork: { size: { width: 96, height: 96 }, style: { borderRadius: 8, shape: "square" } },
  title: { size: { width: 320, height: 32 }, style: { fontSize: 22, fontWeight: 600, color: "#FFFFFF" } },
  artist: { size: { width: 320, height: 24 }, style: { fontSize: 16, fontWeight: 400, color: "#B8B8C4" } },
  album: { size: { width: 320, height: 22 }, style: { fontSize: 14, fontWeight: 400, color: "#8A8A99" } },
  progressBar: {
    size: { width: 320, height: 6 },
    style: { fillColor: "#FFFFFF", trackColor: "rgba(255,255,255,0.15)", borderRadius: 3 },
  },
  elapsedTime: { size: { width: 64, height: 18 }, style: { fontSize: 12, color: "#8A8A99" } },
  remainingTime: { size: { width: 64, height: 18 }, style: { fontSize: 12, color: "#8A8A99", textAlign: "right" } },
  duration: { size: { width: 64, height: 18 }, style: { fontSize: 12, color: "#8A8A99", textAlign: "right" } },
  source: { size: { width: 140, height: 20 }, style: { fontSize: 12, color: "#8A8A99" } },
  text: { size: { width: 260, height: 28 }, style: { fontSize: 18, fontWeight: 600, color: "#FFFFFF" }, content: "En ce moment" },
  shape: {
    size: { width: 240, height: 90 },
    style: { borderRadius: 12, background: defaultFill({ color: "#000000", opacity: 0.55 }) },
  },
  image: { size: { width: 240, height: 240 }, style: { objectFit: "contain" } },
  clock: { size: { width: 160, height: 40 }, style: { fontSize: 28, fontWeight: 700, color: "#FFFFFF", clockFormat: "time" } },
  countdown: {
    size: { width: 260, height: 70 },
    style: { fontSize: 56, fontWeight: 800, color: "#FFFFFF", textAlign: "center", verticalAlign: "middle", countdownSeconds: 300 },
    content: "C'est parti !",
  },
  goal: {
    size: { width: 520, height: 64 },
    style: { fontSize: 18, fontWeight: 700, color: "#FFFFFF", fillColor: "#9B6BFF", trackColor: "rgba(255,255,255,0.15)", borderRadius: 10, goalMetric: "followers", goalTarget: 20 },
    content: "Objectif followers",
  },
};

/** Composants qui ne dépendent pas de la musique : toujours affichés dans un overlay personnalisé. */
export const MUSIC_INDEPENDENT_TYPES: ReadonlySet<OverlayComponentType> = new Set(["image", "shape", "text", "clock", "countdown", "goal"]);

/** Libellé de l'objectif suivi. */
export const GOAL_METRIC_LABELS: Record<import("@/types/overlay").GoalMetric, string> = {
  followers: "Nouveaux followers",
  subs: "Abonnés",
  gifts: "Cadeaux",
  messages: "Messages",
  likes: "Likes TikTok",
  viewers: "Spectateurs TikTok",
};

/** Avancement d'un objectif (0 → 1) ; objectif nul ou invalide = 0. */
export function goalProgress(value: number, target: number | undefined): number {
  if (!target || !Number.isFinite(target) || target <= 0) return 0;
  return Math.max(0, Math.min(1, value / target));
}

/** « 4:59 », « 1:02:03 », jamais négatif. */
export function formatCountdown(remainingMs: number): string {
  const total = Math.max(0, Math.ceil(remainingMs / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/**
 * Taille d'une image ajoutée : ses proportions, dans au plus 40 % du canvas
 * (et jamais plus grande que l'image elle-même).
 */
export function fitImageSize(natural: { width: number; height: number }, canvas: { width: number; height: number }): { width: number; height: number } {
  const w = Math.max(1, natural.width);
  const h = Math.max(1, natural.height);
  const scale = Math.min(1, (canvas.width * 0.4) / w, (canvas.height * 0.4) / h);
  return { width: Math.max(16, Math.round(w * scale)), height: Math.max(16, Math.round(h * scale)) };
}

/** Composant « image » prêt à l'emploi, centré sur `at` (ou au centre du canvas), au-dessus des autres. */
export function createImageComponent(
  mediaId: string,
  natural: { width: number; height: number },
  canvas: { width: number; height: number },
  existing: OverlayComponentConfig[],
  at?: { x: number; y: number },
): OverlayComponentConfig {
  const base = createComponent("image", canvas, existing);
  const size = fitImageSize(natural, canvas);
  const cx = at?.x ?? canvas.width / 2;
  const cy = at?.y ?? canvas.height / 2;
  const x = Math.round(Math.min(Math.max(0, cx - size.width / 2), Math.max(0, canvas.width - size.width)));
  const y = Math.round(Math.min(Math.max(0, cy - size.height / 2), Math.max(0, canvas.height - size.height)));
  return { ...base, mediaId, transform: { x, y, ...size } };
}

/** Renumérote `order` en 0..n-1 en conservant l'empilement actuel (égalités : ordre du tableau). */
export function normalizeOrder(components: OverlayComponentConfig[]): OverlayComponentConfig[] {
  return components
    .map((c, index) => ({ c, index }))
    .sort((a, b) => a.c.order - b.c.order || a.index - b.index)
    .map(({ c }, i) => (c.order === i ? c : { ...c, order: i }));
}

/**
 * Nouveau composant du type demandé, placé au-dessus de tous les autres,
 * centré dans le canvas et légèrement décalé s'il y en a déjà (pour ne pas
 * empiler les ajouts successifs exactement au même endroit).
 */
export function createComponent(
  type: OverlayComponentType,
  canvas: { width: number; height: number },
  existing: OverlayComponentConfig[]
): OverlayComponentConfig {
  const d = DEFAULTS[type];
  const width = Math.min(d.size.width, canvas.width);
  const height = Math.min(d.size.height, canvas.height);
  const cascade = (existing.length % 8) * 12;
  const x = Math.round(Math.min(Math.max(0, (canvas.width - width) / 2 + cascade), canvas.width - width));
  const y = Math.round(Math.min(Math.max(0, (canvas.height - height) / 2 + cascade), canvas.height - height));
  return {
    id: nanoid(6),
    type,
    visible: true,
    order: existing.length === 0 ? 0 : Math.max(...existing.map((c) => c.order)) + 1,
    transform: { x, y, width, height },
    style: structuredClone(d.style),
    ...(d.content !== undefined ? { content: d.content } : {}),
  };
}

export function addComponent(
  components: OverlayComponentConfig[],
  component: OverlayComponentConfig
): OverlayComponentConfig[] {
  return normalizeOrder([...components, component]);
}

export function removeComponent(components: OverlayComponentConfig[], id: string): OverlayComponentConfig[] {
  return normalizeOrder(components.filter((c) => c.id !== id));
}

/** Copie décalée de 16 px, insérée juste au-dessus de l'original. */
export function duplicateComponent(
  components: OverlayComponentConfig[],
  id: string
): { components: OverlayComponentConfig[]; newId: string | null } {
  const original = components.find((c) => c.id === id);
  if (!original) return { components, newId: null };
  const copy: OverlayComponentConfig = {
    ...structuredClone(original),
    id: nanoid(6),
    locked: false,
    name: original.name ? `${original.name} (copie)` : undefined,
    transform: { ...original.transform, x: original.transform.x + 16, y: original.transform.y + 16 },
    order: original.order + 0.5, // juste au-dessus de l'original, renuméroté ci-dessous
  };
  return { components: normalizeOrder([...components, copy]), newId: copy.id };
}

export type LayerMove = "front" | "back" | "forward" | "backward";

/** Change la position d'un calque dans l'empilement. */
export function moveLayer(components: OverlayComponentConfig[], id: string, move: LayerMove): OverlayComponentConfig[] {
  const stack = normalizeOrder(components);
  const from = stack.findIndex((c) => c.id === id);
  if (from === -1) return components;
  const to =
    move === "front" ? stack.length - 1 : move === "back" ? 0 : move === "forward" ? from + 1 : from - 1;
  return reorderTo(stack, id, to);
}

/** Place le calque `id` à l'index `toIndex` de l'empilement (0 = tout en bas). Sert au glisser-déposer. */
export function reorderTo(components: OverlayComponentConfig[], id: string, toIndex: number): OverlayComponentConfig[] {
  const stack = normalizeOrder(components);
  const from = stack.findIndex((c) => c.id === id);
  if (from === -1) return components;
  const target = Math.min(stack.length - 1, Math.max(0, toIndex));
  if (target === from) return stack;
  const next = [...stack];
  const [moved] = next.splice(from, 1);
  next.splice(target, 0, moved);
  return next.map((c, i) => (c.order === i ? c : { ...c, order: i }));
}

export type AlignKind = "left" | "centerH" | "right" | "top" | "middleV" | "bottom";

/** Aligne un composant sur le canvas. */
export function alignInCanvas(
  transform: OverlayComponentConfig["transform"],
  canvas: { width: number; height: number },
  kind: AlignKind
): OverlayComponentConfig["transform"] {
  switch (kind) {
    case "left":
      return { ...transform, x: 0 };
    case "centerH":
      return { ...transform, x: Math.round((canvas.width - transform.width) / 2) };
    case "right":
      return { ...transform, x: canvas.width - transform.width };
    case "top":
      return { ...transform, y: 0 };
    case "middleV":
      return { ...transform, y: Math.round((canvas.height - transform.height) / 2) };
    case "bottom":
      return { ...transform, y: canvas.height - transform.height };
  }
}
