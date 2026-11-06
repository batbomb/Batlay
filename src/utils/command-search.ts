/**
 * Recherche de la palette de commandes (Ctrl+K) : PURE, testée dans
 * tests/command-search.test.ts. Sans accents ni majuscules ; tous les mots
 * tapés doivent se retrouver (dans l'ordre que l'on veut) dans le titre ou les
 * mots-clés. Le titre compte plus que les mots-clés, un début de mot plus qu'un milieu.
 */

export interface SearchableCommand {
  id: string;
  title: string;
  /** Synonymes (« obs », « lien », « url »…). */
  keywords?: string;
  group: string;
}

export const fold = (s: string): string =>
  s
    .normalize("NFD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[’']/g, " ");

function wordScore(word: string, title: string, keywords: string): number {
  const inTitle = title.indexOf(word);
  if (inTitle === 0) return 6;
  if (inTitle > 0) return title[inTitle - 1] === " " ? 5 : 3;
  const inKeys = keywords.indexOf(word);
  if (inKeys >= 0) return inKeys === 0 || keywords[inKeys - 1] === " " ? 2 : 1;
  return 0;
}

/** Commandes qui correspondent, les meilleures d'abord ; requête vide = toutes, dans l'ordre d'origine. */
export function searchCommands<T extends SearchableCommand>(commands: T[], query: string): T[] {
  const words = fold(query).split(/\s+/).filter(Boolean);
  if (!words.length) return commands;
  const scored: { c: T; score: number; i: number }[] = [];
  commands.forEach((c, i) => {
    const title = fold(c.title);
    const keys = fold(`${c.keywords ?? ""} ${c.group}`);
    let score = 0;
    for (const w of words) {
      const s = wordScore(w, title, keys);
      if (s === 0) return;
      score += s;
    }
    scored.push({ c, score, i });
  });
  return scored.sort((a, b) => b.score - a.score || a.i - b.i).map((x) => x.c);
}
