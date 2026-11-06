import { describe, it, expect } from "vitest";
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

/**
 * Garde-fous sur le TEXTE de l'application (src-tauri, crates/batlay-core) et
 * du pont TypeScript : ce code ne se compile que sous Windows (Tauri, WebView2),
 * mais les règles de sécurité et la cohérence entre les trois couches se
 * vérifient ici, à chaque `npm test`. Le comportement lui-même est testé en
 * Rust (`cargo test` dans crates/batlay-core).
 */
const ROOT = process.cwd();
const read = (f: string) => readFileSync(join(ROOT, f), "utf8");
const walk = (dir: string): string[] =>
  readdirSync(join(ROOT, dir)).flatMap((name) => {
    const rel = join(dir, name);
    return statSync(join(ROOT, rel)).isDirectory() ? walk(rel) : [rel];
  });
/** Code Rust sans les modules de test (ils contiennent volontairement de faux cas). */
const rustSources = (dir: string) =>
  walk(dir)
    .filter((f) => f.endsWith(".rs"))
    .map((f) => ({ f, code: read(f).split(/#\[cfg\(test\)\]/)[0] }));

const conf = JSON.parse(read("src-tauri/tauri.conf.json"));
const capability = JSON.parse(read("src-tauri/capabilities/main.json"));
const bridge = read("src/platform/bridge.ts");
const appRs = read("crates/batlay-core/src/app.rs");

describe("aucun secret dans le dépôt", () => {
  const files = [
    ...walk("src"),
    ...walk("shared"),
    ...walk("overlay"),
    ...walk("twitch-overlay"),
    ...walk("alerts"),
    ...walk("crates/batlay-core/src"),
    ...walk("src-tauri/src"),
    "src-tauri/tauri.conf.json",
    "package.json",
    "index.html",
  ];
  it("aucun « client_secret » (Twitch et Spotify passent par PKCE / jeton implicite)", () => {
    for (const f of files) expect(read(f), f).not.toMatch(/client[_-]?secret/i);
  });
  it("aucune variable d'environnement de secret lue par l'application", () => {
    for (const f of files) expect(read(f), f).not.toMatch(/(process\.env|env::var(_os)?)\W+\w*(SECRET|TOKEN|PASSWORD)/i);
  });
  it("le .gitignore exclut les fichiers .env", () => {
    expect(read(".gitignore")).toMatch(/^\.env/m);
  });
});

describe("serveurs locaux : jamais exposés au réseau", () => {
  it("un seul point d'écoute, sur 127.0.0.1", () => {
    const binds = rustSources("crates/batlay-core/src").flatMap(({ f, code }) => [...code.matchAll(/TcpListener::bind\(([^)]*)\)/g)].map((m) => `${f}: ${m[1]}`));
    expect(binds).toHaveLength(1);
    expect(binds[0]).toMatch(/"127\.0\.0\.1"/);
  });
  it("aucune adresse 0.0.0.0 ni « tous les réseaux » dans le code", () => {
    for (const { f, code } of [...rustSources("crates/batlay-core/src"), ...rustSources("src-tauri/src")]) {
      const lines = code.split("\n").filter((l) => !l.trim().startsWith("//"));
      expect(lines.join("\n"), f).not.toMatch(/(?<![\d.])0\.0\.0\.0(?![\d.])|UNSPECIFIED|\[::\]/);
    }
  });
  it("le port 4000 (Twitch) reste fixe et n'est jamais pris par les overlays", () => {
    expect(read("crates/batlay-core/src/config.rs")).toMatch(/pub const TWITCH_PORT: u16 = 4000;/);
    expect(read("crates/batlay-core/src/config.rs")).toMatch(/DEFAULT_OVERLAY_PORT: u16 = 3000/);
    expect(read("src-tauri/src/main.rs")).toContain("twitch_port: batlay_core::config::TWITCH_PORT");
  });
});

describe("Tauri : surface minimale", () => {
  it("une seule commande, réservée aux fenêtres du tableau de bord", () => {
    expect(capability.windows).toEqual(["main", "live"]);
    // Barre de titre Batlay : seulement déplacer, réduire, agrandir / restaurer et fermer (jamais créer,
    // redimensionner à distance, changer d'URL ni lire autre chose que « agrandie ? »).
    expect(capability.permissions).toEqual([
      "allow-batlay",
      "core:event:allow-listen",
      "core:event:allow-unlisten",
      "core:window:allow-start-dragging",
      "core:window:allow-internal-toggle-maximize",
      "core:window:allow-minimize",
      "core:window:allow-toggle-maximize",
      "core:window:allow-is-maximized",
      "core:window:allow-close",
    ]);
    expect(capability.remote).toBeUndefined();
    expect(read("src-tauri/build.rs")).toContain('commands(&["batlay"])');
    expect(read("src-tauri/src/main.rs")).toMatch(/generate_handler!\[bridge::batlay\]/);
    // Double garde dans la commande elle-même.
    expect(read("src-tauri/src/bridge.rs")).toMatch(/webview\.label\(\) != MAIN && webview\.label\(\) != LIVE/);
  });
  it("une seule liste de capacités, et aucune option « dangerous » dans la configuration", () => {
    expect(conf.app.security.capabilities).toEqual(["main"]);
    expect(JSON.stringify(conf)).not.toMatch(/dangerous/i);
  });
  it("CSP stricte : scripts de l'application seulement, aucun eval", () => {
    const csp = conf.app.security.csp as Record<string, string>;
    expect(csp["script-src"]).toBe("'self'");
    expect(csp["object-src"]).toBe("'none'");
    expect(JSON.stringify(csp)).not.toMatch(/unsafe-eval|\*/);
    expect(csp["connect-src"]).toBe("'self' ipc: http://ipc.localhost");
  });
  it("instance unique enregistrée en premier", () => {
    const main = read("src-tauri/src/main.rs");
    const firstPlugin = main.indexOf(".plugin(");
    expect(main.slice(firstPlugin, firstPlugin + 60)).toContain("tauri_plugin_single_instance");
    expect(firstPlugin).toBeLessThan(main.indexOf(".invoke_handler("));
  });
  it("mêmes options WebView2 pour toutes les fenêtres (WebView2 l'exige), minuteurs jamais ralentis", () => {
    const sources = ["src-tauri/src/host.rs", "src-tauri/src/tiktok_pages.rs"].map(read).join("\n");
    const builders = sources.match(/WebviewWindowBuilder::new\(/g) ?? [];
    const args = sources.match(/\.additional_browser_args\(BROWSER_ARGS\)/g) ?? [];
    expect(builders.length).toBeGreaterThanOrEqual(2);
    expect(args.length).toBe(builders.length);
    const host = read("src-tauri/src/host.rs");
    for (const flag of ["CalculateNativeWinOcclusion", "IntensiveWakeUpThrottling", "--disable-background-timer-throttling", "--disable-renderer-backgrounding", "--disable-backgrounding-occluded-windows"]) {
      expect(host).toContain(flag);
    }
  });
  it("fenêtres du tableau de bord sans cadre Windows (barre de titre Batlay), ombre gardée ; mise à jour installée en silence", () => {
    const host = read("src-tauri/src/host.rs");
    expect(host).toContain(".decorations(false)");
    expect(host).toContain(".shadow(true)");
    expect(host).toContain('Some("/S /R")');
    expect(read("src/layouts/TitleBar.tsx")).toContain("data-tauri-drag-region");
    expect(read("src/App.tsx").match(/<TitleBar\b/g)?.length).toBe(2);
  });
  it("le tableau de bord ne navigue nulle part et n'ouvre aucune fenêtre", () => {
    const host = read("src-tauri/src/host.rs");
    expect(host).toContain(".on_navigation(is_app_url)");
    expect(host).toContain(".on_new_window(|_, _| NewWindowResponse::Deny)");
  });
  it("pages TikTok : profil à part, navigation limitée à tiktok.com en https, pas d'outils de développement", () => {
    const pages = read("src-tauri/src/tiktok_pages.rs");
    expect(pages).toContain(".data_directory(self.profile.clone())");
    expect(pages).toContain(".devtools(false)");
    expect(pages).toMatch(/url\.scheme\(\) == "https" && matches!\(url\.host_str\(\), Some\("www\.tiktok\.com" \| "tiktok\.com"\)\)/);
    expect(pages).toContain('.on_navigation(|url| url.as_str() == "about:blank" || is_tiktok_page(url))');
  });
});

describe("pont TypeScript ↔ cœur Rust : cohérence", () => {
  const coreCommands = new Set([...appRs.matchAll(/^\s*"([a-z-]+:[a-z-]+)" =>/gm)].map((m) => m[1]));
  const bridgeCommands = [...bridge.matchAll(/call\("([a-z-]+:[a-z-]+)"/g)].map((m) => m[1]);

  it("chaque commande du pont existe dans le cœur", () => {
    expect(bridgeCommands.length).toBeGreaterThan(40);
    for (const cmd of bridgeCommands) expect(coreCommands.has(cmd), cmd).toBe(true);
  });
  it("chaque commande du cœur est utilisée par le pont (sauf artwork:fetch, appelée par la recherche de pochettes)", () => {
    const used = new Set([...bridgeCommands, "artwork:fetch"]);
    for (const cmd of coreCommands) expect(used.has(cmd), cmd).toBe(true);
    expect(read("src/services/artwork-lookup.ts")).toContain('invoke("artwork:fetch", [method, url])');
  });
  it("chaque événement envoyé par le cœur ou l'application est écouté par le pont", () => {
    const emitted = new Set([
      ...rustSources("crates/batlay-core/src").flatMap(({ code }) => [...code.matchAll(/\.emit\("([a-z-]+:[a-z-]+)"/g)].map((m) => m[1])),
      ...[...read("src-tauri/src/tiktok_pages.rs").matchAll(/emit_app\([^,]+, "([a-z-]+:[a-z-]+)"/g)].map((m) => m[1]),
    ]);
    expect(emitted.size).toBeGreaterThanOrEqual(8);
    const listened = new Set([...bridge.matchAll(/(?:on|nativeListen<\w+>)\("([a-z-]+:[a-z-]+)"/g)].map((m) => m[1]));
    for (const channel of emitted) expect(listened.has(channel), channel).toBe(true);
  });
  it("le pont Twitch ne transmet ni jeton, ni port, ni adresse", () => {
    const block = bridge.slice(bridge.indexOf("twitch: {"), bridge.indexOf("theme: {"));
    expect(block).not.toMatch(/token|port|clientId|url/i);
  });
  it("le pont n'a aucune dépendance npm (Tauri global) et ne s'installe que dans l'application", () => {
    expect(bridge + read("src/platform/native.ts")).not.toMatch(/from "@tauri-apps/);
    expect(conf.app.withGlobalTauri).toBe(true);
    expect(read("src/main.tsx").indexOf('import "./platform/bridge"')).toBeLessThan(read("src/main.tsx").indexOf("initTheme()"));
  });
});

describe("Electron entièrement retiré", () => {
  const pkg = JSON.parse(read("package.json"));
  it("aucune dépendance Electron ni bibliothèque TikTok tierce", () => {
    const deps = { ...pkg.dependencies, ...pkg.devDependencies };
    for (const name of Object.keys(deps)) expect(name, name).not.toMatch(/^electron|tiktok-live-connector|^express$|^ws$|wait-on/);
    expect(pkg.main).toBeUndefined();
    expect(pkg.build).toBeUndefined();
  });
  it("le code de l'interface n'appelle plus ipcRenderer ni require", () => {
    for (const f of walk("src").filter((x) => /\.tsx?$/.test(x))) {
      expect(read(f), f).not.toMatch(/ipcRenderer\.\w+\(|require\(["']electron/);
    }
  });
});
