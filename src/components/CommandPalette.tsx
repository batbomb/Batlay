import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { CornerDownLeft, Search } from "lucide-react";
import { searchCommands } from "@/utils/command-search";

/**
 * Palette de commandes : Ctrl+K (ou le bouton « Rechercher » de la barre
 * latérale). Aller à une page ou lancer une action sans la chercher dans les
 * menus. ↑ ↓ pour choisir, Entrée pour lancer, Échap pour fermer.
 */

export interface Command {
  id: string;
  title: string;
  group: string;
  keywords?: string;
  icon?: ReactNode;
  /** Raccourci affiché à droite (indicatif). */
  hint?: string;
  run: (ctx: { navigate: ReturnType<typeof useNavigate> }) => void | Promise<void>;
}

export const OPEN_PALETTE_EVENT = "batlay:open-palette";

export function openCommandPalette(): void {
  window.dispatchEvent(new Event(OPEN_PALETTE_EVENT));
}

export function CommandPalette({ commands }: { commands: Command[] }) {
  const navigate = useNavigate();
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setOpen((o) => !o);
      }
    };
    const onOpen = () => setOpen(true);
    window.addEventListener("keydown", onKey);
    window.addEventListener(OPEN_PALETTE_EVENT, onOpen);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener(OPEN_PALETTE_EVENT, onOpen);
    };
  }, []);

  useEffect(() => {
    if (open) {
      setQuery("");
      setIndex(0);
      requestAnimationFrame(() => input.current?.focus());
    }
  }, [open]);

  const results = useMemo(() => searchCommands(commands, query), [commands, query]);
  useEffect(() => setIndex(0), [query]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${index}"]`)?.scrollIntoView({ block: "nearest" });
  }, [index]);

  if (!open) return null;

  const run = (c: Command | undefined) => {
    if (!c) return;
    setOpen(false);
    void c.run({ navigate });
  };

  // Regroupe en gardant l'ordre des résultats.
  const groups: { name: string; items: { c: Command; i: number }[] }[] = [];
  results.forEach((c, i) => {
    const g = groups.find((x) => x.name === c.group) ?? (groups.push({ name: c.group, items: [] }), groups[groups.length - 1]);
    g.items.push({ c, i });
  });

  return (
    <div className="modal-backdrop fixed inset-0 z-50 flex items-start justify-center bg-black/50 px-4 pt-[12vh] backdrop-blur-sm" onMouseDown={() => setOpen(false)}>
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Palette de commandes"
        className="modal-panel card w-full max-w-xl overflow-hidden rounded-[20px] p-0 shadow-pop"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 border-b border-line px-5 py-4">
          <Search size={18} className="text-accent" />
          <input
            ref={input}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setIndex((i) => Math.min(results.length - 1, i + 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setIndex((i) => Math.max(0, i - 1));
              } else if (e.key === "Enter") {
                e.preventDefault();
                run(results[index]);
              } else if (e.key === "Escape") {
                setOpen(false);
              }
            }}
            placeholder="Aller à une page, copier une URL, tester une alerte…"
            aria-label="Rechercher une commande"
            className="bare-input min-w-0 flex-1 border-0 bg-transparent text-[15px] text-fg shadow-none outline-none ring-0 placeholder:text-faint focus:border-0 focus:outline-none focus:ring-0"
          />
          <kbd className="rounded-md border border-base-700 px-1.5 py-0.5 font-mono text-[10px] text-muted">Échap</kbd>
        </div>
        <ul ref={list} className="max-h-[52vh] overflow-y-auto p-2" role="listbox">
          {results.length === 0 ? (
            <li className="px-3 py-8 text-center text-sm text-muted">Aucune commande pour « {query} ».</li>
          ) : (
            groups.map((g) => (
              <li key={g.name}>
                <p className="px-3 pb-1 pt-2 text-[10px] font-semibold uppercase tracking-wider text-faint">{g.name}</p>
                <ul>
                  {g.items.map(({ c, i }) => (
                    <li
                      key={c.id}
                      data-index={i}
                      role="option"
                      aria-selected={i === index}
                      onMouseMove={() => setIndex(i)}
                      onClick={() => run(c)}
                      className={`relative flex cursor-pointer items-center gap-3 rounded-xl px-3 py-2 text-sm transition-colors ${i === index ? "nav-item-active" : "text-muted"}`}
                    >
                      <span className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-lg transition-colors ${i === index ? "brand-tile" : "bg-base-800 text-muted"}`}>{c.icon}</span>
                      <span className="min-w-0 flex-1 truncate">{c.title}</span>
                      {c.hint && <span className="text-[11px] text-faint">{c.hint}</span>}
                      {i === index && <CornerDownLeft size={13} className="text-faint" />}
                    </li>
                  ))}
                </ul>
              </li>
            ))
          )}
        </ul>
        <div className="flex items-center gap-4 border-t border-line px-4 py-2 text-[11px] text-faint">
          <span>↑ ↓ choisir</span>
          <span>Entrée lancer</span>
          <span className="ml-auto">Ctrl K pour ouvrir ou fermer</span>
        </div>
      </div>
    </div>
  );
}
