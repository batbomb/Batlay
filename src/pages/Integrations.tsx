import { Cast as PageIcon } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import {
  AlertTriangle,
  Check,
  Copy,
  ExternalLink,
  HelpCircle,
  Info,
  MessageSquare,
  Monitor,
  Music2,
  Ruler,
  Search,
  Smartphone,
  Square,
  X,
} from "lucide-react";
import { useOverlayStore } from "@/stores/useOverlayStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { useToastStore } from "@/stores/useToastStore";
import { Button, PageHeader, SectionTitle, StatusPill } from "@/components/ui";
import { loadOptions as loadChatOptions } from "@/components/ChatOverlaySection";
import { pickMainOverlay } from "@/utils/ui-helpers";
import { buildOverlaySearch } from "../../twitch-overlay/chat-feed";
import { TWITCH_OVERLAY_PATH } from "../../shared/twitch";
import { HOSTS_FILE_LINE, LOCAL_ALIAS_HOSTNAME, type LoopbackHostname } from "../../shared/loopback";
import type { LocalAliasStatus } from "@/types/global";
import {
  CANVAS_FORMATS,
  STATUS_LABELS,
  STATUS_TONES,
  TESTING_NOTE,
  buildDiagnostics,
  buildOverlayUrlVariant,
  chatOverlaySourceSize,
  dimensionsClipboardText,
  formatDimensions,
  getCanvasFormat,
  getPlatform,
  identifyOverlayUrl,
  isCanvasFormatId,
  listPlatforms,
  musicOverlaySourceSize,
  normalizePlatformId,
  placeInScene,
  type CanvasFormat,
  type CanvasFormatId,
  type Capability,
  type CheckStatus,
  type IntegrationPlatform,
  type PlatformId,
  type Size,
} from "@/integrations";

/**
 * Page « Intégrations » : comment utiliser les overlays Batlay dans OBS, TikTok
 * LIVE Studio, Streamlabs, XSplit ou tout logiciel à source Web. Elle n'ajoute
 * aucun rendu : la même URL (celle d'OBS) sert partout, seuls changent les
 * instructions, la taille conseillée et deux options d'URL facultatives
 * (variante 127.0.0.1, « Ajuster à la source »). Textes : src/integrations/platforms.ts.
 */

const PLATFORM_KEY = "batlay.integrations.platform";

function readStoredPlatform(): PlatformId {
  try {
    return normalizePlatformId(localStorage.getItem(PLATFORM_KEY));
  } catch {
    return normalizePlatformId(null);
  }
}
function storePlatform(id: PlatformId): void {
  try {
    localStorage.setItem(PLATFORM_KEY, id);
  } catch {
    /* stockage indisponible : le choix vaut pour cette session */
  }
}

const FORMAT_ICONS: Record<CanvasFormatId, ReactNode> = {
  landscape: <Monitor size={14} />,
  portrait: <Smartphone size={14} />,
  square: <Square size={14} />,
};

const CERTAINTY: Record<Capability["certainty"], { label: string; className: string }> = {
  verified: { label: "Vérifié", className: "text-ok" },
  expected: { label: "Attendu", className: "text-accent" },
  unknown: { label: "À vérifier", className: "text-warn-strong" },
};

const CHECK_STYLES: Record<CheckStatus, { icon: ReactNode; className: string }> = {
  ok: { icon: <Check size={14} />, className: "text-ok" },
  warn: { icon: <AlertTriangle size={14} />, className: "text-warn-strong" },
  error: { icon: <X size={14} />, className: "text-danger" },
  info: { icon: <Info size={14} />, className: "text-muted" },
};

