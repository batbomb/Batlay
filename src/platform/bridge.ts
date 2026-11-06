/**
 * `window.batlay` pour Batlay 0.5 (Tauri) : la MÊME surface que l'ancien
 * preload Electron, pour que le reste du tableau de bord n'ait pas changé.
 * Chaque méthode appelle la commande unique du cœur (voir native.ts) avec
 * des arguments positionnels, comme l'ancien `ipcRenderer.invoke`.
 *
 * À importer en tout premier dans main.tsx : le thème initial est lu avant
 * le premier rendu.
 */
import type { BatlayBridge } from "@/types/global";
import type { ResolvedTheme, ThemePreference } from "../../shared/theme";
import { lookupArtwork } from "@/services/artwork-lookup";
import { nativeInitial, nativeInvoke, nativeListen, type NativeInvoke } from "./native";

/** Octets -> base64, par tranches (pas de dépassement de pile sur une image de 12 Mo). */
export function bytesToBase64(data: Uint8Array): string {
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < data.length; i += chunk) {
    binary += String.fromCharCode(...data.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function initialTheme(): { preference: ThemePreference; resolved: ResolvedTheme } {
  const theme = nativeInitial()?.theme;
  const preference: ThemePreference = theme?.preference === "light" || theme?.preference === "system" ? theme.preference : "dark";
  const resolved: ResolvedTheme = theme?.resolved === "light" ? "light" : "dark";
  return { preference, resolved };
}

export function createBridge(invoke: NativeInvoke): BatlayBridge {
  // Typage relâché ici seulement : la forme des réponses est décrite par BatlayBridge.
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  const call = (cmd: string, ...args: unknown[]): Promise<any> => invoke(cmd, args);
  const on = <T,>(channel: string) => (callback: (payload: T) => void) => nativeListen<T>(channel, callback);

  return {
    config: {
      get: () => call("config:get"),
      setSettings: (settings) => call("config:set-settings", settings),
      getOverlays: () => call("config:get-overlays"),
      saveOverlays: (overlays) => call("config:save-overlays", overlays),
      getActiveOverlayId: () => call("config:get-active-overlay-id"),
      setActiveOverlayId: (id) => call("config:set-active-overlay-id", id),
    },
    overlay: {
      getUrl: (overlayId) => call("overlay:get-url", overlayId),
      isRunning: () => call("overlay:is-running"),
      getClientCounts: () => call("overlay:get-client-counts"),
      // Envoi sans réponse attendue (comme ipcRenderer.send) : jamais d'erreur remontée au polling.
      broadcastState: (state) => {
        void call("overlay:broadcast-state", state).catch(() => undefined);
      },
      onServerError: on("overlay-server:error"),
    },
    integrations: {
      openDocs: (platformId) => call("integrations:open-docs", platformId),
      checkLocalAlias: () => call("integrations:check-local-alias"),
      setupLocalAlias: () => call("integrations:setup-local-alias"),
      removeLocalAlias: () => call("integrations:remove-local-alias"),
      onLocalAliasChange: on("integrations:alias-changed"),
    },
    alerts: {
      get: () => call("alerts:get"),
      save: (config) => call("alerts:save", config),
      test: (kind) => call("alerts:test", kind),
      skip: () => call("alerts:skip"),
      regenerateKey: () => call("alerts:regenerate-key"),
      // Le type réel du fichier est revérifié par le cœur (octets, pas l'extension).
      importMedia: (data, expected) => call("alerts:import-media", bytesToBase64(data), expected),
      mediaDataUrl: (id) => call("alerts:media-data-url", id),
      listMedia: (kind) => call("alerts:list-media", kind),
    },
    live: {
      getSnapshot: () => call("live:get-snapshot"),
      reset: () => call("live:reset"),
      replay: (entryId) => call("live:replay", entryId),
      openWindow: () => call("live:open-window"),
      setAlwaysOnTop: (enabled) => call("live:set-always-on-top", enabled === true),
      onUpdate: on("live:update"),
    },
    tiktok: {
      getState: () => call("tiktok:get-state"),
      connect: (username) => call("tiktok:connect", username),
      disconnect: () => call("tiktok:disconnect"),
      login: () => call("tiktok:login"),
      logout: () => call("tiktok:logout"),
      updateSettings: (patch) => call("tiktok:update-settings", patch),
      diagnose: () => call("tiktok:diagnose"),
      setEulerKey: (key) => call("tiktok:set-euler-key", typeof key === "string" ? key : null),
      showPage: (show) => call("tiktok:show-page", show === true),
      onState: on("tiktok:state"),
      onPageVisible: (callback) => nativeListen<unknown>("tiktok:page-visible", (visible) => callback(visible === true)),
    },
    spotify: {
      isConfigured: () => call("spotify:is-configured"),
      setClientId: (clientId) => call("spotify:set-client-id", clientId),
      connect: () => call("spotify:connect"),
      disconnect: () => call("spotify:disconnect"),
      isSessionRestorable: () => call("spotify:is-session-restorable"),
      restoreSession: () => call("spotify:restore-session"),
      getCurrentlyPlaying: () => call("spotify:get-currently-playing"),
    },
    twitch: {
      getState: () => call("twitch:get-state"),
      connect: () => call("twitch:connect"),
      cancel: () => call("twitch:cancel"),
      reauthorize: () => call("twitch:reauthorize"),
      disconnect: () => call("twitch:disconnect"),
      setChannel: (channel) => call("twitch:set-channel", channel),
      sendTest: () => call("twitch:send-test"),
      restartOverlay: () => call("twitch:restart-overlay"),
      onState: on("twitch:state"),
    },
    theme: {
      // Synchrone : posé par l'application avant le chargement de la page.
      getInitial: initialTheme,
      onChange: on("theme:changed"),
    },
    update: {
      getStatus: () => call("update:get-status"),
      check: () => call("update:check"),
      download: () => call("update:download"),
      install: () => call("update:install"),
      openReleases: () => call("update:open-releases"),
      getVersion: () => call("update:get-version"),
      onStatus: on("update:status"),
    },
    artwork: {
      // Recherche faite ici ; le réseau passe par le cœur (sources autorisées seulement).
      lookup: (artist, title) => lookupArtwork(artist, title),
    },
    systemMedia: {
      isSupported: () => call("system-media:is-supported"),
      listSessions: () => call("system-media:list-sessions"),
      getCurrent: (options) => call("system-media:get-current", options ?? {}),
    },
  };
}

const invoke = nativeInvoke();
if (invoke && typeof window !== "undefined") {
  Object.defineProperty(window, "batlay", { value: Object.freeze(createBridge(invoke)), writable: false, configurable: false });
}
