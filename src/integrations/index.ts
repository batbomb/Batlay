/**
 * Couche d'intégration multi-logiciels (OBS, TikTok LIVE Studio, Streamlabs,
 * XSplit, autres sources Web). Elle ne rend RIEN elle-même : les overlays
 * Musique et Chat gardent leur unique moteur HTML/CSS/JS, la même URL sert
 * partout. Ce module ne fait que décrire, pour chaque logiciel, comment y
 * coller cette URL, à quelle taille, et comment vérifier que ça marche.
 */
export * from "./platforms";
export * from "./formats";
export * from "./urls";
export * from "./diagnostics";