export function Integrations() {
  const navigate = useNavigate();
  const { overlays, activeOverlayId } = useOverlayStore((s) => ({ overlays: s.overlays, activeOverlayId: s.activeOverlayId }));
  const { settings, musicServerUp } = useSettingsStore((s) => ({ settings: s.settings, musicServerUp: s.overlayServerRunning }));
  const twitch = useTwitchStore((s) => s.state);
  const push = useToastStore((s) => s.push);

  const [platformId, setPlatformId] = useState<PlatformId>(readStoredPlatform);
  const platform = getPlatform(platformId);
  const [formatId, setFormatId] = useState<CanvasFormatId>(platform.defaultFormat);
  /** `null` = hôte fourni par Batlay (batlay.test s'il est configuré, sinon localhost). */
  const [host, setHost] = useState<LoopbackHostname | null>(platform.preferredHost);
  const [alias, setAlias] = useState<LocalAliasStatus | null>(null);
  const aliasOk = alias?.state === "ok";
  useEffect(() => {
    let alive = true;
    window.batlay.integrations
      .checkLocalAlias()
      .then((s) => alive && setAlias(s))
      .catch(() => undefined);
    const off = window.batlay.integrations.onLocalAliasChange((s) => setAlias(s));
    return () => {
      alive = false;
      off();
    };
  }, []);
  const effectiveHost: LoopbackHostname = host ?? (aliasOk ? LOCAL_ALIAS_HOSTNAME : "localhost");
  const [fit, setFit] = useState(false);
  const [musicUrls, setMusicUrls] = useState<Record<string, string>>({});
  const [clientCounts, setClientCounts] = useState<Record<string, number> | null>(null);
  const chatOptions = useMemo(loadChatOptions, []);
  const format = getCanvasFormat(formatId);
  const mainId = pickMainOverlay(overlays, activeOverlayId)?.id ?? null;
  const musicPort = settings?.overlayServerPort ?? null;

  function choosePlatform(id: PlatformId) {
    const next = getPlatform(id);
    setPlatformId(next.id);
    storePlatform(next.id);
    // Chaque logiciel a son format habituel (portrait pour TikTok) et son adresse (batlay.test pour TikTok), modifiables ensuite.
    setFormatId(next.defaultFormat);
    setHost(next.preferredHost);
  }

  // URL existantes, fournies par le cœur de Batlay (même source que « Copier l'URL OBS »).
  const overlayIds = overlays.map((o) => o.id).join(",");
  useEffect(() => {
    let cancelled = false;
    if (!musicServerUp) {
      setMusicUrls({});
      return;
    }
    Promise.all(
      overlays.map(async (o) => {
        try {
          return [o.id, await window.batlay.overlay.getUrl(o.id)] as const;
        } catch {
          return null;
        }
      })
    ).then((entries) => {
      if (cancelled) return;
      setMusicUrls(Object.fromEntries(entries.filter((e): e is readonly [string, string] => e !== null)));
    });
    return () => {
      cancelled = true;
    };
    // overlayIds résume la liste : un renommage ne change pas les URL, inutile de les redemander.
  }, [overlayIds, musicServerUp, musicPort, aliasOk]);

  // Sources réellement connectées : relevé toutes les 2 s tant que la page est ouverte.
  useEffect(() => {
    let cancelled = false;
    async function poll() {
      try {
        const counts = await window.batlay.overlay.getClientCounts();
        if (!cancelled) setClientCounts(counts ?? {});
      } catch {
        if (!cancelled) setClientCounts(null);
      }
    }
    void poll();
    const timer = setInterval(poll, 2000);
    return () => {
      cancelled = true;
      clearInterval(timer);
    };
  }, []);

  const musicClientTotal = clientCounts === null ? null : Object.values(clientCounts).reduce((a, b) => a + b, 0);
  const chatBaseUrl = `${twitch.server.overlayUrl}${buildOverlaySearch(chatOptions)}`;

  const diagnostics = buildDiagnostics({
    platform,
    musicServerRunning: musicServerUp,
    musicPort,
    overlayCount: overlays.length,
    musicClients: musicClientTotal,
    chatServerRunning: twitch.server.running,
    chatPort: twitch.server.port,
    chatClients: twitch.overlayClients,
  });

  async function copy(text: string | null, success: string) {
    if (!text) {
      push("Rien à copier : le serveur correspondant est-il démarré ?", "error");
      return;
    }
    try {
      await navigator.clipboard.writeText(text);
      push(success, "success");
    } catch {
      push("Copie impossible : sélectionnez le texte et copiez-le à la main.", "error");
    }
  }

  async function openDocs() {
    try {
      const opened = await window.batlay.integrations.openDocs(platform.id);
      if (!opened) push("Aucune documentation officielle n'est disponible pour ce logiciel.", "error");
    } catch {
      push("Impossible d'ouvrir le navigateur.", "error");
    }
  }

  return (
    <div className="stagger mx-auto max-w-6xl px-8 py-10">
      <PageHeader icon={PageIcon} eyebrow="Diffusion"
        title="Intégrations"
        subtitle="Les mêmes overlays, dans OBS, TikTok LIVE Studio, Streamlabs, XSplit ou tout logiciel qui affiche une page Web."
      />

      <p className="mt-4 flex items-start gap-2 rounded-xl border border-line bg-base-900/60 px-4 py-3 text-xs text-muted">
        <Info size={14} className="mt-0.5 shrink-0 text-accent" />
        <span>{TESTING_NOTE}</span>
      </p>

      <div className="mt-6 grid gap-6 lg:grid-cols-[260px_minmax(0,1fr)]">
        {/* --- Liste des logiciels --- */}
        <nav aria-label="Logiciels de streaming" className="flex flex-col gap-2">
          {listPlatforms().map((p) => {
            const selected = p.id === platform.id;
            return (
              <button
                key={p.id}
                type="button"
                aria-pressed={selected}
                onClick={() => choosePlatform(p.id)}
                className={`card card-hover flex flex-col items-start gap-1.5 p-3.5 text-left transition ${
                  selected ? "ring-2 ring-signal-500/60" : ""
                }`}
              >
                <span className="font-medium text-fg">{p.name}</span>
                <StatusPill tone={STATUS_TONES[p.status]}>{STATUS_LABELS[p.status]}</StatusPill>
                <span className="text-[11px] text-faint">Source : {p.sourceName}</span>
              </button>
            );
          })}
        </nav>

        <div className="min-w-0 space-y-6">
          <PlatformSummary platform={platform} onOpenDocs={() => void openDocs()} />

          {/* --- URL et tailles --- */}
          <section className="card p-6" aria-labelledby="integration-urls">
            <SectionTitle hint={`Une seule URL par overlay, valable dans tous les logiciels. Taille à saisir : ${platform.sizeField}.`}>
              <span id="integration-urls">Vos overlays pour {platform.name}</span>
            </SectionTitle>

            <div className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-3 text-sm">
              <FormatPicker value={formatId} onChange={(id) => isCanvasFormatId(id) && setFormatId(id)} />
              <HostPicker value={effectiveHost} onChange={setHost} />
              <label
                className="flex cursor-pointer items-center gap-2 text-muted"
                title="Ajoute fit=contain : l'overlay musical s'adapte à la taille de la source, proportions gardées."
              >
                <input
                  type="checkbox"
                  checked={fit}
                  onChange={(e) => setFit(e.target.checked)}
                  className="h-4 w-4 accent-[rgb(var(--signal-500))]"
                />
                Ajuster à la source
                {platform.recommendFit && <span className="text-[11px] text-faint">(conseillé si la taille ne peut pas être fixée)</span>}
              </label>
            </div>

            <LocalAliasPanel
              status={alias}
              required={effectiveHost === LOCAL_ALIAS_HOSTNAME}
              onStatus={setAlias}
              onCopy={(text, msg) => void copy(text, msg)}
              onToast={push}
            />

            <div className="mt-5 space-y-3">
              {overlays.length === 0 && (
                <div className="flex items-center justify-between gap-3 rounded-xl border border-dashed border-line px-4 py-3 text-sm text-muted">
                  Aucun overlay musical pour l'instant.
                  <Button size="sm" variant="primary" onClick={() => navigate("/overlays")}>
                    Créer un overlay
                  </Button>
                </div>
              )}
              {overlays.map((overlay) => {
                const size = musicOverlaySourceSize(overlay.theme);
                const base = musicUrls[overlay.id];
                const url = base ? buildOverlayUrlVariant(base, { host: effectiveHost, fit }) : null;
                return (
                  <OverlayRow
                    key={overlay.id}
                    icon={<Music2 size={15} />}
                    name={overlay.name}
                    badge={overlay.id === mainId ? "Principal" : undefined}
                    url={url}
                    unavailable={musicServerUp ? "Récupération de l'URL…" : "Serveur d'overlay musical arrêté (Paramètres)."}
                    size={size}
                    sizeNote={fit ? "Avec « Ajuster à la source », toute taille de source garde les proportions du canvas." : "Taille du canvas de l'overlay (éditeur)."}
                    format={format}
                    clients={clientCounts === null ? null : (clientCounts[overlay.id] ?? 0)}
                    onCopyUrl={() => void copy(url, `URL de « ${overlay.name} » copiée — collez-la dans la source « ${platform.sourceName} » de ${platform.name}.`)}
                    onCopySize={() => void copy(dimensionsClipboardText(size), `Taille ${formatDimensions(size)} copiée.`)}
                  />
                );
              })}
              {(() => {
                const size = chatOverlaySourceSize(chatOptions, formatId);
                const url = twitch.server.running ? buildOverlayUrlVariant(chatBaseUrl, { host: effectiveHost }) : null;
                const auto = chatOptions.width === 0 || chatOptions.height === 0;
                return (
                  <OverlayRow
                    icon={<MessageSquare size={15} />}
                    name="Chat Twitch"
                    url={url}
                    unavailable="Serveur du chat arrêté."
                    size={size}
                    sizeNote={
                      auto
                        ? `Zone « Auto » dans Overlays → Chat : taille conseillée pour le format ${format.label.toLowerCase()}.`
                        : "Zone choisie dans Overlays → Chat."
                    }
                    format={format}
                    clients={twitch.overlayClients}
                    onCopyUrl={() => void copy(url, `URL du chat copiée — collez-la dans la source « ${platform.sourceName} » de ${platform.name}.`)}
                    onCopySize={() => void copy(dimensionsClipboardText(size), `Taille ${formatDimensions(size)} copiée.`)}
                  />
                );
              })()}
            </div>
            <p className="mt-3 text-xs text-faint">
              Les liens existants ne changent pas : l'adresse ({LOCAL_ALIAS_HOSTNAME}, 127.0.0.1) et « Ajuster à la source » sont des variantes du même lien. Pour invalider un lien,
              utilisez toujours « Régénérer le lien » (Overlays ou Paramètres) : il s'applique à tous les logiciels à la fois.
            </p>
          </section>

          {/* Mode d'emploi après les URL : ce qu'on copie d'abord, le détail ensuite. */}
          <PlatformGuide platform={platform} />
          <Diagnostics checks={diagnostics} />
          <UrlIdentifier
            musicPort={musicPort}
            chatPort={twitch.server.port}
            overlays={overlays}
          />
        </div>
      </div>
    </div>
  );
}

