import { useEffect, useState } from "react";
import { ImagePlus } from "lucide-react";
import { OverlayImage, OverlayMediaContext } from "@/components/OverlayMedia";
import { EDITOR_MEDIA_ENV } from "@/components/media-cache";

/**
 * « Mes images » : les images déjà importées dans Batlay (overlays et alertes),
 * à réutiliser d'un clic sans rechercher le fichier.
 */
export function ImageGallery({ refreshKey, onPick, onImport }: { refreshKey: number; onPick: (mediaId: string) => void; onImport: () => void }) {
  const [ids, setIds] = useState<string[] | null>(null);
  useEffect(() => {
    let alive = true;
    void window.batlay.alerts
      .listMedia("image")
      .then((list) => alive && setIds(list))
      .catch(() => alive && setIds([]));
    return () => {
      alive = false;
    };
  }, [refreshKey]);

  return (
    <div className="mt-3">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-xs font-medium text-muted">Mes images</span>
        <button type="button" onClick={onImport} className="inline-flex items-center gap-1 text-[11px] text-accent hover:underline">
          <ImagePlus size={12} /> Importer
        </button>
      </div>
      {ids === null ? (
        <div className="grid grid-cols-4 gap-1.5">
          {[0, 1, 2, 3].map((i) => (
            <div key={i} className="skeleton aspect-square rounded-md" />
          ))}
        </div>
      ) : ids.length === 0 ? (
        <p className="rounded-lg border border-dashed border-base-700 px-2 py-3 text-center text-[11px] text-muted">
          Aucune image pour l'instant. Glissez un fichier sur le canvas, collez-le (Ctrl+V) ou cliquez sur « Importer ».
        </p>
      ) : (
        <OverlayMediaContext.Provider value={EDITOR_MEDIA_ENV}>
          <div className="grid max-h-48 grid-cols-4 gap-1.5 overflow-y-auto pr-0.5">
            {ids.map((id) => (
              <button
                key={id}
                type="button"
                onClick={() => onPick(id)}
                title="Ajouter cette image"
                className="relative aspect-square overflow-hidden rounded-md border border-base-700 bg-stage transition hover:border-signal-500"
              >
                <OverlayImage mediaId={id} fit="contain" />
              </button>
            ))}
          </div>
        </OverlayMediaContext.Provider>
      )}
    </div>
  );
}
