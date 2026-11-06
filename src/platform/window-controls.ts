/**
 * Boutons de la barre de titre Batlay (les fenêtres n'ont plus la barre de
 * Windows depuis la 0.5) : réduire, agrandir / restaurer, fermer.
 *
 * Passe par l'API de fenêtre de Tauri exposée par `withGlobalTauri`
 * (`window.__TAURI__.window`), limitée par src-tauri/capabilities/main.json
 * à ces seules actions sur les fenêtres du tableau de bord. Le glisser de la
 * fenêtre passe par l'attribut `data-tauri-drag-region` (script de Tauri).
 *
 * Hors de l'application (tests, navigateur), `currentWindow()` vaut `null` :
 * la barre de titre n'affiche alors pas ses boutons.
 */

export interface AppWindow {
  minimize(): Promise<void>;
  toggleMaximize(): Promise<void>;
  close(): Promise<void>;
  isMaximized(): Promise<boolean>;
  onResized(handler: () => void): Promise<() => void>;
}

interface TauriWindowGlobal {
  window?: { getCurrentWindow?: () => AppWindow };
}

export function currentWindow(): AppWindow | null {
  try {
    const t = (globalThis as { __TAURI__?: TauriWindowGlobal }).__TAURI__;
    const w = t?.window?.getCurrentWindow?.();
    return w && typeof w.minimize === "function" && typeof w.close === "function" ? w : null;
  } catch {
    return null;
  }
}