function PlatformSummary({ platform, onOpenDocs }: { platform: IntegrationPlatform; onOpenDocs: () => void }) {
  return (
    <section className="card p-6" aria-labelledby="integration-platform">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 id="integration-platform" className="font-display text-xl font-semibold text-fg">
            {platform.name}
          </h2>
          <div className="mt-1.5">
            <StatusPill tone={STATUS_TONES[platform.status]}>
              {platform.status === "partial" && <AlertTriangle size={11} />}
              {STATUS_LABELS[platform.status]}
            </StatusPill>
          </div>
        </div>
        {platform.docs && (
          <Button size="sm" onClick={onOpenDocs} title={platform.docs.url}>
            <ExternalLink size={14} /> {platform.docs.label}
          </Button>
        )}
      </div>
      <p className="mt-3 text-sm text-muted">{platform.statusDetail}</p>
      {platform.docs?.kind === "vendor-download" && (
        <p className="mt-1 text-xs text-faint">L'éditeur ne publie pas de documentation de cette méthode : le bouton mène à sa page officielle.</p>
      )}

      <p className="mt-4 text-sm text-fg">
        <span className="text-muted">Méthode : </span>
        {platform.method}
      </p>
    </section>
  );
}

function PlatformGuide({ platform }: { platform: IntegrationPlatform }) {
  return (
    <section className="card p-6" aria-labelledby="integration-guide">
      <SectionTitle hint={`Source « ${platform.sourceName} »`}>
        <span id="integration-guide">Configurer {platform.name}</span>
      </SectionTitle>
      <dl className="mt-4 grid gap-3 sm:grid-cols-3">
        <CapabilityItem title="Mises à jour en direct" capability={platform.liveUpdates} />
        <CapabilityItem title="Fond transparent" capability={platform.transparency} />
        <CapabilityItem title="Adresse locale" capability={platform.localAccess} />
      </dl>

      <h3 className="mt-6 text-sm font-semibold text-fg">Étapes</h3>
      <ol className="mt-2 list-decimal space-y-1.5 pl-5 text-sm text-muted marker:text-faint">
        {platform.steps.map((step) => (
          <li key={step}>{step}</li>
        ))}
      </ol>

      {platform.limitations.length > 0 && (
        <>
          <h3 className="mt-6 text-sm font-semibold text-fg">Limites connues</h3>
          <ul className="mt-2 space-y-1.5 text-sm text-muted">
            {platform.limitations.map((item) => (
              <li key={item} className="flex gap-2">
                <AlertTriangle size={13} className="mt-1 shrink-0 text-warn-strong" />
                <span>{item}</span>
              </li>
            ))}
          </ul>
        </>
      )}

      {platform.alternatives.map((alt) => (
        <details key={alt.title} className="mt-4 rounded-xl border border-line bg-base-900/40 px-4 py-3">
          <summary className="cursor-pointer text-sm font-medium text-fg">{alt.title}</summary>
          <ol className="mt-2 list-decimal space-y-1.5 pl-5 text-sm text-muted marker:text-faint">
            {alt.steps.map((step) => (
              <li key={step}>{step}</li>
            ))}
          </ol>
          <ul className="mt-2 space-y-1 text-xs text-faint">
            {alt.caveats.map((c) => (
              <li key={c} className="flex gap-2">
                <HelpCircle size={12} className="mt-0.5 shrink-0" />
                <span>{c}</span>
              </li>
            ))}
          </ul>
        </details>
      ))}
    </section>
  );
}

