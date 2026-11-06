import { useMediaCache } from "@/components/media-cache";

/**
 * Importe une image choisie, glissée ou collée : contrôle de taille ici, type
 * réel vérifié par le cœur de Batlay (contenu du fichier, pas son nom),
 * puis taille naturelle lue pour garder les proportions.
 */
export const MAX_IMAGE_BYTES = 8 * 1024 * 1024;
export const IMAGE_ACCEPT = "image/png,image/jpeg,image/gif,image/webp";

export type ImportedImage = { id: string; width: number; height: number };

function naturalSize(url: string): Promise<{ width: number; height: number }> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => resolve({ width: img.naturalWidth || 320, height: img.naturalHeight || 320 });
    img.onerror = () => resolve({ width: 320, height: 320 });
    img.src = url;
  });
}

export async function importImageFile(file: File): Promise<ImportedImage> {
  if (file.size > MAX_IMAGE_BYTES) throw new Error("Image trop lourde (8 Mo maximum).");
  const r = await window.batlay.alerts.importMedia(new Uint8Array(await file.arrayBuffer()), "image");
  if (!r.ok) throw new Error(r.reason);
  const url = await window.batlay.alerts.mediaDataUrl(r.id);
  if (url) useMediaCache.setState((s) => ({ urls: { ...s.urls, [r.id]: url } }));
  const size = url ? await naturalSize(url) : { width: 320, height: 320 };
  return { id: r.id, ...size };
}

/** Image déjà importée (« Mes images ») : taille naturelle relue, mise en cache pour l'aperçu. */
export async function loadExistingImage(id: string): Promise<ImportedImage> {
  const url = useMediaCache.getState().urls[id] ?? (await window.batlay.alerts.mediaDataUrl(id));
  if (!url) throw new Error("Image introuvable dans le dossier de Batlay.");
  useMediaCache.setState((s) => ({ urls: { ...s.urls, [id]: url } }));
  return { id, ...(await naturalSize(url)) };
}

/** Premier fichier image d'un glisser-déposer ou d'un collage. */
export function firstImageFile(list: FileList | DataTransferItemList | null | undefined): File | null {
  if (!list) return null;
  for (const item of Array.from(list as ArrayLike<File | DataTransferItem>)) {
    const file = item instanceof File ? item : item.kind === "file" ? item.getAsFile() : null;
    if (file && /^image\/(png|jpeg|gif|webp)$/.test(file.type)) return file;
  }
  return null;
}
