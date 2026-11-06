import { createContext, useContext, useEffect, useState, type CSSProperties } from "react";
import { ALERT_MEDIA_PREFIX, isImageMediaId } from "../../shared/alerts";
import type { ClockFormat, OverlayComponentConfig } from "@/types/overlay";
import { GOAL_METRIC_LABELS, formatCountdown, goalProgress } from "@/utils/overlay-model";
import { SAMPLE_LIVE_STATS, formatCount, liveTemplateVars, type LiveStats } from "../../shared/live";

/**
 * Images des overlays personnalisés.
 *
 * - Page OBS (servie par le serveur d'overlay) : l'image est lue sur le même
 *   serveur, `/api/alert-media/<id>` (dossier des médias de Batlay, contenu vérifié à l'import).
 * - Tableau de bord (éditeur, miniatures) : sa CSP n'autorise pas le serveur
 *   local, l'image arrive en data URL par le cœur de Batlay (mise en cache).
 */

export interface OverlayMediaEnv {
  useMediaUrl: (id: string | undefined) => string | null;
  /** Éditeur : une image manquante est montrée comme un emplacement à remplir (jamais dans OBS). */
  editing: boolean;
}

const useServerUrl = (id: string | undefined) => (id && isImageMediaId(id) ? `${ALERT_MEDIA_PREFIX}${id}` : null);

export const OverlayMediaContext = createContext<OverlayMediaEnv>({ useMediaUrl: useServerUrl, editing: false });

const FILL: CSSProperties = { position: "absolute", inset: 0, width: "100%", height: "100%" };

export function OverlayImage({ mediaId, fit, radius }: { mediaId?: string; fit?: "cover" | "contain"; radius?: number }) {
  const env = useContext(OverlayMediaContext);
  const url = env.useMediaUrl(mediaId);
  if (url) return <img src={url} alt="" draggable={false} style={{ ...FILL, objectFit: fit ?? "contain", borderRadius: radius, display: "block" }} />;
  if (!env.editing) return null;
  // Emplacement vide (éditeur) : invite à choisir une image.
  return (
    <div
      style={{
        ...FILL,
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        border: "2px dashed rgba(155,107,255,0.7)",
        borderRadius: radius ?? 12,
        background: "rgba(155,107,255,0.08)",
        color: "rgba(255,255,255,0.8)",
        // Taille relative au cadre : lisible même quand l'éditeur est zoomé à 25 %.
        containerType: "size",
        font: "600 clamp(12px, 9cqmin, 72px)/1.2 system-ui, sans-serif",
        textAlign: "center",
        padding: 8,
        boxSizing: "border-box",
      }}
    >
      {mediaId ? "Chargement…" : "Glissez une image ici"}
    </div>
  );
}

const CLOCK_OPTIONS: Record<ClockFormat, Intl.DateTimeFormatOptions> = {
  time: { hour: "2-digit", minute: "2-digit" },
  timeSeconds: { hour: "2-digit", minute: "2-digit", second: "2-digit" },
  date: { weekday: "long", day: "numeric", month: "long" },
  dateTime: { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" },
};

export function formatClock(date: Date, format: ClockFormat = "time"): string {
  const text = new Intl.DateTimeFormat("fr-FR", CLOCK_OPTIONS[format] ?? CLOCK_OPTIONS.time).format(date);
  return format === "date" ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}

/** Heure ou date, mise à jour chaque seconde (le texte ne change que si l'affichage change). */
export function ClockText({ format }: { format?: ClockFormat }) {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const t = setInterval(() => setNow(new Date()), 1000);
    return () => clearInterval(t);
  }, []);
  return <>{formatClock(now, format)}</>;
}

/**
 * Infos du live pour les overlays : valeurs d'exemple par défaut (éditeur,
 * miniatures) ; la page OBS fournit les vraies, reçues du serveur local.
 */
export const LiveStatsContext = createContext<LiveStats>(SAMPLE_LIVE_STATS);

export function useLiveVars(): Record<string, string> {
  const stats = useContext(LiveStatsContext);
  return liveTemplateVars(stats);
}

/**
 * Compte à rebours : démarre quand la page s'affiche (ouverture de la source
 * dans OBS) ou quand la durée change ; à zéro, affiche le texte de fin.
 */
export function CountdownText({ seconds, endText }: { seconds: number; endText: string }) {
  const [end, setEnd] = useState(() => Date.now() + seconds * 1000);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    setEnd(Date.now() + seconds * 1000);
  }, [seconds]);
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 250);
    return () => clearInterval(t);
  }, []);
  const remaining = end - now;
  return <>{remaining <= 0 && endText ? endText : formatCountdown(remaining)}</>;
}

/** Barre d'objectif : libellé et « 12 / 20 » au-dessus, barre en dessous. Valeur = infos du live. */
export function GoalBar({ component }: { component: OverlayComponentConfig }) {
  const stats = useContext(LiveStatsContext);
  const s = component.style;
  const metric = s.goalMetric && s.goalMetric in GOAL_METRIC_LABELS ? s.goalMetric : "followers";
  const target = Math.max(1, Math.round(s.goalTarget ?? 20));
  const value = stats[metric] ?? 0;
  const ratio = goalProgress(value, target);
  const radius = s.borderRadius ?? 10;
  return (
    <div style={{ position: "absolute", inset: 0, display: "flex", flexDirection: "column", justifyContent: "space-between", gap: 6 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "baseline", gap: 12, whiteSpace: "nowrap" }}>
        <span style={{ overflow: "hidden", textOverflow: "ellipsis" }}>{component.content?.trim() || `Objectif ${GOAL_METRIC_LABELS[metric].toLowerCase()}`}</span>
        <span style={{ fontVariantNumeric: "tabular-nums" }}>
          {formatCount(Math.min(value, 99_999_999))} / {formatCount(target)}
        </span>
      </div>
      <div style={{ position: "relative", flex: "0 0 38%", minHeight: 6, borderRadius: radius, overflow: "hidden", backgroundColor: s.trackColor ?? "rgba(255,255,255,0.15)" }}>
        <div
          style={{
            position: "absolute",
            inset: 0,
            width: `${ratio * 100}%`,
            borderRadius: radius,
            backgroundColor: s.fillColor ?? "#9B6BFF",
            transition: "width 600ms cubic-bezier(0.16, 1, 0.3, 1)",
          }}
        />
      </div>
    </div>
  );
}