function CapabilityItem({ title, capability }: { title: string; capability: Capability }) {
  const certainty = CERTAINTY[capability.certainty];
  return (
    <div className="rounded-xl border border-line bg-base-900/40 p-3">
      <dt className="flex items-center justify-between gap-2 text-xs font-medium text-fg">
        {title}
        <span className={`text-[11px] ${certainty.className}`}>{certainty.label}</span>
      </dt>
      <dd className="mt-1 text-xs text-muted">{capability.detail}</dd>
    </div>
  );
}

function HostPicker({ value, onChange }: { value: LoopbackHostname; onChange: (host: LoopbackHostname) => void }) {
  const options: { id: LoopbackHostname; label: string; title: string }[] = [
    { id: "localhost", label: "localhost", title: "Adresse historique (OBS, Streamlabs, XSplit)" },
    { id: "127.0.0.1", label: "127.0.0.1", title: "Même serveur, écrit en adresse IP" },
    { id: LOCAL_ALIAS_HOSTNAME, label: LOCAL_ALIAS_HOSTNAME, title: "Nom local accepté par TikTok LIVE Studio (fichier hosts requis)" },
  ];
  return (
    <div role="radiogroup" aria-label="Adresse dans l'URL" className="inline-flex rounded-lg border border-base-700 bg-base-800 p-0.5">
      {options.map((o) => (
        <button
          key={o.id}
          type="button"
          role="radio"
          aria-checked={value === o.id}
          title={o.title}
          onClick={() => onChange(o.id)}
          className={`rounded-md px-2.5 py-1 font-mono text-xs transition ${value === o.id ? "bg-base-900 text-fg shadow-sm" : "text-muted hover:text-fg"}`}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

/**
 * Nom local batlay.test : état (lecture seule), configuration automatique
 * (Windows demande l'autorisation) et marche à suivre manuelle en secours.
 */
function LocalAliasPanel({
  status,
  required,
  onStatus,
  onCopy,
  onToast,
}: {
  status: LocalAliasStatus | null;
  /** L'URL affichée utilise batlay.test : sans configuration, elle ne fonctionnera pas. */
  required: boolean;
  onStatus: (s: LocalAliasStatus) => void;
  onCopy: (text: string, message: string) => void;
  onToast: (message: string, tone: "success" | "error") => void;
}) {
  const [busy, setBusy] = useState(false);
  const ok = status?.state === "ok";

  async function run(kind: "setup" | "remove" | "check") {
    setBusy(true);
    try {
      if (kind === "check") {
        onStatus(await window.batlay.integrations.checkLocalAlias());
        return;
      }
      const outcome = kind === "setup" ? await window.batlay.integrations.setupLocalAlias() : await window.batlay.integrations.removeLocalAlias();
      onStatus(outcome.status);
      const messages: Record<string, [string, "success" | "error"]> = {
        done: [kind === "setup" ? `${LOCAL_ALIAS_HOSTNAME} est configuré : vos URL l'utilisent désormais.` : `${LOCAL_ALIAS_HOSTNAME} retiré : les URL reviennent à localhost.`, "success"],
        already: [kind === "setup" ? `${LOCAL_ALIAS_HOSTNAME} était déjà configuré.` : "Rien à retirer.", "success"],
        cancelled: ["Autorisation refusée dans la fenêtre de Windows : rien n'a été modifié.", "error"],
        unsupported: ["Configuration automatique disponible sous Windows uniquement.", "error"],
      };
      const [message, tone] = outcome.result === "failed" ? [outcome.message, "error" as const] : messages[outcome.result];
      onToast(message, tone);
    } catch (err) {
      onToast((err as Error).message || "Opération impossible.", "error");
    } finally {
      setBusy(false);
    }
  }

  if (ok) {
    return (
      <div className="mt-4 flex flex-wrap items-center justify-between gap-2 rounded-xl border border-live/30 bg-live/5 px-4 py-2.5 text-sm">
        <p className="flex items-center gap-2 font-medium text-ok">
          <Check size={14} /> {LOCAL_ALIAS_HOSTNAME} pointe vers ce PC : vos URL l'utilisent.
        </p>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void run("remove")} title="Retire la ligne du fichier hosts (Windows demande l'autorisation)">
          Retirer
        </Button>
      </div>
    );
  }

  const problem =
    status === null
      ? `Vérification de ${LOCAL_ALIAS_HOSTNAME}…`
      : status.state === "missing"
        ? `${LOCAL_ALIAS_HOSTNAME} n'est pas encore configuré sur ce PC`
        : status.state === "wrong-address"
          ? `${LOCAL_ALIAS_HOSTNAME} pointe vers ${status.addresses.join(", ")} au lieu de 127.0.0.1`
          : `Vérification impossible : ${status.message}`;

  return (
    <div className={`mt-4 rounded-xl border px-4 py-3 text-sm ${required ? "border-amber-500/30 bg-amber-500/5" : "border-line bg-base-900/40"}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className={`flex items-center gap-2 font-medium ${required ? "text-warn-strong" : "text-fg"}`}>
          {required ? <AlertTriangle size={14} /> : <Info size={14} className="text-accent" />}
          {problem}
        </p>
        <div className="flex gap-2">
          <Button size="sm" variant="primary" disabled={busy} onClick={() => void run("setup")}>
            {busy ? "En attente de Windows…" : "Configurer automatiquement"}
          </Button>
          <Button size="sm" disabled={busy} onClick={() => void run("check")}>
            Vérifier
          </Button>
        </div>
      </div>
      <p className="mt-2 text-xs text-muted">
        {required
          ? "TikTok LIVE Studio refuse « localhost » et « 127.0.0.1 » : cette URL utilise "
          : "Pour des URL plus lisibles (et compatibles TikTok LIVE Studio), Batlay peut utiliser "}
        <span className="font-mono text-fg">{LOCAL_ALIAS_HOSTNAME}</span>. Un clic ajoute la ligne <span className="font-mono text-fg">{HOSTS_FILE_LINE}</span> au fichier
        hosts de Windows, qui demande votre autorisation. Ce nom réservé n'existe pas sur Internet : il ne désigne que ce PC.
      </p>
      <details className="mt-2 text-xs text-muted">
        <summary className="cursor-pointer hover:text-fg">Le faire à la main</summary>
        <ol className="mt-2 list-decimal space-y-1 pl-5 marker:text-faint">
          <li>Menu Démarrer → tapez « Bloc-notes » → clic droit → « Exécuter en tant qu'administrateur ».</li>
          <li>
            Fichier → Ouvrir → <code className="font-mono text-fg">C:\Windows\System32\drivers\etc\hosts</code> (choisissez « Tous les fichiers »).
          </li>
          <li>
            Ajoutez à la fin la ligne <code className="font-mono text-fg">{HOSTS_FILE_LINE}</code>, enregistrez, puis cliquez sur « Vérifier ».
          </li>
        </ol>
        <div className="mt-2 flex flex-wrap gap-2">
          <Button size="sm" onClick={() => onCopy(HOSTS_FILE_LINE, "Ligne copiée — collez-la à la fin du fichier hosts.")}>
            <Copy size={14} /> Copier la ligne
          </Button>
          <Button size="sm" onClick={() => onCopy("C:\\Windows\\System32\\drivers\\etc\\hosts", "Chemin du fichier hosts copié.")}>
            <Copy size={14} /> Copier le chemin
          </Button>
        </div>
        <p className="mt-2 text-[11px] text-faint">Un antivirus peut demander de confirmer la modification du fichier hosts : c'est normal.</p>
      </details>
    </div>
  );
}

function FormatPicker({ value, onChange }: { value: CanvasFormatId; onChange: (id: string) => void }) {
  return (
    <div role="radiogroup" aria-label="Format de la scène" className="inline-flex rounded-lg border border-base-700 bg-base-800 p-0.5">
      {CANVAS_FORMATS.map((f) => (
        <button
          key={f.id}
          type="button"
          role="radio"
          aria-checked={value === f.id}
          title={`${f.width} × ${f.height} — ${f.hint}`}
          onClick={() => onChange(f.id)}
          className={`inline-flex items-center gap-1.5 rounded-md px-2.5 py-1 text-xs font-medium transition ${
            value === f.id ? "bg-base-900 text-fg shadow-sm" : "text-muted hover:text-fg"
          }`}
        >
          {FORMAT_ICONS[f.id]}
          {f.label} <span className="text-faint">{f.width}×{f.height}</span>
        </button>
      ))}
    </div>
  );
}

function OverlayRow({
  icon,
  name,
  badge,
  url,
  unavailable,
  size,
  sizeNote,
  format,
  clients,
  onCopyUrl,
  onCopySize,
}: {
  icon: ReactNode;
  name: string;
  badge?: string;
  url: string | null;
  unavailable: string;
  size: Size;
  sizeNote: string;
  format: CanvasFormat;
  clients: number | null;
  onCopyUrl: () => void;
  onCopySize: () => void;
}) {
  const placement = placeInScene(size, format);
  return (
    <div className="rounded-xl border border-line bg-base-900/40 p-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="flex min-w-0 items-center gap-2 font-medium text-fg">
          <span className="text-accent">{icon}</span>
          <span className="truncate">{name}</span>
          {badge && <StatusPill>{badge}</StatusPill>}
        </p>
        <StatusPill tone={clients && clients > 0 ? "ok" : "neutral"}>
          {clients === null ? "Sources : ?" : `${clients} source${clients > 1 ? "s" : ""} connectée${clients > 1 ? "s" : ""}`}
        </StatusPill>
      </div>

      <div className="mt-3 flex flex-wrap items-center gap-2">
        <input
          readOnly
          value={url ?? unavailable}
          aria-label={`URL de l'overlay ${name}`}
          onFocus={(e) => e.currentTarget.select()}
          className={`min-w-0 flex-1 rounded-lg border border-base-700 bg-base-800 px-3 py-2 font-mono text-xs outline-none focus:border-signal-500 ${
            url ? "text-fg" : "text-faint"
          }`}
        />
        <Button size="sm" onClick={onCopyUrl} disabled={!url}>
          <Copy size={14} /> Copier l'URL
        </Button>
        <Button size="sm" onClick={onCopySize} title="Copie « largeurxhauteur »">
          <Ruler size={14} /> Copier la taille
        </Button>
      </div>

      <p className="mt-2 text-xs text-muted">
        Taille de la source : <span className="font-medium text-fg">{formatDimensions(size)}</span> · {sizeNote}
      </p>
      <p className="mt-0.5 text-xs text-faint">
        Scène {format.label.toLowerCase()} {format.width} × {format.height} :{" "}
        {placement.fitsNatively
          ? "tient à sa taille réelle."
          : `trop grand — réduisez la source à ${Math.round(placement.scale * 100)} % (${formatDimensions(placement.displayed)}) dans la scène.`}
      </p>
    </div>
  );
}

function Diagnostics({ checks }: { checks: ReturnType<typeof buildDiagnostics> }) {
  return (
    <section className="card p-6" aria-labelledby="integration-diagnostics">
      <SectionTitle hint="Mis à jour toutes les 2 secondes. Une source « connectée » a chargé la page et reçoit les mises à jour en direct.">
        <span id="integration-diagnostics">Vérifications</span>
      </SectionTitle>
      <ul className="mt-4 space-y-2.5">
        {checks.map((check) => {
          const style = CHECK_STYLES[check.status];
          return (
            <li key={check.id} className="flex gap-2.5 text-sm">
              <span className={`mt-0.5 shrink-0 ${style.className}`}>{style.icon}</span>
              <span className="min-w-0">
                <span className="text-fg">{check.label}</span>
                {check.hint && <span className="block text-xs text-muted">{check.hint}</span>}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function UrlIdentifier({
  musicPort,
  chatPort,
  overlays,
}: {
  musicPort: number | null;
  chatPort: number;
  overlays: readonly { id: string; name: string }[];
}) {
  const [input, setInput] = useState("");
  const result =
    input.trim() === "" || musicPort === null
      ? null
      : identifyOverlayUrl(input, { musicPort, chatPort, chatPath: TWITCH_OVERLAY_PATH, overlays });

  let message: ReactNode = null;
  if (result?.kind === "music") {
    message = result.overlayName ? (
      <span className="text-ok">
        Overlay musical « {result.overlayName} »{result.fit === "contain" ? ", avec « Ajuster à la source »" : ""}. Lien valide.
      </span>
    ) : (
      <span className="text-danger">
        Lien révoqué ou overlay supprimé : la source n'affichera rien. Copiez l'URL actuelle de l'overlay ci-dessus.
      </span>
    );
  } else if (result?.kind === "chat") {
    message = <span className="text-ok">Overlay Chat Twitch{result.search ? " (avec réglages dans l'URL)" : ""}.</span>;
  } else if (result?.kind === "invalid") {
    message = <span className="text-warn-strong">{result.reason}</span>;
  }

  return (
    <section className="card p-6" aria-labelledby="integration-identify">
      <SectionTitle hint="Collez l'URL trouvée dans votre logiciel de streaming : Batlay indique l'overlay qu'elle affiche et si le lien est encore valide.">
        <span id="integration-identify">Quel overlay est derrière cette URL ?</span>
      </SectionTitle>
      <div className="mt-4 flex items-center gap-2">
        <Search size={15} className="shrink-0 text-muted" />
        <input
          value={input}
          onChange={(e) => setInput(e.target.value.slice(0, 2048))}
          placeholder="http://localhost:3000/overlay/…"
          aria-label="URL à identifier"
          spellCheck={false}
          className="min-w-0 flex-1 rounded-lg border border-base-700 bg-base-800 px-3 py-2 font-mono text-xs text-fg outline-none focus:border-signal-500"
        />
      </div>
      {message && <p className="mt-2 text-sm">{message}</p>}
    </section>
  );
}
