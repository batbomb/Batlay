/**
 * Accès au cœur de Batlay (Rust) depuis le tableau de bord, par Tauri.
 *
 * Une seule commande existe : `batlay(cmd, args)`. Le cœur tient la liste
 * fermée des commandes et revalide chaque argument ; la page ne transmet
 * jamais de port, de chemin, de jeton ni d'adresse arbitraire. Aucune
 * dépendance npm : Tauri expose `window.__TAURI__` (option withGlobalTauri).
 *
 * Hors de l'application (tests, `vite` seul dans un navigateur), ces
 * fonctions renvoient `null` / ne font rien.
 */

interface TauriGlobal {
  core: { invoke(cmd: string, args?: Record<string, unknown>): Promise<unknown> };
  event: { listen(event: string, handler: (event: { payload: unknown }) => void): Promise<() => void> };
}

export type NativeInvoke = (cmd: string, args: unknown[]) => Promise<unknown>;

function tauri(): TauriGlobal | null {
  const t = (globalThis as { __TAURI__?: Partial<TauriGlobal> }).__TAURI__;
  return t?.core?.invoke && t.event?.listen ? (t as TauriGlobal) : null;
}

/** Appel au cœur ; l'erreur renvoyée par le cœur (texte) devient une `Error`. */
export function nativeInvoke(): NativeInvoke | null {
  const t = tauri();
  if (!t) return null;
  return async (cmd, args) => {
    try {
      return await t.core.invoke("batlay", { cmd, args });
    } catch (err) {
      throw err instanceof Error ? err : new Error(typeof err === "string" ? err : String(err));
    }
  };
}

/** Écoute un événement du cœur. Renvoie de quoi se désabonner (utilisable tout de suite). */
export function nativeListen<T>(channel: string, callback: (payload: T) => void): () => void {
  const t = tauri();
  if (!t) return () => undefined;
  let stopped = false;
  let unlisten: (() => void) | null = null;
  void t.event
    .listen(channel, (event) => {
      if (!stopped) callback(event.payload as T);
    })
    .then((off) => {
      if (stopped) off();
      else unlisten = off;
    })
    .catch(() => undefined);
  return () => {
    stopped = true;
    unlisten?.();
    unlisten = null;
  };
}

/** Données posées par l'application avant le chargement de la page (thème résolu, version). */
export interface NativeInitial {
  theme?: { preference: string; resolved: string };
  version?: string;
}

export function nativeInitial(): NativeInitial | null {
  const v = (globalThis as { __BATLAY_INITIAL__?: NativeInitial }).__BATLAY_INITIAL__;
  return v && typeof v === "object" ? v : null;
}
