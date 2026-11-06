import { Bell as PageIcon } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState, type ChangeEvent } from "react";
import { useNavigate } from "react-router-dom";
import { Bell, Copy, Gift, Heart, ImagePlus, Music4, Play, RefreshCw, SkipForward, Star, Trash2, Volume2, type LucideIcon } from "lucide-react";
import { Button, PageHeader, SectionTitle, StatusPill } from "@/components/ui";
import { SegmentedField, SelectField, SliderField, TextField, ToggleField, ColorField } from "@/components/editor/fields";
import { useToastStore } from "@/stores/useToastStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { TikTokLivePanel } from "@/components/TikTokLivePanel";
import { enterEffectOptions, exitEffectOptions } from "@/animation/effects";
import { AlertPlayer } from "@/alerts/AlertPlayer";
import { playChime, playFile } from "@/alerts/sound";
import {
  ALERT_KINDS,
  ALERT_KIND_LABELS,
  normalizeAlertsConfig,
  sampleAlert,
  type AlertKind,
  type AlertStyle,
  type AlertsConfig,
} from "../../shared/alerts";

/**
 * Page « Alertes » : nouveaux followers et abonnés (Twitch et TikTok),
 * personnalisables (texte, image, son, animation, couleurs), aperçu immédiat.
 * Les réglages sont enregistrés automatiquement et poussés en direct à la page
 * des alertes ouverte dans le logiciel de streaming.
 */

const MAX_FILE = 8 * 1024 * 1024;
const KIND_ICONS: Record<AlertKind, LucideIcon> = { follow: Heart, subscribe: Star, resub: RefreshCw, gift: Gift, tiktokGift: Gift };
const VARIABLES = "Variables : {user} {amount} {months} {tier} {message} {gift} {platform}";
const seconds = (ms: number) => `${(ms / 1000).toFixed(ms % 1000 === 0 ? 0 : 1)} s`;

type Loaded = { config: AlertsConfig; urls: { page: string; hook: string }; pages: number };

