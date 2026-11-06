import { create } from "zustand";
import { applyThemePreference, systemPrefersDark } from "@/theme/apply-theme";
import { applyAccentColor } from "@/theme/accent";
import { applyUiPrefs, type UiDensity, type UiMotion } from "@/theme/ui-prefs";
import type { ThemePreference } from "../../shared/theme";

export interface Settings {
  launchOnStartup: boolean;
  startMinimized: boolean;
  theme: ThemePreference;
  language: "fr" | "en";
  overlayServerPort: number;
  preferredSystemMediaAppId: string | null;
  /** Couleur des boutons choisie par l'utilisateur (hex `#rrggbb`). `null` = violet par défaut. */
  accentColor: string | null;
  /** Densité de l'interface (absente dans une ancienne configuration = confortable). */
  density?: UiDensity;
  /** Animations de l'interface (absentes = normales). */
  uiMotion?: UiMotion;
}

interface SettingsStoreState {
  settings: Settings | null;
  overlayServerRunning: boolean;
  load: () => Promise<void>;
  update: (partial: Partial<Settings>) => Promise<void>;
}

export const useSettingsStore = create<SettingsStoreState>((set) => ({
  settings: null,
  overlayServerRunning: false,

  load: async () => {
    const config = await window.batlay.config.get();
    const running = await window.batlay.overlay.isRunning();
    set({ settings: config.settings, overlayServerRunning: running });
    // Thème posé avant le premier rendu par l'application (window.__BATLAY_INITIAL__) ;
    // réappliqué ici depuis les réglages : après un rechargement de la page (F5), la
    // valeur posée à l'ouverture de la fenêtre peut dater d'avant un changement de thème.
    applyThemePreference(document.documentElement, config.settings.theme, systemPrefersDark());
    // Couleur des boutons : appliquée dès le chargement, comme le thème.
    applyAccentColor(document.documentElement, config.settings.accentColor ?? null);
    applyUiPrefs(document.documentElement, config.settings);
  },

  update: async (partial) => {
    const updated = await window.batlay.config.setSettings(partial);
    set({ settings: updated });
    // Thème appliqué tout de suite, sans attendre un rechargement. Pour « system »
    // le cœur de Batlay confirme ensuite par `theme:changed` (source de vérité
    // = nativeTheme) : si matchMedia n'a pas encore suivi, cet événement corrige.
    if (partial.theme !== undefined) {
      applyThemePreference(document.documentElement, updated.theme, systemPrefersDark());
    }
    if (partial.accentColor !== undefined) {
      applyAccentColor(document.documentElement, updated.accentColor);
    }
    if (partial.density !== undefined || partial.uiMotion !== undefined) applyUiPrefs(document.documentElement, updated);
  },
}));
