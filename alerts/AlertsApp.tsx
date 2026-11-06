import { useEffect, useReducer, useState } from "react";
import {
  ALERTS_WS_PATH,
  ALERT_MEDIA_PREFIX,
  DEFAULT_ALERTS_CONFIG,
  normalizeAlertsConfig,
  shouldShowAlert,
  type AlertsConfig,
  type AlertsOverlayMessage,
} from "../shared/alerts";
import { alertQueueReducer, emptyQueue } from "../src/alerts/queue";
import { AlertPlayer } from "../src/alerts/AlertPlayer";

/**
 * Page des alertes chargée par la source Web (OBS, TikTok LIVE Studio…) :
 * http://batlay.test:3000/alerts. Fond transparent, une alerte à la fois,
 * centrée. Réglages reçus en direct par WebSocket (aucun rechargement).
 * Ne connaît aucun jeton : elle ne reçoit que des événements déjà validés.
 */

const mediaUrl = (id: string) => `${ALERT_MEDIA_PREFIX}${encodeURIComponent(id)}`;

export function AlertsApp() {
  const [config, setConfig] = useState<AlertsConfig>(DEFAULT_ALERTS_CONFIG);
  const [queue, dispatch] = useReducer(alertQueueReducer, undefined, emptyQueue);
  const [gap, setGap] = useState(false);

  useEffect(() => {
    let socket: WebSocket | null = null;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let closedByUs = false;
    let attempt = 0;
    function connect() {
      socket = new WebSocket(`ws://${window.location.host}${ALERTS_WS_PATH}`);
      socket.onopen = () => {
        attempt = 0;
      };
      socket.onmessage = (e) => {
        let msg: AlertsOverlayMessage;
        try {
          msg = JSON.parse(String(e.data)) as AlertsOverlayMessage;
        } catch {
          return;
        }
        if (msg.type === "config") setConfig(normalizeAlertsConfig(msg.payload));
        else if (msg.type === "alert" && msg.payload && typeof msg.payload.id === "string") dispatch({ type: "enqueue", event: msg.payload });
        else if (msg.type === "skip") dispatch({ type: "skip" });
      };
      socket.onclose = () => {
        if (closedByUs) return;
        timer = setTimeout(connect, Math.min(10_000, 1_000 * 2 ** attempt++));
      };
    }
    connect();
    return () => {
      closedByUs = true;
      if (timer) clearTimeout(timer);
      socket?.close();
    };
  }, []);

  // Pause entre deux alertes, puis la suivante.
  useEffect(() => {
    if (!gap) return;
    const t = setTimeout(() => {
      setGap(false);
      dispatch({ type: "done" });
    }, config.gapMs);
    return () => clearTimeout(t);
  }, [gap, config.gapMs]);

  const current = queue.current;
  if (!current || gap) return null;
  // Type désactivé pendant que l'alerte attendait : on passe.
  if (!shouldShowAlert(config, current)) {
    queueMicrotask(() => dispatch({ type: "done" }));
    return null;
  }
  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", alignItems: "center", justifyContent: "center", pointerEvents: "none" }}>
      <AlertPlayer key={current.id} event={current} style={config.kinds[current.kind]} mediaUrl={mediaUrl} onDone={() => setGap(true)} />
    </div>
  );
}
