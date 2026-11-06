

/** État de la mise à jour, poussé par le cœur de Batlay (crates/batlay-core/src/updater.rs) vers l'interface. */
export type UpdateStatus =
  | { state: "unavailable"; reason: "dev" | "not-configured" }
  | { state: "idle" }
  | { state: "checking" }
  | { state: "up-to-date" }
  | { state: "available"; version: string }
  | { state: "downloading"; percent: number }
  | { state: "downloaded"; version: string }
  | { state: "error"; message: string };
