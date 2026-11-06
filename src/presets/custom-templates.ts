import { nanoid } from "nanoid";
import type { ComponentStyle, FillConfig, OverlayComponentConfig, OverlayConfig } from "@/types/overlay";
import { defaultFill } from "@/utils/overlay-style";

/**
 * Modèles d'overlays personnalisés (écrans d'attente, cadres, bandeaux).
 * Chacun est un point de départ entièrement modifiable dans l'éditeur ; les
 * emplacements d'image (logo, photo) sont à remplir par glisser-déposer.
 */

export type CustomTemplateId = "blank" | "starting" | "brb" | "ending" | "webcam" | "socials" | "goal" | "tiktokLikes" | "vertical" | "logo";

export interface CustomTemplate {
  id: CustomTemplateId;
  label: string;
  description: string;
  build: () => Omit<OverlayConfig, "id" | "name" | "createdAt" | "updatedAt">;
}

const ANIM = { enter: "fade", exit: "fade", durationMs: 500, easing: "cubic-bezier(0.16, 1, 0.3, 1)" } as const;
const FONT = "'Space Grotesk Variable', 'Segoe UI', system-ui, sans-serif";

let order = 0;
function c(type: OverlayComponentConfig["type"], name: string, x: number, y: number, width: number, height: number, style: ComponentStyle = {}, extra: Partial<OverlayComponentConfig> = {}): OverlayComponentConfig {
  return { id: nanoid(6), type, name, visible: true, order: order++, transform: { x, y, width, height }, style, ...extra };
}
const text = (name: string, content: string, x: number, y: number, w: number, h: number, style: ComponentStyle) =>
  c("text", name, x, y, w, h, { fontFamily: FONT, color: "#FFFFFF", fontWeight: 700, verticalAlign: "middle", overflow: "wrap", ...style }, { content });
const gradient = (from: string, to: string, angle = 135): FillConfig => defaultFill({ mode: "gradient", opacity: 1, gradient: { type: "linear", angle, from, to } });

function scene(w: number, h: number, background: FillConfig | undefined, make: () => OverlayComponentConfig[]) {
  order = 0;
  return {
    schemaVersion: 1 as const,
    kind: "custom" as const,
    presetId: "blank" as const,
    theme: { canvasWidth: w, canvasHeight: h, padding: 0, gap: 0, ...(background ? { background } : {}) },
    animations: { ...ANIM },
    components: make(),
  };
}

/** Écran plein (commence bientôt, pause, fin) : même mise en page, textes différents. */
function screen(title: string, subtitle: string, from: string, to: string, extra: () => OverlayComponentConfig[] = () => []) {
  return () =>
    scene(1920, 1080, gradient(from, to), () => [
      c("shape", "Halo", 1240, 40, 640, 640, { borderRadius: 320, background: defaultFill({ color: "#FFFFFF", opacity: 0.06 }) }),
      c("image", "Logo (à remplacer)", 840, 170, 240, 240, { objectFit: "contain" }),
      text("Titre", title, 260, 450, 1400, 150, { fontSize: 104, textAlign: "center", letterSpacing: -1 }),
      text("Sous-titre", subtitle, 360, 610, 1200, 60, { fontSize: 38, fontWeight: 500, textAlign: "center", color: "#E6DDFF" }),
      c("shape", "Barre du bas", 0, 960, 1920, 120, { background: defaultFill({ color: "#000000", opacity: 0.35 }) }),
      text("Réseaux", "twitch.tv/pseudo  ·  @pseudo", 60, 975, 1200, 90, { fontSize: 30, fontWeight: 600, color: "#FFFFFF" }),
      text("Musique", "♪ {title} — {artist}", 60, 890, 1100, 50, { fontSize: 24, fontWeight: 500, color: "#D6CCF5", overflow: "ellipsis" }),
      c("clock", "Heure", 1660, 975, 200, 90, { fontFamily: FONT, fontSize: 44, fontWeight: 700, color: "#FFFFFF", textAlign: "right", verticalAlign: "middle", clockFormat: "time" }),
      ...extra(),
    ]);
}

const liveLine = (name: string, content: string) =>
  text(name, content, 460, 790, 1000, 60, { fontSize: 32, fontWeight: 600, textAlign: "center", color: "#FFFFFF", overflow: "ellipsis" });

