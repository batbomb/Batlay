import { nativeInitial } from "@/platform/native";

/** Version de Batlay posée par l'application avant le chargement de la page (« 0.5.0 »), ou "". */
export function appVersion(): string {
  const v = nativeInitial()?.version;
  return typeof v === "string" && /^\d+\.\d+\.\d+/.test(v) ? v : "";
}

/** « 0.5.0 » → « 0.5 » (pastille de la barre de titre) ; une préversion garde son suffixe. */
export function shortVersion(version: string): string {
  const m = /^(\d+)\.(\d+)\.\d+(-[\w.]+)?/.exec(version);
  return m ? `${m[1]}.${m[2]}${m[3] ?? ""}` : version;
}
