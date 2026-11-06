/**
 * Noms et adresses « boucle locale » des serveurs de Batlay, pour l'interface
 * (testés dans tests/loopback.test.ts). Les contrôles eux-mêmes (Host, Origin)
 * sont faits par le cœur : crates/batlay-core/src/loopback.rs.
 *
 * Les serveurs n'écoutent que sur 127.0.0.1, mais cela ne suffit pas : une
 * page web quelconque ouverte dans un navigateur peut viser http://127.0.0.1
 * (requêtes « simples », WebSocket) ou passer par un DNS-rebinding
 * (`evil.example` résolu en 127.0.0.1). Vérifier l'en-tête `Host` (et
 * l'`Origin` des WebSocket) ferme ces deux portes, sans gêner OBS, Streamlabs,
 * XSplit ni TikTok LIVE Studio : leur source Web charge la page depuis
 * `http://localhost:<port>` ou `http://127.0.0.1:<port>`, donc même origine.
 */

/**
 * Nom local optionnel pour les logiciels qui refusent `localhost` et les
 * adresses IP (constaté sur le champ « Lien » de TikTok LIVE Studio, qui
 * affiche « Saisis l'URL correcte »). `.test` est réservé par la RFC 6761 : il
 * n'existe sur aucun DNS public, donc il ne pointe vers ce PC que grâce à la
 * ligne `127.0.0.1 batlay.test` du fichier hosts : ajoutée par Batlay après
 * l'autorisation de Windows (crates/batlay-core/src/hosts_alias.rs), ou à la main.
 */
export const LOCAL_ALIAS_HOSTNAME = "batlay.test";
/** Ligne à ajouter au fichier hosts de Windows. */
export const HOSTS_FILE_LINE = `127.0.0.1 ${LOCAL_ALIAS_HOSTNAME}`;

/** Noms d'hôte acceptés pour joindre un serveur de Batlay. */
export const LOOPBACK_HOSTNAMES = ["localhost", "127.0.0.1", LOCAL_ALIAS_HOSTNAME] as const;
export type LoopbackHostname = (typeof LOOPBACK_HOSTNAMES)[number];

/**
 * Vrai si `url` est une adresse http locale de Batlay (jamais https, jamais un
 * autre hôte, jamais d'identifiants dans l'URL). Sert de garde-fou avant de
 * copier une URL : l'interface ne doit jamais proposer d'adresse publique.
 */
export function isLoopbackHttpUrl(url: string): boolean {
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  return (
    parsed.protocol === "http:" &&
    (LOOPBACK_HOSTNAMES as readonly string[]).includes(parsed.hostname) &&
    parsed.username === "" &&
    parsed.password === "" &&
    parsed.port !== ""
  );
}
