import { create } from "zustand";
import { initialTwitchState, type TwitchState } from "../../shared/twitch";

interface TwitchStoreState {
  state: TwitchState;
  /** Faux tant que le cœur de Batlay n'a pas répondu : évite d'afficher « Connecter » puis de le retirer. */
  ready: boolean;
  /** À appeler une fois au démarrage : lit l'état courant et suit les changements. */
  init: () => () => void;
  connect: () => Promise<void>;
  cancel: () => Promise<void>;
  disconnect: () => Promise<void>;
  /** Pseudo ou lien Twitch ; `null` = sa propre chaîne. `false` si la saisie n'est pas reconnue. */
  setChannel: (channel: string | null) => Promise<boolean>;
  sendTest: () => Promise<void>;
  /** Recharge les sources OBS du chat ; renvoie le nombre de sources prévenues. */
  restartOverlay: () => Promise<number>;
}

// Hors de l'application (navigateur, tests) il n'y a pas de pont : le module est simplement indisponible.
const bridge = () => (typeof window !== "undefined" ? window.batlay?.twitch : undefined);

/**
 * Miroir de l'état tenu par le cœur de Batlay (crates/batlay-core/src/twitch/).
 * Le renderer ne détient ni jeton, ni port, ni URL d'autorisation : il affiche
 * un état et envoie des ordres (le seul paramètre est le pseudo / lien de la chaîne à afficher).
 */
export const useTwitchStore = create<TwitchStoreState>((set) => ({
  state: initialTwitchState(),
  ready: false,

  init: () => {
    const api = bridge();
    if (!api) return () => {};
    void api
      .getState()
      .then((state) => set((s) => ({ state: state ?? s.state, ready: true })))
      .catch(() => set({ ready: true }));
    return api.onState((state) => set({ state, ready: true }));
  },

  connect: async () => {
    await bridge()?.connect();
  },
  cancel: async () => {
    await bridge()?.cancel();
  },
  disconnect: async () => {
    await bridge()?.disconnect();
  },
  setChannel: async (channel) => (await bridge()?.setChannel(channel)) ?? false,
  sendTest: async () => {
    await bridge()?.sendTest();
  },
  restartOverlay: async () => (await bridge()?.restartOverlay()) ?? 0,
}));
