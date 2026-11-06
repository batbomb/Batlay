import { create } from "zustand";
import type { UpdateStatus } from "../../shared/update";

interface UpdateStoreState {
  status: UpdateStatus;
  version: string | null;
  /** Installation lancée : l'écran « Installation de la mise à jour » recouvre Batlay jusqu'à sa fermeture. */
  installing: boolean;
  /** À appeler une fois au démarrage : lit l'état courant et suit les changements. */
  init: () => () => void;
  check: () => Promise<void>;
  download: () => Promise<void>;
  install: () => Promise<void>;
  openReleases: () => Promise<void>;
}

// Hors de l'application (navigateur, tests) il n'y a pas de pont : la mise à jour est simplement indisponible.
const bridge = () => (typeof window !== "undefined" ? window.batlay?.update : undefined);

export const useUpdateStore = create<UpdateStoreState>((set) => ({
  status: { state: "idle" },
  version: null,
  installing: false,

  init: () => {
    const api = bridge();
    if (!api) return () => {};
    void api.getVersion().then((version) => set({ version })).catch(() => {});
    void api.getStatus().then((status) => set({ status })).catch(() => {});
    return api.onStatus((status) => set({ status }));
  },

  check: async () => {
    const api = bridge();
    if (!api) return;
    set({ status: await api.check() });
  },
  download: async () => {
    const api = bridge();
    if (!api) return;
    set({ status: await api.download() });
  },
  install: async () => {
    const api = bridge();
    if (!api) return;
    // L'écran d'installation s'affiche d'abord (l'installateur, silencieux, n'a pas de fenêtre),
    // puis Batlay lance l'installateur et se ferme ; il se rouvre tout seul à la fin.
    set({ installing: true });
    await new Promise((r) => setTimeout(r, 1400));
    const ok = await api.install().catch(() => false);
    if (!ok) set({ installing: false });
  },
  openReleases: async () => {
    await bridge()?.openReleases();
  },
}));
