import { useEffect, useRef, useState } from "react";
import { OverlayPreview } from "@/components/OverlayPreview";
import type { OverlayConfig } from "@/types/overlay";
import type { PlaybackState } from "@/types/track";
import { ProgressClock, feedClock } from "@/utils/progress-clock";
import { computeFitScale, parseOverlayDisplayParams } from "@/integrations/urls";
import { AnimationProvider } from "@/animation/react";
import { isStalePlayback } from "@/utils/playback-order";
import { LiveStatsContext } from "@/components/OverlayMedia";
import { EMPTY_LIVE_STATS, normalizeLiveStats, type LiveStats } from "../shared/live";

function getOverlayIdFromPath(): string | null {
  // Ex: /overlay/abc123 -> "abc123"
  const match = window.location.pathname.match(/\/overlay\/([^/?]+)/);
  return match ? match[1] : null;
}

/**
 * Page réellement chargée par OBS Browser Source. Elle ne contient
 * aucun dashboard, aucun menu, aucun fond opaque — uniquement l'overlay,
 * mis à jour en temps réel via WebSocket sans rechargement de page
 * (voir section "Mise à jour temps réel" du brief).
 *
 * Progression : la même ProgressClock que le Dashboard (src/utils/
 * progress-clock.ts) interpole la position entre deux mesures du provider et
 * absorbe les écarts sans saut ; les composants live la lisent à chaque
 * image d'animation. L'état React ne change qu'à l'arrivée d'un message.
 */
/**
 * `?fit=contain` (facultatif, voir src/integrations/urls.ts) : le canvas est
 * mis à l'échelle pour tenir dans la fenêtre de la source, pour les logiciels
 * qui ne laissent pas toujours fixer cette taille (source « Link » de TikTok
 * LIVE Studio…). Sans paramètre : taille native, comme toujours.
 */
const display = parseOverlayDisplayParams(window.location.search);

function useViewportSize(enabled: boolean): { width: number; height: number } {
  const [size, setSize] = useState(() => ({ width: window.innerWidth, height: window.innerHeight }));
  useEffect(() => {
    if (!enabled) return;
    const onResize = () => setSize({ width: window.innerWidth, height: window.innerHeight });
    onResize();
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [enabled]);
  return size;
}

export function OverlayApp() {
  const overlayId = getOverlayIdFromPath();
  const viewport = useViewportSize(display.fit === "contain");
  const [config, setConfig] = useState<OverlayConfig | null>(null);
  const [playback, setPlayback] = useState<PlaybackState>({ track: null, isPlaying: false, updatedAt: 0 });
  const [error, setError] = useState<string | null>(null);
  // Infos du live (dernier follower, totaux…) : vides tant que Batlay n'a rien reçu.
  const [liveStats, setLiveStats] = useState<LiveStats>(EMPTY_LIVE_STATS);
  const wsRef = useRef<WebSocket | null>(null);
  const clockRef = useRef<ProgressClock>(new ProgressClock());
  const lastStateRef = useRef<PlaybackState | null>(null);

  useEffect(() => {
    if (!overlayId) {
      setError("overlayId manquant dans l'URL.");
      return;
    }

    fetch(`/api/overlays/${overlayId}`)
      .then((res) => {
        if (!res.ok) throw new Error("Overlay introuvable.");
        return res.json();
      })
      .then(setConfig)
      .catch((err) => setError((err as Error).message));

    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
    let closedIntentionally = false;

    function connect() {
      const ws = new WebSocket(`ws://${window.location.host}/ws/${overlayId}`);
      wsRef.current = ws;
      ws.onmessage = (event) => {
        // JSON.parse d'un message texte WebSocket : toujours de l'UTF-8.
        const msg = JSON.parse(event.data);
        if (msg.type === "state") {
          const payload = msg.payload as PlaybackState;
          // Mesure plus ancienne que celle affichée : ignorée (l'ancien morceau ne doit jamais revenir).
          if (isStalePlayback(lastStateRef.current, payload)) return;
          lastStateRef.current = payload;
          // `sentAt` = horloge de Batlay à l'envoi ; avec `updatedAt` (même
          // horloge) on connaît l'âge exact de la mesure sans dépendre de
          // l'horloge de cette page.
          feedClock(clockRef.current, payload, typeof msg.sentAt === "number" ? msg.sentAt : undefined);
          setPlayback(payload);
        } else if (msg.type === "live") {
          setLiveStats(normalizeLiveStats(msg.payload));
        } else if (msg.type === "config") {
          // Sauvegarde dans l'éditeur : appliquée en direct, sans recharger la source OBS.
          setConfig(msg.payload as OverlayConfig);
        }
      };
      // Reconnexion automatique si Batlay redémarre (ex. après un crash ou
      // un changement de port) — l'overlay ne doit jamais rester figé.
      ws.onclose = () => {
        if (closedIntentionally) return;
        reconnectTimer = setTimeout(connect, 2000);
      };
    }
    connect();

    return () => {
      closedIntentionally = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      wsRef.current?.close();
    };
  }, [overlayId]);

  if (error) return null; // une overlay OBS ne doit jamais afficher un message d'erreur visible au public
  if (!config) return null;

  const scale =
    display.fit === "contain"
      ? computeFitScale(viewport.width, viewport.height, config.theme.canvasWidth, config.theme.canvasHeight)
      : "1:1";
  // Animations autorisées ici (et dans l'éditeur) ; les réglages viennent de la config de l'overlay,
  // poussée en direct par le serveur : changer un effet ne recharge pas la source.
  return (
    <AnimationProvider>
      <LiveStatsContext.Provider value={liveStats}>
        <OverlayPreview overlay={config} track={playback.track} reader={clockRef.current} scale={scale} />
      </LiveStatsContext.Provider>
    </AnimationProvider>
  );
}
