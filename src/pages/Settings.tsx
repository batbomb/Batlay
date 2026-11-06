import { SlidersHorizontal as PageIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { AlertTriangle, ArrowUpCircle, CheckCircle2, ExternalLink, Loader2, RefreshCw, Sparkles } from "lucide-react";
import { useLocation } from "react-router-dom";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useOverlayStore } from "@/stores/useOverlayStore";
import { useToastStore } from "@/stores/useToastStore";
import { useUpdateStore } from "@/stores/useUpdateStore";
import { ColorField } from "@/components/editor/fields";
import { Button, PageHeader, SectionTitle, StatusPill, Switch } from "@/components/ui";
import { RegenerateObsLinkModal } from "@/components/RegenerateObsLinkModal";
import { BrandMark } from "@/components/BrandMark";
import { appVersion } from "@/utils/app-version";
import { DEFAULT_ACCENT_HEX } from "@/theme/accent";
import { ACCENT_PRESETS, normalizeDensity, normalizeMotion, type UiDensity, type UiMotion } from "@/theme/ui-prefs";
import { validatePort } from "@/utils/ui-helpers";
import type { ThemePreference } from "../../shared/theme";

const THEME_OPTIONS: { value: ThemePreference; label: string }[] = [
  { value: "dark", label: "Sombre" },
  { value: "light", label: "Clair" },
  { value: "system", label: "Système" },
];

