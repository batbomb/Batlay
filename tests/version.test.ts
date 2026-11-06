import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join } from "node:path";

const read = (file: string) => readFileSync(join(process.cwd(), file), "utf8");

// Depuis la 0.5, l'à-propos (Paramètres) affiche la version donnée par Batlay
// lui-même ; ce test vérifie que les fichiers de version restent alignés.
describe("numéro de version", () => {
  const { version } = JSON.parse(read("package.json")) as { version: string };

  it("est le même dans package.json et package-lock.json ; l'à-propos n'écrit aucun numéro à la main", () => {
    const lock = JSON.parse(read("package-lock.json")) as { version: string; packages: Record<string, { version: string }> };
    expect(lock.version).toBe(version);
    expect(lock.packages[""].version).toBe(version);
    const settings = read("src/pages/Settings.tsx");
    expect(settings).not.toMatch(/Version \d+\.\d+\.\d+/);
    expect(settings).toContain("version: s.version");
  });

  it("est le même pour l'application Tauri et le cœur Rust (installateur et mises à jour)", () => {
    const conf = JSON.parse(read("src-tauri/tauri.conf.json")) as { version: string };
    expect(conf.version).toBe(version);
    for (const cargo of ["src-tauri/Cargo.toml", "crates/batlay-core/Cargo.toml"]) {
      expect(read(cargo).match(/^version = "([^"]+)"/m)?.[1], cargo).toBe(version);
    }
  });
});
