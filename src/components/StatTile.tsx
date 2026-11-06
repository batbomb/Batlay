import type { ReactNode } from "react";
import type { LucideIcon } from "lucide-react";

/**
 * Tuile de statistique de l'identité 0.5 (Vue d'ensemble, page Live) : libellé,
 * icône teintée avec son halo, grand chiffre qui rebondit à chaque changement.
 * `tint` = couleur RVB (« 254 44 85 ») de l'icône et du halo.
 */
export function StatTile({ icon: Icon, tint, label, value, footer, title }: { icon: LucideIcon; tint: string; label: string; value: number; footer?: ReactNode; title?: string }) {
  return (
    <div className="card overflow-hidden px-4 py-3.5" title={title}>
      <div className="pointer-events-none absolute -right-6 -top-8 h-24 w-24 rounded-full blur-2xl" style={{ background: `rgb(${tint} / ${value > 0 ? 0.28 : 0.14})` }} aria-hidden="true" />
      <div className="relative flex items-center justify-between gap-2">
        <span className="truncate text-xs text-muted">{label}</span>
        <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg" style={{ background: `rgb(${tint} / 0.14)`, color: `rgb(${tint})` }}>
          <Icon size={14} />
        </span>
      </div>
      {/* La clé change avec la valeur : le chiffre rebondit à chaque nouvel événement. */}
      <div className="relative mt-1.5 font-display text-[30px] font-semibold leading-none tabular-nums tracking-[-0.02em] text-fg">
        <span key={value} className={value > 0 ? "num-tick" : undefined}>
          {value}
        </span>
      </div>
      {footer && <div className="relative mt-2">{footer}</div>}
    </div>
  );
}