export function Settings() {
  const { settings, overlayServerRunning, load, update } = useSettingsStore((s) => ({
    settings: s.settings,
    overlayServerRunning: s.overlayServerRunning,
    load: s.load,
    update: s.update,
  }));
  const push = useToastStore((s) => s.push);
  const { overlayCount, regenerateAllObsLinks } = useOverlayStore((s) => ({
    overlayCount: s.overlays.length,
    regenerateAllObsLinks: s.regenerateAllObsLinks,
  }));
  const [confirmRegenerateAll, setConfirmRegenerateAll] = useState(false);

  // Le port se saisit dans un texte local et n'est enregistré qu'à la validation
  // (Entrée / sortie du champ) : avant, chaque frappe était enregistrée — taper
  // « 8945 » sauvegardait successivement 8, 89, 894 puis 8945, avec une
  // notification à chaque touche.
  const savedPort = settings?.overlayServerPort;
  const [portText, setPortText] = useState("");
  const [portError, setPortError] = useState<string | null>(null);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (savedPort !== undefined) setPortText(String(savedPort));
  }, [savedPort]);

  if (!settings) return null;

  async function commitPort() {
    const result = validatePort(portText);
    if (!result.ok) {
      setPortError(result.reason);
      return;
    }
    setPortError(null);
    if (result.port === savedPort) return;
    await update({ overlayServerPort: result.port });
    push("Port enregistré : redémarrez Batlay pour l'appliquer.", "info");
  }

  async function handleRegenerateAll() {
    try {
      const count = await regenerateAllObsLinks();
      push(
        `${count} lien${count > 1 ? "s" : ""} OBS régénéré${count > 1 ? "s" : ""} : recopiez-les dans OBS (Mes overlays > URL OBS).`,
        "success"
      );
    } catch (err) {
      push((err as Error).message || "Impossible de régénérer les liens OBS.", "error");
    }
  }

  return (
    <div className="stagger mx-auto max-w-3xl px-8 py-10">
      <PageHeader icon={PageIcon} eyebrow="Application" title="Paramètres" subtitle="Démarrage, apparence, serveur d'overlay et mises à jour." />

      <section className="mt-6 card p-6">
        <SectionTitle className="mb-2">Général</SectionTitle>
        <Toggle
          label="Lancer au démarrage de Windows"
          hint="Version installée uniquement."
          checked={settings.launchOnStartup}
          onChange={(v) => update({ launchOnStartup: v })}
        />
        <Toggle
          label="Démarrer réduit"
          hint="Batlay s'ouvre réduit dans la barre des tâches : pratique pour l'ouvrir avec Windows sans qu'il gêne."
          checked={settings.startMinimized}
          onChange={(v) => update({ startMinimized: v })}
        />
      </section>

      <section className="mt-4 card p-6">
        <SectionTitle className="mb-4">Apparence</SectionTitle>
        <div className="flex items-center justify-between text-sm text-fg">
          <span id="theme-label">Thème</span>
          <div
            role="radiogroup"
            aria-labelledby="theme-label"
            className="seg flex gap-1 rounded-lg p-0.5"
          >
            {THEME_OPTIONS.map((option) => {
              const selected = settings.theme === option.value;
              return (
                <button
                  key={option.value}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  onClick={() => update({ theme: option.value })}
                  className={`seg-item rounded-md px-3 py-1 text-xs font-medium ${selected ? "seg-active" : ""}`}
                >
                  {option.label}
                </button>
              );
            })}
          </div>
        </div>
        <p className="mt-2 text-xs text-muted">« Système » suit le thème clair/sombre de Windows.</p>

        <ChoiceRow<UiDensity>
          label="Densité"
          hint="« Compacte » affiche plus de choses à l'écran (textes et espacements un peu plus petits)."
          value={normalizeDensity(settings.density)}
          options={[
            { value: "comfortable", label: "Confortable" },
            { value: "compact", label: "Compacte" },
          ]}
          onChange={(density) => update({ density })}
        />
        <ChoiceRow<UiMotion>
          label="Animations de l'interface"
          hint="Concerne seulement Batlay, pas vos overlays. Elles sont aussi réduites si Windows le demande."
          value={normalizeMotion(settings.uiMotion)}
          options={[
            { value: "full", label: "Normales" },
            { value: "reduced", label: "Réduites" },
          ]}
          onChange={(uiMotion) => update({ uiMotion })}
        />

        <div className="mt-4 border-t border-line pt-4">
          <span className="mb-2 block text-sm text-fg">Couleur d'accent</span>
          <div className="mb-3 flex flex-wrap gap-2" role="radiogroup" aria-label="Couleurs proposées">
            {ACCENT_PRESETS.map((p) => {
              const selected = (settings.accentColor ?? DEFAULT_ACCENT_HEX).toUpperCase() === p.hex;
              return (
                <button
                  key={p.hex}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  title={p.label}
                  aria-label={p.label}
                  onClick={() => update({ accentColor: p.hex === DEFAULT_ACCENT_HEX ? null : p.hex })}
                  className={`h-7 w-7 rounded-full transition hover:scale-110 ${selected ? "ring-2 ring-fg ring-offset-2 ring-offset-base-900" : "ring-1 ring-black/10"}`}
                  style={{ background: p.hex }}
                />
              );
            })}
          </div>
          <ColorField
            label="Couleur des boutons"
            value={settings.accentColor ?? DEFAULT_ACCENT_HEX}
            onChange={(accentColor) => update({ accentColor })}
          />
          {settings.accentColor && (
            <button
              type="button"
              onClick={() => update({ accentColor: null })}
              className="mt-2 text-xs text-muted hover:text-fg"
            >
              Réinitialiser la couleur par défaut
            </button>
          )}
        </div>
      </section>

      <section className="mt-4 card p-6">
        <SectionTitle className="mb-4">Serveur d'overlay</SectionTitle>
        <div className="flex items-center justify-between text-sm text-fg">
          <span>État</span>
          <StatusPill tone={overlayServerRunning ? "ok" : "warn"}>{overlayServerRunning ? "En ligne" : "Hors ligne"}</StatusPill>
        </div>
        <label className="mt-3 flex items-center justify-between text-sm text-fg">
          <span>Port</span>
          <input
            type="text"
            inputMode="numeric"
            value={portText}
            onChange={(e) => {
              setPortText(e.target.value);
              setPortError(null);
            }}
            onBlur={() => void commitPort()}
            onKeyDown={(e) => {
              if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            }}
            aria-invalid={portError !== null}
            className="w-24 rounded-md border border-base-700 bg-base-800 px-2 py-1 text-right text-sm outline-none focus:border-signal-500"
          />
        </label>
        {portError ? (
          <p role="alert" className="mt-2 text-xs text-danger">{portError}</p>
        ) : (
          <p className="mt-2 text-xs text-muted">
            Un changement de port nécessite de redémarrer Batlay, et les URL déjà collées dans OBS devront être mises à jour.
          </p>
        )}

        <div className="mt-4 flex items-center justify-between gap-4 border-t border-line pt-4">
          <div className="min-w-0">
            <p className="text-sm text-fg">Liens OBS</p>
            <p className="mt-0.5 text-xs text-muted">
              Crée de nouveaux liens pour tous vos overlays. Les anciens cessent de fonctionner (lien partagé par erreur, source OBS à réinitialiser...).
            </p>
          </div>
          <Button size="sm" disabled={overlayCount === 0} onClick={() => setConfirmRegenerateAll(true)}>
            <RefreshCw size={14} /> Régénérer
          </Button>
        </div>
      </section>

      <UpdateSection />

      {confirmRegenerateAll && (
        <RegenerateObsLinkModal
          count={overlayCount}
          onClose={() => setConfirmRegenerateAll(false)}
          onConfirm={() => {
            setConfirmRegenerateAll(false);
            void handleRegenerateAll();
          }}
        />
      )}
    </div>
  );
}

