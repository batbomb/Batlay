/**
 * Structure de la navigation, en trois blocs séparés par un espace (sans titres de section) :
 *   1. Vue d'ensemble, Live, Overlays, Alertes, Intégrations
 *   2. Lecture en cours, Chat Twitch, Connexions
 *   3. Paramètres
 * Données pures (sans React) pour pouvoir tester l'ordre et les routes ; les icônes sont
 * associées dans Sidebar.tsx. Les routes historiques (/overlays, /editor, /connections) ne changent pas.
 */
export interface NavItem {
  to: string;
  label: string;
}
export interface NavGroup {
  id: "overview" | "sources" | "app";
  /** Titre de section ; `null` = pas d'en-tête (simple espace entre deux blocs). */
  label: string | null;
  items: NavItem[];
}

export const NAV_GROUPS: NavGroup[] = [
  {
    id: "overview",
    label: null,
    items: [
      { to: "/", label: "Vue d'ensemble" },
      // Tout le live au même endroit : chats Twitch et TikTok, journal, compteurs.
      { to: "/live", label: "Live" },
      // Page commune : une section pour les overlays Musique, une pour les overlays Chat.
      { to: "/overlays", label: "Overlays" },
      // Mode d'emploi des overlays dans chaque logiciel de streaming (OBS, TikTok LIVE Studio…).
      // Follows et abonnements (Twitch, TikTok via un outil tiers) : texte, image, son, animation.
      { to: "/alerts", label: "Alertes" },
      { to: "/integrations", label: "Intégrations" },
    ],
  },
  {
    id: "sources",
    label: null,
    items: [
      { to: "/musique", label: "Lecture en cours" },
      { to: "/chat", label: "Chat Twitch" },
      { to: "/connections", label: "Connexions" },
    ],
  },
  { id: "app", label: null, items: [{ to: "/settings", label: "Paramètres" }] },
];
