import { useEffect } from "react";
import { create } from "zustand";
import { isImageMediaId } from "../../shared/alerts";
import type { OverlayMediaEnv } from "@/components/OverlayMedia";

/**
 * Images des overlays dans le TABLEAU DE BORD (éditeur, miniatures) : sa CSP
 * n'autorise pas le serveur local, l'image arrive donc en data URL par le
 * cœur de Batlay, mise en cache ici. La page OBS n'utilise pas ce module.
 */

interface MediaCacheState {
  urls: Record<string, string>;
  pending: Record<string, true>;
  load: (id: string) => void;
}

/** Cache des data URL (tableau de bord seulement). */
export const useMediaCache = create<MediaCacheState>((set, get) => ({
  urls: {},
  pending: {},
  load: (id) => {
    if (get().urls[id] || get().pending[id] || !isImageMediaId(id)) return;
    set((s) => ({ pending: { ...s.pending, [id]: true } }));
    void window.batlay.alerts
      .mediaDataUrl(id)
      .then((url) => url && set((s) => ({ urls: { ...s.urls, [id]: url } })))
      .catch(() => undefined);
  },
}));

function useDashboardUrl(id: string | undefined): string | null {
  const url = useMediaCache((s) => (id ? s.urls[id] : undefined));
  const load = useMediaCache((s) => s.load);
  useEffect(() => {
    if (id && !url) load(id);
  }, [id, url, load]);
  return url ?? null;
}

export const DASHBOARD_MEDIA_ENV: OverlayMediaEnv = { useMediaUrl: useDashboardUrl, editing: false };
export const EDITOR_MEDIA_ENV: OverlayMediaEnv = { useMediaUrl: useDashboardUrl, editing: true };