/**
 * « Batlay » : à propos et mises à jour dans une même carte (identité 0.5).
 * Recherche sur GitHub, téléchargement, puis installation : Batlay affiche son
 * propre écran d'installation (voir UpdateInstallOverlay), sans fenêtre Windows.
 */
function UpdateSection() {
  const { status, version, check, download, install, openReleases } = useUpdateStore((s) => ({
    status: s.status,
    version: s.version,
    check: s.check,
    download: s.download,
    install: s.install,
    openReleases: s.openReleases,
  }));
  const shownVersion = version ?? (appVersion() || null);
  const { hash } = useLocation();
  // Bouton « Mettre à jour » de la barre de titre : la carte vient à l'écran.
  useEffect(() => {
    if (hash !== "#updates") return;
    // Défilement de la zone de page seulement (scrollIntoView ferait aussi glisser la barre de titre).
    const el = document.getElementById("updates");
    const main = el?.closest("main");
    if (el && main) main.scrollTo({ top: main.scrollTop + el.getBoundingClientRect().top - main.getBoundingClientRect().top - 24, behavior: "smooth" });
  }, [hash]);

  let icon: React.ReactNode = <RefreshCw size={16} />;
  let tone: "idle" | "ok" | "new" | "error" = "idle";
  let title: React.ReactNode;
  let detail: React.ReactNode = null;
  let action: React.ReactNode;
  const checkButton = (
    <Button size="sm" onClick={() => void check()}>
      <RefreshCw size={13} /> Rechercher
    </Button>
  );

  switch (status.state) {
    case "unavailable":
      title = "Mises à jour indisponibles ici";
      detail =
        status.reason === "dev"
          ? "Disponible uniquement dans la version installée de Batlay."
          : "Dépôt GitHub non renseigné : modifiez shared/update.ts (voir le README).";
      action = null;
      break;
    case "checking":
      icon = <Loader2 size={16} className="animate-spin" />;
      title = "Recherche d'une mise à jour…";
      action = (
        <Button size="sm" disabled>
          <RefreshCw size={13} /> Rechercher
        </Button>
      );
      break;
    case "up-to-date":
      icon = <CheckCircle2 size={16} />;
      tone = "ok";
      title = "Batlay est à jour";
      detail = "Batlay vérifie tout seul au démarrage, sans rien télécharger sans vous.";
      action = checkButton;
      break;
    case "available":
      icon = <Sparkles size={16} />;
      tone = "new";
      title = (
        <>
          Batlay <span className="font-semibold">{status.version}</span> est disponible
        </>
      );
      detail = "Téléchargement en arrière-plan : vous pouvez continuer à utiliser Batlay.";
      action = (
        <Button size="sm" variant="primary" onClick={() => void download()}>
          <ArrowUpCircle size={14} /> Télécharger
        </Button>
      );
      break;
    case "downloading":
      icon = <Loader2 size={16} className="animate-spin" />;
      tone = "new";
      title = `Téléchargement… ${status.percent} %`;
      detail = (
        <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-base-800">
          <div
            className="h-full rounded-full transition-[width] duration-300"
            style={{
              width: `${status.percent}%`,
              backgroundImage: "linear-gradient(90deg, rgb(var(--signal-600)), rgb(var(--nova)))",
              boxShadow: "0 0 14px rgb(var(--signal-500) / 0.8)",
            }}
          />
        </div>
      );
      action = null;
      break;
    case "downloaded":
      icon = <Sparkles size={16} />;
      tone = "new";
      title = (
        <>
          Batlay <span className="font-semibold">{status.version}</span> est prêt à s'installer
        </>
      );
      detail = "Batlay se ferme, installe la mise à jour sans fenêtre supplémentaire, puis se rouvre tout seul.";
      action = (
        <Button size="sm" variant="primary" onClick={() => void install()}>
          <ArrowUpCircle size={14} /> Installer et redémarrer
        </Button>
      );
      break;
    case "error":
      icon = <AlertTriangle size={16} />;
      tone = "error";
      title = "La recherche a échoué";
      detail = <span className="text-danger">{status.message}</span>;
      action = checkButton;
      break;
    default:
      title = "Aucune recherche effectuée pour l'instant";
      action = checkButton;
  }

  const toneClass = {
    idle: "bg-base-800 text-muted ring-base-700",
    ok: "bg-live/12 text-ok ring-live/25",
    new: "bg-signal-600/15 text-accent ring-signal-500/30",
    error: "bg-red-500/10 text-danger ring-red-500/25",
  }[tone];

  return (
    <section id="updates" className="mt-4 card scroll-mt-6 overflow-hidden p-0">
      <div
        className="relative flex items-center gap-4 border-b border-line px-6 py-5"
        style={{ background: "radial-gradient(80% 160% at 0% 0%, rgb(var(--signal-600) / 0.2), transparent 60%), radial-gradient(60% 140% at 100% 0%, rgb(var(--nova) / 0.1), transparent 65%)" }}
      >
        <BrandMark className="h-12 w-12" />
        <div className="min-w-0 flex-1">
          <p className="flex items-center gap-2 font-display text-lg font-semibold text-fg">
            Batlay
            {shownVersion && <span className="chip-version rounded-full px-2 py-px font-mono text-[11px] font-medium">{shownVersion}</span>}
          </p>
          <p className="text-xs text-muted">Overlays musique, chat et alertes pour le streaming · Créateur : Adilbl</p>
        </div>
        {status.state !== "unavailable" && (
          <Button variant="ghost" size="sm" onClick={() => void openReleases()} title="Ouvrir les versions sur GitHub">
            <ExternalLink size={13} /> Notes de version
          </Button>
        )}
      </div>
      <div className="flex items-center gap-4 px-6 py-4">
        <span className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-xl ring-1 ring-inset ${toneClass}`}>{icon}</span>
        <div className="min-w-0 flex-1">
          <p className="text-sm text-fg">{title}</p>
          {detail && <div className="mt-0.5 text-xs text-muted">{detail}</div>}
        </div>
        {action && <div className="shrink-0">{action}</div>}
      </div>
    </section>
  );
}

function Toggle({
  label,
  hint,
  checked,
  onChange,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-center justify-between gap-4 py-2.5">
      <div className="min-w-0">
        <p className="text-sm text-fg">{label}</p>
        {hint && <p className="mt-0.5 text-xs text-muted">{hint}</p>}
      </div>
      <Switch checked={checked} onChange={onChange} label={label} />
    </div>
  );
}

/** Ligne « libellé + choix segmenté », comme le choix du thème. */
function ChoiceRow<T extends string>({ label, hint, value, options, onChange }: { label: string; hint: string; value: T; options: { value: T; label: string }[]; onChange: (v: T) => void }) {
  return (
    <div className="mt-4 border-t border-line pt-4">
      <div className="flex items-center justify-between gap-4 text-sm text-fg">
        <span>{label}</span>
        <div role="radiogroup" aria-label={label} className="seg flex gap-1 rounded-lg p-0.5">
          {options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="radio"
              aria-checked={value === o.value}
              onClick={() => onChange(o.value)}
              className={`seg-item rounded-md px-3 py-1 text-xs font-medium ${value === o.value ? "seg-active" : ""}`}
            >
              {o.label}
            </button>
          ))}
        </div>
      </div>
      <p className="mt-2 text-xs text-muted">{hint}</p>
    </div>
  );
}
