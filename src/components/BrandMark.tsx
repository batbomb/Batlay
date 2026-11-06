import { useId } from "react";

/**
 * Logo de Batlay 0.5 : la chauve-souris de l'icône, l'égaliseur découpé dans
 * le corps, sur une tuile dégradée (accent → second ton « nova », voir
 * .brand-tile dans globals.css : la tuile suit la couleur d'accent choisie).
 * Les tracés sont ceux de assets/icon.svg.
 */
export const BAT_WING = "M232 214 C 196 178 128 162 58 188 C 74 204 84 222 86 246 C 104 238 122 244 132 262 C 148 252 168 258 178 278 C 192 270 208 276 216 294 L 232 300 Z";
export const BAT_BODY = "M212 206 L 226 150 L 246 186 L 266 186 L 286 150 L 300 206 C 304 250 300 300 256 350 C 212 300 208 250 212 206 Z";

/** Chauve-souris seule (couleur = currentColor), barres de l'égaliseur en creux. */
export function BatGlyph({ className = "", playing = false }: { className?: string; playing?: boolean }) {
  // Identifiant propre à chaque logo : plusieurs logos peuvent être affichés à la fois.
  const id = `batlay-eq-${useId().replace(/:/g, "")}`;
  return (
    <svg viewBox="44 140 424 220" className={className} fill="currentColor" aria-hidden="true">
      <mask id={id}>
        <rect x="0" y="0" width="512" height="512" fill="#fff" />
        <rect className={playing ? "eq-bar" : undefined} x="226" y="240" width="16" height="58" rx="8" fill="#000" />
        <rect className={playing ? "eq-bar" : undefined} x="248" y="222" width="16" height="76" rx="8" fill="#000" />
        <rect className={playing ? "eq-bar" : undefined} x="270" y="252" width="16" height="46" rx="8" fill="#000" />
      </mask>
      <g mask={`url(#${id})`}>
        <path d={BAT_WING} />
        <path d={BAT_WING} transform="translate(512,0) scale(-1,1)" />
        <path d={BAT_BODY} />
      </g>
    </svg>
  );
}

/** Tuile du logo. `playing` : les barres de l'égaliseur dansent (un morceau est en lecture). */
export function BrandMark({ className = "h-8 w-8", playing = false }: { className?: string; playing?: boolean }) {
  return (
    <span className={`brand-tile inline-flex shrink-0 items-center justify-center rounded-[30%] ${className}`} aria-hidden="true">
      <BatGlyph className="w-[86%]" playing={playing} />
    </span>
  );
}