export const CUSTOM_TEMPLATES: CustomTemplate[] = [
  { id: "blank", label: "Toile vide", description: "1920 × 1080, à composer librement.", build: () => scene(1920, 1080, undefined, () => []) },
  {
    id: "starting",
    label: "Ça commence bientôt",
    description: "Écran d'attente : logo, titre, compte à rebours, réseaux, heure.",
    build: screen("Le live commence bientôt", "Installe-toi, on arrive !", "#1A0B2E", "#5B2A86", () => [
      c("countdown", "Compte à rebours", 660, 700, 600, 120, { fontFamily: FONT, fontSize: 96, fontWeight: 800, color: "#FFFFFF", textAlign: "center", verticalAlign: "middle", countdownSeconds: 300 }, { content: "C'est parti !" }),
    ]),
  },
  {
    id: "brb",
    label: "Pause",
    description: "Écran « je reviens » : dernier follower, réseaux, heure.",
    build: screen("Petite pause", "Je reviens tout de suite", "#0B1E2E", "#1F5F7A", () => [
      liveLine("Dernier follower", "💜 Dernier follower : {lastFollower}"),
    ]),
  },
  {
    id: "ending",
    label: "Fin du live",
    description: "Écran de fin : remerciements, dernier abonné, réseaux.",
    build: screen("Merci d'être passé !", "À très vite pour le prochain live", "#2E0B1A", "#8A2B4F", () => [
      liveLine("Dernier abonné", "⭐ Merci {lastSub} pour l'abonnement !"),
    ]),
  },
  {
    id: "webcam",
    label: "Cadre webcam",
    description: "Cadre arrondi et étiquette de pseudo, à poser sur la caméra.",
    build: () =>
      scene(640, 420, undefined, () => [
        c("shape", "Cadre", 10, 10, 620, 360, { borderRadius: 24, border: { width: 6, color: "#9B6BFF", style: "solid" }, boxShadow: { enabled: true, x: 0, y: 0, blur: 24, spread: 0, color: "#9B6BFF", opacity: 0.6 } }),
        c("shape", "Étiquette", 30, 340, 260, 60, { borderRadius: 14, background: gradient("#7C4DFF", "#FE2C55", 90) }),
        text("Pseudo", "@pseudo", 46, 340, 230, 60, { fontSize: 26 }),
      ]),
  },
  {
    id: "socials",
    label: "Bandeau réseaux",
    description: "Bandeau bas d'écran : vos réseaux et l'heure.",
    build: () =>
      scene(1920, 120, undefined, () => [
        c("shape", "Fond", 0, 0, 1920, 120, { background: gradient("#120A24", "#2A1450", 90), border: { width: 2, color: "#9B6BFF", style: "solid" } }),
        c("image", "Logo (à remplacer)", 24, 14, 92, 92, { objectFit: "contain" }),
        text("Réseaux", "twitch.tv/pseudo  ·  tiktok.com/@pseudo  ·  @pseudo", 140, 0, 1400, 120, { fontSize: 34, fontWeight: 600 }),
        c("clock", "Heure", 1640, 0, 250, 120, { fontFamily: FONT, fontSize: 44, fontWeight: 700, color: "#FFFFFF", textAlign: "right", verticalAlign: "middle", clockFormat: "time" }),
      ]),
  },
  {
    id: "goal",
    label: "Objectif du live",
    description: "Barre de progression : followers, abonnés, cadeaux ou messages.",
    build: () =>
      scene(720, 120, undefined, () => [
        c("shape", "Fond", 0, 0, 720, 120, { borderRadius: 22, background: defaultFill({ color: "#0E0A1A", opacity: 0.78 }), border: { width: 2, color: "#9B6BFF", style: "solid" } }),
        c("goal", "Objectif", 28, 22, 664, 76, { fontFamily: FONT, fontSize: 24, fontWeight: 700, color: "#FFFFFF", fillColor: "#9B6BFF", trackColor: "rgba(255,255,255,0.14)", borderRadius: 12, goalMetric: "followers", goalTarget: 20 }, { content: "Objectif followers" }),
      ]),
  },
  {
    id: "tiktokLikes",
    label: "Likes et spectateurs TikTok",
    description: "Objectif de likes, avec le nombre de spectateurs en direct.",
    build: () =>
      scene(720, 170, undefined, () => [
        c("shape", "Fond", 0, 0, 720, 170, { borderRadius: 22, background: defaultFill({ color: "#0E0A1A", opacity: 0.78 }), border: { width: 2, color: "#FE2C55", style: "solid" } }),
        c("goal", "Objectif de likes", 28, 22, 664, 76, { fontFamily: FONT, fontSize: 24, fontWeight: 700, color: "#FFFFFF", fillColor: "#FE2C55", trackColor: "rgba(255,255,255,0.14)", borderRadius: 12, goalMetric: "likes", goalTarget: 10000 }, { content: "Objectif likes ❤" }),
        text("Spectateurs", "👀 {viewers} spectateurs", 28, 110, 664, 44, { fontSize: 24, fontWeight: 600, color: "#FFFFFFCC" }),
      ]),
  },
  {
    id: "vertical",
    label: "Écran vertical (TikTok)",
    description: "1080 × 1920 : titre en haut, logo, réseaux en bas, loin des zones de TikTok.",
    build: () =>
      scene(1080, 1920, undefined, () => [
        c("shape", "Bandeau du haut", 60, 120, 960, 180, { borderRadius: 36, background: defaultFill({ color: "#000000", opacity: 0.45 }) }),
        text("Titre", "Live du soir 🎮", 90, 120, 900, 180, { fontSize: 70, textAlign: "center" }),
        c("image", "Logo (à remplacer)", 390, 360, 300, 300, { objectFit: "contain" }),
        text("Réseaux", "@pseudo", 90, 700, 900, 90, { textAlign: "center", fontSize: 44, textShadow: { enabled: true, x: 0, y: 3, blur: 10, spread: 0, color: "#000000", opacity: 0.7 } }),
      ]),
  },
  {
    id: "logo",
    label: "Logo ou image",
    description: "Une image seule (logo, filigrane, sticker) à placer dans la scène.",
    build: () => scene(500, 500, undefined, () => [c("image", "Image (à remplacer)", 0, 0, 500, 500, { objectFit: "contain" })]),
  },
];

export function getCustomTemplate(id: CustomTemplateId): CustomTemplate {
  return CUSTOM_TEMPLATES.find((t) => t.id === id) ?? CUSTOM_TEMPLATES[0];
}
