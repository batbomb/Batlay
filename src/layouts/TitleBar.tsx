import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ArrowUpCircle, Search } from "lucide-react";
import { BrandMark } from "@/components/BrandMark";
import { openCommandPalette } from "@/components/CommandPalette";
import { useUpdateStore } from "@/stores/useUpdateStore";
import { currentWindow, type AppWindow } from "@/platform/window-controls";
import { appVersion, shortVersion } from "@/utils/app-version";

/**
 * Barre de titre de Batlay 0.5, à la place de celle de Windows : marque et
 * version, recherche (Ctrl+K), mise à jour disponible, boutons de la fenêtre.
 * Tout ce qui n'est pas un bouton sert à déplacer la fenêtre (double-clic :
 * agrandir / restaurer), comme une barre de titre classique.
 */
export function TitleBar({ variant = "main", title }: { variant?: "main" | "live"; title?: string }) {
  const [win] = useState<AppWindow | null>(() => currentWindow());
  const [maximized, setMaximized] = useState(false);
  const status = useUpdateStore((s) => s.status);
  const updateReady = variant === "main" && (status.state === "available" || status.state === "downloaded");
  const updateVersion = status.state === "available" || status.state === "downloaded" ? status.version : "";
  const version = appVersion();

  useEffect(() => {
    if (!win) return;
    let alive = true;
    let off: (() => void) | null = null;
    const refresh = () => void win.isMaximized().then((m) => alive && setMaximized(m)).catch(() => undefined);
    refresh();
    void win
      .onResized(refresh)
      .then((u) => (alive ? (off = u) : u()))
      .catch(() => undefined);
    return () => {
      alive = false;
      off?.();
    };
  }, [win]);

  return (
    <header className="titlebar relative z-30 flex shrink-0 items-center" data-tauri-drag-region>
      {/* Marque : la zone reste une poignée de déplacement. */}
      <div className="flex h-full items-center gap-2.5 pl-3.5 pr-3" data-tauri-drag-region>
        <BrandMark className="h-[22px] w-[22px]" />
        <span className="font-display text-[13px] font-semibold tracking-tight text-fg" data-tauri-drag-region>
          Batlay
        </span>
        {version && (
          <span className="chip-version rounded-full px-1.5 py-px font-mono text-[10px] font-medium" title={`Batlay ${version}`} data-tauri-drag-region>
            {shortVersion(version)}
          </span>
        )}
        {title && (
          <span className="truncate text-xs text-muted" data-tauri-drag-region>
            · {title}
          </span>
        )}
      </div>

      <div className="h-full flex-1" data-tauri-drag-region />

      {/* Recherche au centre de la barre, comme dans les applications récentes. */}
      {variant === "main" && (
        <button
          type="button"
          onClick={openCommandPalette}
          title="Rechercher une page ou une action (Ctrl+K)"
          aria-label="Rechercher une page ou une action"
          className="absolute left-1/2 top-1/2 flex h-7 w-[min(380px,34vw)] -translate-x-1/2 -translate-y-1/2 items-center gap-2 rounded-lg border border-base-700/80 bg-base-800/60 px-2.5 text-xs text-muted transition hover:border-signal-500/40 hover:bg-base-800 hover:text-fg"
        >
          <Search size={13} className="shrink-0" />
          <span className="flex-1 truncate text-left">Rechercher une page, une action…</span>
          <kbd className="rounded border border-base-700 bg-base-900/60 px-1 font-mono text-[10px] text-faint">Ctrl K</kbd>
        </button>
      )}

      <div className="flex h-full items-center">
        {updateReady && (
          <Link
            to="/settings#updates"
            title={`Mise à jour ${updateVersion} disponible`}
            className="btn-primary mr-2 flex h-7 items-center gap-1.5 rounded-lg px-2.5 text-[11px] font-semibold"
          >
            <ArrowUpCircle size={13} />
            {updateVersion ? `Mettre à jour (${updateVersion})` : "Mise à jour"}
          </Link>
        )}
        {win && (
          <div className="flex h-full items-stretch">
            <button type="button" className="win-btn" onClick={() => void win.minimize()} title="Réduire" aria-label="Réduire">
              <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                <path d="M0 5.5h10" stroke="currentColor" strokeWidth="1" />
              </svg>
            </button>
            <button
              type="button"
              className="win-btn"
              onClick={() => void win.toggleMaximize()}
              title={maximized ? "Restaurer" : "Agrandir"}
              aria-label={maximized ? "Restaurer" : "Agrandir"}
            >
              {maximized ? (
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
                  <rect x="0.5" y="2.5" width="7" height="7" rx="1" stroke="currentColor" />
                  <path d="M2.5 2.5V1.5a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" stroke="currentColor" />
                </svg>
              ) : (
                <svg width="10" height="10" viewBox="0 0 10 10" fill="none" aria-hidden="true">
                  <rect x="0.5" y="0.5" width="9" height="9" rx="1.5" stroke="currentColor" />
                </svg>
              )}
            </button>
            <button type="button" className="win-btn win-btn-close" onClick={() => void win.close()} title="Fermer" aria-label="Fermer">
              <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true">
                <path d="M0.7 0.7l8.6 8.6M9.3 0.7L0.7 9.3" stroke="currentColor" strokeWidth="1.1" />
              </svg>
            </button>
          </div>
        )}
      </div>
    </header>
  );
}