export function Alerts() {
  const navigate = useNavigate();
  const push = useToastStore((s) => s.push);
  const twitch = useTwitchStore((s) => s.state);
  const [data, setData] = useState<Loaded | null>(null);
  const [kind, setKind] = useState<AlertKind>("follow");
  const [media, setMedia] = useState<Record<string, string>>({});
  const [replay, setReplay] = useState(0);
  const [withSound, setWithSound] = useState(false);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Chargement, puis relevé du nombre de pages d'alertes ouvertes.
  useEffect(() => {
    let alive = true;
    const load = () =>
      window.batlay.alerts
        .get()
        .then((d) => alive && setData((cur) => (cur ? { ...cur, pages: d.pages, urls: d.urls } : d)))
        .catch(() => undefined);
    void load();
    const t = setInterval(load, 3000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  // Aperçus des fichiers importés (data URL : la CSP du tableau de bord n'autorise pas le serveur local).
  const neededMedia = useMemo(() => {
    if (!data) return [];
    return ALERT_KINDS.flatMap((k) => {
      const s = data.config.kinds[k];
      return [s.imageId, s.sound.type === "file" ? s.sound.mediaId : null].filter((x): x is string => !!x);
    });
  }, [data]);
  useEffect(() => {
    for (const id of neededMedia) {
      if (media[id]) continue;
      void window.batlay.alerts.mediaDataUrl(id).then((url) => url && setMedia((m) => ({ ...m, [id]: url })));
    }
  }, [neededMedia, media]);

  const update = useCallback((next: AlertsConfig) => {
    const normalized = normalizeAlertsConfig(next);
    setData((cur) => (cur ? { ...cur, config: normalized } : cur));
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(() => void window.batlay.alerts.save(normalized), 350);
  }, []);
  useEffect(() => () => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
  }, []);

  if (!data) return <div className="px-8 py-10 text-sm text-muted">Chargement…</div>;
  const { config, urls, pages } = data;
  const style = config.kinds[kind];
  const setStyle = (patch: Partial<AlertStyle>) => update({ ...config, kinds: { ...config.kinds, [kind]: { ...style, ...patch } } });

  async function copy(text: string, message: string) {
    try {
      await navigator.clipboard.writeText(text);
      push(message, "success");
    } catch {
      push("Copie impossible : sélectionnez le texte et copiez-le à la main.", "error");
    }
  }

  async function test(k: AlertKind) {
    const r = await window.batlay.alerts.test(k);
    if (r.shown) push(`Alerte « ${ALERT_KIND_LABELS[k]} » envoyée à ${pages} page${pages > 1 ? "s" : ""}.`, "success");
    else if (r.reason === "filtered") push("Ce type d'alerte est désactivé.", "error");
    else push("Aucune page d'alertes ouverte : ajoutez l'URL des alertes dans votre logiciel de streaming.", "error");
  }

  async function importFile(e: ChangeEvent<HTMLInputElement>, expected: "image" | "sound") {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    if (file.size > MAX_FILE) {
      push("Fichier trop lourd (8 Mo maximum).", "error");
      return;
    }
    const r = await window.batlay.alerts.importMedia(new Uint8Array(await file.arrayBuffer()), expected);
    if (!r.ok) {
      push(r.reason, "error");
      return;
    }
    if (expected === "image") setStyle({ imageId: r.id });
    else setStyle({ sound: { type: "file", mediaId: r.id } });
    push(`${expected === "image" ? "Image" : "Son"} importé${expected === "image" ? "e" : ""}.`, "success");
  }

  function listen() {
    if (style.sound.type === "chime") playChime(style.volume);
    else if (style.sound.type === "file" && media[style.sound.mediaId]) playFile(media[style.sound.mediaId], style.volume);
  }

  const ownChannel = twitch.status === "connected" && (!twitch.channel || !twitch.account || twitch.channel.id === twitch.account.id);
  const scopesOk = twitch.alertScopes.follows && twitch.alertScopes.subscriptions;
  const sample = sampleAlert(kind, `preview-${kind}-${replay}`, 0);

  return (
    <div className="stagger mx-auto max-w-6xl px-8 py-10">
      <PageHeader icon={PageIcon} eyebrow="Diffusion"
        title="Alertes"
        subtitle="Nouveaux followers, abonnés et cadeaux, sur Twitch et TikTok : texte, image, son et animation à votre goût."
        actions={
          <>
            <Button onClick={() => void window.batlay.alerts.skip()} title="Arrête l'alerte en cours et vide la file d'attente">
              <SkipForward size={14} /> Passer
            </Button>
            <Button variant="primary" onClick={() => void test(kind)}>
              <Play size={14} /> Tester « {ALERT_KIND_LABELS[kind]} »
            </Button>
          </>
        }
      />

      {/* --- Page à ajouter dans le logiciel de streaming --- */}
      <section className="card mt-6 p-6">
        <SectionTitle
          hint="Une seule source pour toutes les alertes. Source Web plein écran (1920 × 1080, ou 1080 × 1920 en portrait) : l'alerte apparaît au centre, fond transparent."
          actions={<StatusPill tone={pages > 0 ? "ok" : "neutral"}>{pages > 0 ? `${pages} page${pages > 1 ? "s" : ""} ouverte${pages > 1 ? "s" : ""}` : "Aucune page ouverte"}</StatusPill>}
        >
          Page des alertes
        </SectionTitle>
        <div className="mt-4 flex items-center gap-2">
          <input
            readOnly
            value={urls.page}
            aria-label="URL de la page des alertes"
            onFocus={(e) => e.currentTarget.select()}
            className="min-w-0 flex-1 rounded-lg border border-base-700 bg-base-800 px-3 py-2 font-mono text-xs text-fg outline-none focus:border-signal-500"
          />
          <Button onClick={() => void copy(urls.page, "URL des alertes copiée — collez-la dans une source Navigateur (OBS) ou Lien (TikTok LIVE Studio).")}>
            <Copy size={14} /> Copier l'URL
          </Button>
        </div>
      </section>

      {/* --- Sources --- */}
      <div className="mt-6 grid gap-6 md:grid-cols-2">
        <section className="card p-6">
          <SectionTitle actions={<ToggleField label="Activé" checked={config.sources.twitch} onChange={(twitchOn) => update({ ...config, sources: { ...config.sources, twitch: twitchOn } })} />}>
            Twitch
          </SectionTitle>
          {twitch.status !== "connected" ? (
            <div className="mt-3 space-y-2 text-sm text-muted">
              <p>Connectez votre compte Twitch pour recevoir vos follows et abonnements.</p>
              <Button size="sm" variant="primary" onClick={() => navigate("/chat")}>
                Connecter Twitch
              </Button>
            </div>
          ) : (
            <div className="mt-3 space-y-2 text-sm">
              <div className="flex flex-wrap gap-2">
                <StatusPill tone={twitch.alertScopes.follows ? "ok" : "warn"}>Follows {twitch.alertScopes.follows ? "autorisés" : "non autorisés"}</StatusPill>
                <StatusPill tone={twitch.alertScopes.subscriptions ? "ok" : "warn"}>Abonnements {twitch.alertScopes.subscriptions ? "autorisés" : "non autorisés"}</StatusPill>
              </div>
              {!scopesOk && (
                <>
                  <p className="text-xs text-muted">
                    Batlay a besoin de deux permissions de plus, en lecture seule (followers et abonnements). Reconnectez votre compte une fois et
                    acceptez-les : le chat continue de fonctionner pendant ce temps.
                  </p>
                  <Button
                    size="sm"
                    variant="primary"
                    onClick={() => {
                      void window.batlay.twitch.reauthorize();
                      push("Acceptez les permissions dans le navigateur qui vient de s'ouvrir.", "success");
                    }}
                  >
                    <RefreshCw size={14} /> Reconnecter Twitch
                  </Button>
                </>
              )}
              {!ownChannel && (
                <p className="text-xs text-warn-strong">
                  Le chat affiché est celui d'une autre chaîne : Twitch ne donne les follows et abonnements que de votre propre chaîne.
                </p>
              )}
              <p className="text-[11px] text-faint">Abonnements « Prime » inclus. Un abonnement offert à plusieurs personnes donne une seule alerte.</p>
              {ownChannel && twitch.channel && (
                <div className="mt-2 flex items-center gap-3 rounded-lg border border-base-700/70 bg-base-800/40 px-3 py-2">
                  {twitch.channel.profileImageUrl ? (
                    <img src={twitch.channel.profileImageUrl} alt="" className="h-8 w-8 rounded-full object-cover" />
                  ) : (
                    <span className="btn-primary flex h-8 w-8 items-center justify-center rounded-full text-sm font-bold">{twitch.channel.displayName.slice(0, 1).toUpperCase()}</span>
                  )}
                  <div className="min-w-0 flex-1">
                    <p className="truncate text-sm font-medium text-fg">{twitch.channel.displayName}</p>
                    <p className="text-[11px] text-muted">Alertes reçues pour cette chaîne</p>
                  </div>
                  <Button size="sm" variant="ghost" onClick={() => navigate("/chat")}>
                    Chat Twitch
                  </Button>
                </div>
              )}
            </div>
          )}
          <div className="mt-4 border-t border-line pt-3">
            <p className="mb-2 text-[11px] text-muted">Essayer une alerte dans votre logiciel :</p>
            <div className="flex flex-wrap gap-1.5">
              {(["follow", "subscribe", "resub", "gift"] as const).map((k) => {
                const Icon = KIND_ICONS[k];
                return (
                  <Button key={k} size="sm" variant="ghost" onClick={() => void test(k)} title={`Envoyer une alerte « ${ALERT_KIND_LABELS[k]} » de test`}>
                    <Icon size={13} /> {ALERT_KIND_LABELS[k]}
                  </Button>
                );
              })}
            </div>
          </div>
        </section>

        <TikTokLivePanel
          sourceEnabled={config.sources.tiktok}
          onSourceEnabled={(tiktokOn) => update({ ...config, sources: { ...config.sources, tiktok: tiktokOn } })}
          hookUrl={urls.hook}
          copy={copy}
          onRegenerateKey={async () => {
            const next = await window.batlay.alerts.regenerateKey();
            setData((cur) => (cur ? { ...cur, urls: next } : cur));
            push("Nouvelle clé : mettez à jour l'adresse dans votre outil TikTok.", "success");
          }}
        />
      </div>

      {/* --- Personnalisation --- */}
      <section className="card mt-6 p-6">
        <SectionTitle hint="Enregistré automatiquement et appliqué en direct dans votre logiciel.">Personnalisation</SectionTitle>
        {/* Un onglet par type d'alerte : icône, nom, et pastille verte si l'alerte est activée. */}
        <div className="mt-4 flex flex-wrap gap-2" role="tablist" aria-label="Type d'alerte">
          {ALERT_KINDS.map((k) => {
            const Icon = KIND_ICONS[k];
            const selected = k === kind;
            const on = config.kinds[k].enabled;
            return (
              <button
                key={k}
                type="button"
                role="tab"
                aria-selected={selected}
                onClick={() => {
                  setKind(k);
                  setReplay((n) => n + 1);
                }}
                className={`flex items-center gap-2 rounded-xl border px-3 py-2 text-xs font-medium transition ${
                  selected ? "border-signal-500/60 bg-signal-600/15 text-fg shadow-[0_0_0_1px_rgb(var(--signal-500)/0.25)]" : "border-base-700 bg-base-800/60 text-muted hover:border-base-700 hover:text-fg"
                }`}
              >
                <Icon size={14} className={selected ? "text-accent" : undefined} />
                {ALERT_KIND_LABELS[k]}
                <span className={`h-1.5 w-1.5 rounded-full ${on ? "bg-live" : "bg-base-700"}`} title={on ? "Activée" : "Désactivée"} />
              </button>
            );
          })}
        </div>

        <div className="mt-5 grid gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          <div className="space-y-4">
            <ToggleField label={`Afficher l'alerte « ${ALERT_KIND_LABELS[kind]} »`} checked={style.enabled} onChange={(enabled) => setStyle({ enabled })} />
            <TextField label="Titre" value={style.title} onChange={(title) => setStyle({ title })} hint={VARIABLES} />
            <TextField label="Texte" value={style.subtitle} onChange={(subtitle) => setStyle({ subtitle })} />

            <div>
              <span className="mb-1 block text-[11px] text-muted">Image (PNG, JPG, GIF animé, WebP)</span>
              <div className="flex items-center gap-2">
                {style.imageId && media[style.imageId] && <img src={media[style.imageId]} alt="" className="h-10 w-10 rounded-md bg-stage object-contain" />}
                <label className="btn-secondary inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-fg">
                  <ImagePlus size={14} /> {style.imageId ? "Changer" : "Choisir une image"}
                  <input type="file" accept="image/png,image/jpeg,image/gif,image/webp" className="hidden" onChange={(e) => void importFile(e, "image")} />
                </label>
                {style.imageId && (
                  <Button size="sm" variant="ghost" onClick={() => setStyle({ imageId: null })} title="Retirer l'image">
                    <Trash2 size={14} />
                  </Button>
                )}
              </div>
            </div>

            <div className="space-y-2">
              <SegmentedField
                label="Son"
                value={style.sound.type}
                onChange={(type) => {
                  if (type === "none" || type === "chime") setStyle({ sound: { type } });
                  else if (style.sound.type !== "file") push("Choisissez un fichier son ci-dessous.", "success");
                }}
                options={[
                  { value: "none", label: "Aucun" },
                  { value: "chime", label: "Carillon" },
                  { value: "file", label: "Fichier" },
                ]}
              />
              <div className="flex items-center gap-2">
                <label className="btn-secondary inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs text-fg">
                  <Music4 size={14} /> {style.sound.type === "file" ? "Changer de fichier" : "Choisir un son (MP3, OGG, WAV)"}
                  <input type="file" accept="audio/mpeg,audio/ogg,audio/wav,.mp3,.ogg,.wav" className="hidden" onChange={(e) => void importFile(e, "sound")} />
                </label>
                <Button size="sm" onClick={listen} disabled={style.sound.type === "none"}>
                  <Volume2 size={14} /> Écouter
                </Button>
              </div>
              <SliderField label="Volume" value={style.volume} min={0} max={1} step={0.05} format={(v) => `${Math.round(v * 100)} %`} onChange={(volume) => setStyle({ volume })} />
            </div>

            <SliderField label="Durée d'affichage" value={style.durationMs} min={1000} max={15000} step={500} format={seconds} onChange={(durationMs) => setStyle({ durationMs })} />
            <div className="grid grid-cols-2 gap-3">
              <SelectField<string> label="Apparition" value={style.enter} options={enterEffectOptions()} onChange={(enter) => setStyle({ enter })} />
              <SelectField<string> label="Disparition" value={style.exit} options={exitEffectOptions()} onChange={(exit) => setStyle({ exit })} />
            </div>
            <details className="rounded-md border border-base-700 px-3 py-2">
              <summary className="cursor-pointer text-[11px] text-muted hover:text-fg">Apparence</summary>
              <div className="mt-3 space-y-3">
                <SegmentedField label="Disposition" value={style.layout} onChange={(layout) => setStyle({ layout })} options={[{ value: "stack", label: "Image au-dessus" }, { value: "side", label: "Image à gauche" }]} />
                <ColorField label="Couleur du titre" value={style.accentColor} onChange={(accentColor) => setStyle({ accentColor: accentColor.toUpperCase() })} />
                <ColorField label="Couleur du texte" value={style.textColor} onChange={(textColor) => setStyle({ textColor: textColor.toUpperCase() })} />
                <SliderField label="Taille du texte" value={style.fontSize} min={16} max={96} format={(v) => `${v} px`} onChange={(fontSize) => setStyle({ fontSize })} />
                <SliderField label="Durée des animations" value={style.animMs} min={100} max={1500} step={50} format={(v) => `${v} ms`} onChange={(animMs) => setStyle({ animMs })} />
                <ToggleField label="Carte sombre derrière le texte" checked={style.card} onChange={(card) => setStyle({ card })} />
              </div>
            </details>
          </div>

          <div>
            <div className="relative flex aspect-video items-center justify-center overflow-hidden rounded-xl bg-stage" aria-label="Aperçu de l'alerte">
              <div style={{ transform: "scale(0.55)" }}>
                <AlertPlayer
                  key={sample.id + JSON.stringify(style)}
                  event={sample}
                  style={style}
                  muted={!withSound}
                  mediaUrl={(id) => media[id] ?? null}
                  onDone={() => {
                    setWithSound(false);
                    setTimeout(() => setReplay((n) => n + 1), 700);
                  }}
                />
              </div>
            </div>
            <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
              <Button
                size="sm"
                onClick={() => {
                  setWithSound(true);
                  setReplay((n) => n + 1);
                }}
              >
                <Play size={14} /> Rejouer avec le son
              </Button>
              <Button size="sm" variant="primary" onClick={() => void test(kind)}>
                <Bell size={14} /> Tester dans le logiciel
              </Button>
            </div>
            <p className="mt-2 text-[11px] text-faint">
              Aperçu réduit, sur fond de scène. Les alertes arrivent une par une ; « Passer » arrête celle en cours et vide la file.
            </p>
          </div>
        </div>
      </section>
    </div>
  );
}
