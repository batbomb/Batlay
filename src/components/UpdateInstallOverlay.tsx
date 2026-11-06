import { BrandMark } from "@/components/BrandMark";
import { useUpdateStore } from "@/stores/useUpdateStore";

/**
 * Écran « Installation de la mise à jour » (Batlay 0.5) : il remplace la
 * fenêtre de l'installateur Windows. L'installateur tourne en silence ;
 * Batlay se ferme, puis se rouvre tout seul une fois la mise à jour posée.
 */
export function UpdateInstallOverlay() {
  const installing = useUpdateStore((s) => s.installing);
  const status = useUpdateStore((s) => s.status);
  if (!installing) return null;
  const version = status.state === "downloaded" ? status.version : null;

  return (
    <div className="modal-backdrop fixed inset-0 z-[60] flex items-center justify-center bg-base-950/85 backdrop-blur-xl" role="alertdialog" aria-modal="true" aria-label="Installation de la mise à jour">
      <div
        className="pointer-events-none absolute inset-0"
        aria-hidden="true"
        style={{ background: "radial-gradient(40rem 26rem at 50% 30%, rgb(var(--signal-600) / 0.28), transparent 65%), radial-gradient(30rem 20rem at 70% 70%, rgb(var(--nova) / 0.14), transparent 70%)" }}
      />
      <div className="modal-panel relative flex w-[420px] max-w-[90vw] flex-col items-center text-center">
        <BrandMark className="h-20 w-20 shadow-glow" playing />
        <h2 className="mt-6 font-display text-2xl font-semibold tracking-[-0.02em] text-fg">Installation de la mise à jour</h2>
        <p className="mt-2 text-sm text-muted">
          {version ? `Batlay ${version} s'installe.` : "La nouvelle version s'installe."} Batlay va se fermer, puis se rouvrir tout seul dans quelques secondes.
        </p>
        <div className="mt-6 h-1.5 w-64 overflow-hidden rounded-full bg-base-800">
          <div className="update-indeterminate h-full w-1/3 rounded-full" />
        </div>
        <p className="mt-6 text-[11px] text-faint">Vos overlays, réglages et connexions sont conservés. S'il ne se rouvre pas d'ici une minute, lancez Batlay depuis le menu Démarrer.</p>
      </div>
    </div>
  );
}
