/**
 * Petits effets liés au mouvement de la souris. Rien n'est animé en JS : on ne
 * fait que publier la position du pointeur dans des variables CSS, et ce sont
 * les styles de globals.css qui en font un effet (halo, parallaxe). Ça garde
 * l'interface propre : un seul écouteur, une écriture par image, aucun rendu React.
 *
 * `normalizePointer` est pure et testée dans tests/pointer-effects.test.ts.
 */

/** Éléments qui affichent un halo local sous le curseur (voir `.card` et `.btn-*` dans globals.css). */
const CARD_SELECTOR = ".card";
const BUTTON_SELECTOR = ".btn-primary, .btn-secondary";

/** Position du pointeur ramenée à [-1, 1] dans la fenêtre (0 = centre) ; sert à décaler le fond. */
export function normalizePointer(x: number, y: number, width: number, height: number): { x: number; y: number } {
  if (!(width > 0) || !(height > 0)) return { x: 0, y: 0 };
  const clamp = (v: number) => Math.min(1, Math.max(-1, v));
  return { x: clamp((x / width) * 2 - 1), y: clamp((y / height) * 2 - 1) };
}

/** Publie la position du pointeur, relative à l'élément, dans --mx / --my (le halo du CSS s'y accroche). */
function trackLocalPointer(el: HTMLElement | null, x: number, y: number): void {
  if (!el) return;
  const rect = el.getBoundingClientRect();
  el.style.setProperty("--mx", `${Math.round(x - rect.left)}px`);
  el.style.setProperty("--my", `${Math.round(y - rect.top)}px`);
}

/**
 * Active les effets de souris pour toute l'application. Renvoie la fonction de
 * nettoyage (compatible avec un `useEffect`). Sans effet si l'utilisateur a
 * demandé de réduire les animations, et ignore le tactile.
 */
export function initPointerEffects(): () => void {
  if (typeof window === "undefined" || typeof document === "undefined") return () => {};

  const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
  let frame = 0;
  let last: { x: number; y: number; target: EventTarget | null } | null = null;
  let aurora: HTMLElement | null = null;

  function paint() {
    frame = 0;
    if (!last || reducedMotion.matches) return;
    const { x, y, target } = last;

    // Parallaxe : les variables sont posées sur le fond seul, pas sur <html>,
    // pour ne pas invalider les styles de toute la page à chaque mouvement.
    if (!aurora || !aurora.isConnected) aurora = document.querySelector<HTMLElement>(".app-aurora");
    if (aurora) {
      const p = normalizePointer(x, y, window.innerWidth, window.innerHeight);
      aurora.style.setProperty("--px", p.x.toFixed(3));
      aurora.style.setProperty("--py", p.y.toFixed(3));
    }

    // Halo local : le bouton survolé ET la carte qui le contient suivent le curseur.
    if (target instanceof Element) {
      trackLocalPointer(target.closest<HTMLElement>(BUTTON_SELECTOR), x, y);
      trackLocalPointer(target.closest<HTMLElement>(CARD_SELECTOR), x, y);
    }
  }

  function onPointerMove(e: PointerEvent) {
    if (e.pointerType !== "mouse" && e.pointerType !== "pen") return;
    last = { x: e.clientX, y: e.clientY, target: e.target };
    // Une seule mise à jour par image, quelle que soit la cadence de la souris.
    if (!frame) frame = requestAnimationFrame(paint);
  }

  document.addEventListener("pointermove", onPointerMove, { passive: true });
  return () => {
    document.removeEventListener("pointermove", onPointerMove);
    if (frame) cancelAnimationFrame(frame);
  };
}
