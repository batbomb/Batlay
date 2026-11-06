import type { PlaybackState } from "./track";
import type { SpotifyItemLike } from "@/services/spotify-track";
import type { Settings } from "@/stores/useSettingsStore";
import type { ResolvedTheme, ThemePreference } from "../../shared/theme";
import type { UpdateStatus } from "../../shared/update";
import type { TwitchState } from "../../shared/twitch";
import type { AlertKind, AlertsConfig } from "../../shared/alerts";
import type { TikTokDiagnostic, TikTokLiveSettings, TikTokLiveState, TikTokMode } from "../../shared/tiktok";
import type { LiveSnapshot, LiveUpdate } from "../../shared/live";

export type LocalAliasStatus =
  | { state: "ok"; hostname: string }
  | { state: "missing"; hostname: string }
  | { state: "wrong-address"; hostname: string; addresses: string[] }
  | { state: "error"; hostname: string; message: string };

export type LocalAliasOutcome =
  | { result: "done" | "already" | "cancelled" | "unsupported"; status: LocalAliasStatus }
  | { result: "failed"; status: LocalAliasStatus; message: string };

export interface BatlayBridge {
  config: {
    get(): Promise<{ settings: Settings; [key: string]: unknown }>;
    setSettings(settings: Partial<Settings>): Promise<Settings>;
    getOverlays(): Promise<unknown[]>;
    saveOverlays(overlays: unknown[]): Promise<boolean>;
    getActiveOverlayId(): Promise<string | null>;
    setActiveOverlayId(id: string | null): Promise<boolean>;
  };
  overlay: {
    getUrl(overlayId: string): Promise<string>;
    isRunning(): Promise<boolean>;
    /** Sources réellement connectées (WebSocket ouvert), par id d'overlay. Overlays sans source absents. */
    getClientCounts(): Promise<Record<string, number>>;
    broadcastState(state: PlaybackState): void;
    onServerError(callback: (payload: { message: string }) => void): () => void;
  };
  integrations: {
    /** Ouvre la documentation officielle du logiciel (adresse choisie par le cœur de Batlay). `false` si aucune. */
    openDocs(platformId: string): Promise<boolean>;
    checkLocalAlias(): Promise<LocalAliasStatus>;
    setupLocalAlias(): Promise<LocalAliasOutcome>;
    removeLocalAlias(): Promise<LocalAliasOutcome>;
    onLocalAliasChange(callback: (status: LocalAliasStatus) => void): () => void;
  };
  alerts: {
    get(): Promise<{ config: AlertsConfig; urls: { page: string; hook: string }; pages: number }>;
    save(config: AlertsConfig): Promise<AlertsConfig>;
    test(kind: AlertKind): Promise<{ shown: boolean; reason?: "invalid" | "filtered" | "rate" | "no-source" }>;
    skip(): Promise<boolean>;
    regenerateKey(): Promise<{ page: string; hook: string }>;
    importMedia(data: Uint8Array, expected: "image" | "sound"): Promise<{ ok: true; id: string; kind: "image" | "sound" } | { ok: false; reason: string }>;
    mediaDataUrl(id: string): Promise<string | null>;
    listMedia(kind: "image" | "sound"): Promise<string[]>;
  };
  live: {
    getSnapshot(): Promise<LiveSnapshot>;
    /** Remet à zéro les compteurs et le journal (les chats restent). */
    reset(): Promise<LiveSnapshot>;
    replay(entryId: string): Promise<{ shown: boolean }>;
    /** Ouvre (ou met au premier plan) la fenêtre Live détachée. */
    openWindow(): Promise<boolean>;
    /** Fenêtre Live détachée seulement. Renvoie l'état réel. */
    setAlwaysOnTop(on: boolean): Promise<boolean>;
    onUpdate(callback: (update: LiveUpdate) => void): () => void;
  };
  tiktok: {
    getState(): Promise<TikTokLiveState>;
    /** Enregistre le pseudo et attend le live. `false` si le pseudo n'est pas valable. */
    connect(username: string): Promise<boolean>;
    disconnect(): Promise<boolean>;
    login(): Promise<boolean>;
    logout(): Promise<boolean>;
    updateSettings(patch: { giftMinDiamonds?: number; mode?: TikTokMode }): Promise<TikTokLiveSettings>;
    /** Enregistre (chiffrée par Windows) la clé API d'Euler Stream, ou l'efface (`null`). Faux si refusée. */
    setEulerKey(key: string | null): Promise<boolean>;
    diagnose(): Promise<{ diagnostic: TikTokDiagnostic; report: string }>;
    /** Montre / remasque la page du live ouverte en arrière-plan (mode navigateur). Faux si aucune page n'est ouverte. */
    showPage(show: boolean): Promise<boolean>;
    onState(callback: (state: TikTokLiveState) => void): () => void;
    onPageVisible(callback: (visible: boolean) => void): () => void;
  };
  spotify: {
    isConfigured(): Promise<boolean>;
    setClientId(clientId: string): Promise<boolean>;
    connect(): Promise<boolean>;
    disconnect(): Promise<boolean>;
    isSessionRestorable(): Promise<boolean>;
    restoreSession(): Promise<boolean>;
    getCurrentlyPlaying(): Promise<{
      isPlaying: boolean;
      progressMs: number;
      /**
       * Instant (Date.now(), cœur de Batlay) où Spotify a mesuré
       * `progressMs` : milieu de l'aller-retour réseau. Optionnel pour
       * rester compatible avec d'anciens mocks de test.
       */
      sampledAt?: number;
      item: SpotifyItemLike | null;
    } | null>;
  };
  twitch: {
    getState(): Promise<TwitchState | null>;
    /** Ouvre le navigateur sur Twitch. Le résultat (succès ou erreur) arrive par `onState`, jamais par exception. */
    connect(): Promise<boolean>;
    cancel(): Promise<boolean>;
    reauthorize(): Promise<boolean>;
    disconnect(): Promise<boolean>;
    /** Choisit la chaîne à afficher (pseudo ou lien Twitch) ; `null` = la sienne. `false` si la saisie n'est pas reconnue. */
    setChannel(channel: string | null): Promise<boolean>;
    /** Affiche un faux message dans les sources OBS connectées. */
    sendTest(): Promise<boolean>;
    /** Redémarre le lien OBS du chat (les sources Navigateur rechargent la page). Renvoie le nombre de sources prévenues. */
    restartOverlay(): Promise<number>;
    onState(callback: (state: TwitchState) => void): () => void;
  };
  theme: {
    /** Thème déjà résolu par le cœur de Batlay (lecture synchrone, avant le premier rendu). */
    getInitial(): { preference: ThemePreference; resolved: ResolvedTheme };
    /** Notifié quand le réglage change ou que Windows bascule (réglage « system »). */
    onChange(callback: (payload: { preference: ThemePreference; resolved: ResolvedTheme }) => void): () => void;
  };
  update: {
    getStatus(): Promise<UpdateStatus>;
    check(): Promise<UpdateStatus>;
    download(): Promise<UpdateStatus>;
    /** Quitte Batlay, installe la version téléchargée, puis le relance. */
    install(): Promise<boolean>;
    openReleases(): Promise<boolean>;
    getVersion(): Promise<string>;
    onStatus(callback: (status: UpdateStatus) => void): () => void;
  };
  artwork: {
    /** (artiste, titre) -> URL de pochette. `url: null` si introuvable — jamais d'exception. */
    lookup(
      artist: string,
      title: string
    ): Promise<{
      url: string | null;
      source: "itunes" | "deezer" | "listenbrainz" | "coverartarchive" | "audius" | "none";
      cached: boolean;
    }>;
  };
  systemMedia: {
    isSupported(): Promise<boolean>;
    listSessions(): Promise<{ sourceAppId: string | null; title: string; artist: string }[]>;
    getCurrent(options?: { preferredAppId?: string; includeArtwork?: boolean }): Promise<{
      title: string;
      artist: string;
      /** Diagnostic seulement (journal debug) : jamais utilisé pour l'affichage. Optionnel pour les anciens mocks. */
      albumArtist?: string | null;
      /** Titre de la fenêtre Spotify Desktop (voir src/utils/spotify-metadata.ts). Optionnel pour les anciens mocks. */
      windowTitle?: string | null;
      album: string | null;
      sourceAppId: string | null;
      isPlaying: boolean;
      playbackStatus?: "playing" | "paused" | "stopped" | "unknown";
      /** Position déjà extrapolée à l'instant `sampledAtMs` (voir crates/batlay-core/src/system_media.rs). */
      positionMs: number;
      /** Instant (Date.now()) où Windows a été interrogé. Optionnel pour les anciens mocks. */
      sampledAtMs?: number;
      durationMs: number;
      artwork: string | null;
      hasThumbnail: boolean;
      artworkError: string | null;
    } | null>;
  };
}

declare global {
  interface Window {
    batlay: BatlayBridge;
  }
}
