import { useEffect, useState, type ReactNode } from "react";
import { useNavigate } from "react-router-dom";
import { Activity, AlertTriangle, ArrowRight, Bell, Check, Copy, ExternalLink, Gift, Heart, MessageSquare, Music2, Radio, Star, type LucideIcon } from "lucide-react";
import { useMusicStore } from "@/stores/useMusicStore";
import { useOverlayStore } from "@/stores/useOverlayStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { useToastStore } from "@/stores/useToastStore";
import { useCopyObsUrl } from "@/hooks/useCopyObsUrl";
import { Button, EmptyState, StatusPill } from "@/components/ui";
import { BatGlyph } from "@/components/BrandMark";
import { StatTile } from "@/components/StatTile";
import { pickMainOverlay } from "@/utils/ui-helpers";
import { channelLabel, chatStatus, isOtherChannel } from "@/utils/twitch-ui";
import { useLiveData, useTikTokState } from "@/hooks/useLive";
import { ALERT_OUTCOME_LABELS, alertTitle, formatCount, type ActivityEntry } from "../../shared/live";
import { normalizeTikTokMode, TIKTOK_MODES } from "../../shared/tiktok";

const SOURCE_LABELS: Record<string, string> = {
  spotify: "Spotify",
  demo: "Mode démo",
  "system-media": "Lecture système",
};

type Tone = "ok" | "warn" | "bad" | "idle";
const DOT: Record<Tone, string> = { ok: "dot-live", warn: "bg-amber-400", bad: "bg-red-500", idle: "bg-base-700" };

interface Check {
  label: string;
  value: string;
  tone: Tone;
}
interface Issue {
  text: string;
  to: string;
  cta: string;
}

/**
 * Vue d'ensemble : l'état des deux overlays en un coup d'œil. Chacun a sa carte (ce qu'il affiche en ce moment,
 * ses points de contrôle, son action principale) ; aucun ne dépend de l'autre. Ce qui reste à régler est
 * regroupé en une liste d'actions, ou remplacé par une confirmation quand tout est prêt.
 */
export function Home() {
  const navigate = useNavigate();
  const { activeProviderId, track } = useMusicStore((s) => ({ activeProviderId: s.activeProviderId, track: s.playbackState.track }));
  const { overlays, activeOverlayId } = useOverlayStore((s) => ({ overlays: s.overlays, activeOverlayId: s.activeOverlayId }));
  const { settings, musicServerUp } = useSettingsStore((s) => ({ settings: s.settings, musicServerUp: s.overlayServerRunning }));
  const twitch = useTwitchStore((s) => s.state);
  const push = useToastStore((s) => s.push);
  const copyMusicUrl = useCopyObsUrl();
  const tiktok = useTikTokState();
  const { activity, counters } = useLiveData();

  const mainOverlay = pickMainOverlay(overlays, activeOverlayId);
  const chat = chatStatus(twitch);
  const musicReady = musicServerUp && activeProviderId !== null && mainOverlay !== null;
  const chatReady = twitch.server.running && twitch.status === "connected" && twitch.chat === "live";
  const twitchConnected = twitch.status === "connected";

  async function copyChatUrl() {
    try {
      await navigator.clipboard.writeText(twitch.server.overlayUrl);
      push("URL du chat copiée — collez-la dans une source « Navigateur » d'OBS.", "success");
    } catch {
      push("Copie impossible : sélectionnez l'URL dans la page Chat Twitch.", "error");
    }
  }

  const musicChecks: Check[] = [
    { label: "Serveur", value: musicServerUp ? `En ligne, port ${settings?.overlayServerPort ?? "…"}` : "Hors ligne", tone: musicServerUp ? "ok" : "bad" },
    { label: "Source", value: activeProviderId ? (SOURCE_LABELS[activeProviderId] ?? activeProviderId) : "Aucune", tone: activeProviderId ? "ok" : "warn" },
    { label: "Overlay", value: mainOverlay ? mainOverlay.name : "Aucun", tone: mainOverlay ? "ok" : "warn" },
  ];
  const chatChecks: Check[] = [
    { label: "Serveur", value: twitch.server.running ? `En ligne, port ${twitch.server.port}` : "Hors ligne", tone: twitch.server.running ? "ok" : "bad" },
    { label: "Compte", value: twitch.account ? twitch.account.displayName : "Non connecté", tone: twitchConnected ? "ok" : "warn" },
    { label: "Sources OBS", value: String(twitch.overlayClients), tone: twitch.overlayClients > 0 ? "ok" : "idle" },
  ];

  // Adresse locale : un autre outil gère TikTok, Batlay ne suit aucun live lui-même.
  const tiktokLocal = normalizeTikTokMode(tiktok.settings.mode) === "local";
  const tiktokOn = !tiktokLocal && tiktok.settings.enabled && !!tiktok.settings.username;
  const tiktokLive = tiktok.status === "live";
  const tiktokChecks: Check[] = [
    { label: "Méthode", value: TIKTOK_MODES.find((m) => m.value === normalizeTikTokMode(tiktok.settings.mode))?.label ?? "Navigateur intégré", tone: tiktokLocal ? "idle" : "ok" },
    { label: "Compte", value: tiktok.account ? tiktok.account.displayName : tiktok.settings.username ? `@${tiktok.settings.username}` : "Aucun", tone: tiktok.account ? "ok" : tiktokOn ? "warn" : "idle" },
    {
      label: "Live",
      value: tiktokLocal ? "Géré par votre outil" : tiktokLive ? "En cours" : tiktok.status === "hidden" ? "Caché aux visiteurs" : tiktok.status === "error" ? "Erreur" : tiktokOn ? "En attente" : "Non suivi",
      tone: tiktokLive ? "ok" : tiktok.status === "error" || tiktok.status === "hidden" ? "bad" : tiktokOn ? "warn" : "idle",
    },
    { label: "Session TikTok", value: tiktok.loggedIn ? "Connecté" : "Non connecté", tone: tiktok.loggedIn ? "ok" : "idle" },
  ];

  const issues: Issue[] = [];
  if (!musicServerUp) issues.push({ text: "Le serveur de l'overlay musical est hors ligne (port déjà utilisé ?).", to: "/settings", cta: "Changer le port" });
  if (!activeProviderId) issues.push({ text: "Aucune source musicale n'est choisie.", to: "/connections", cta: "Choisir une source" });
  if (!mainOverlay) issues.push({ text: "Aucun overlay musical n'existe encore.", to: "/overlays", cta: "Créer un overlay" });
  if (!twitch.server.running) issues.push({ text: "Le serveur du chat est hors ligne.", to: "/chat", cta: "Voir le détail" });
  else if (!twitchConnected) issues.push({ text: "Twitch n'est pas connecté : le chat ne peut pas s'afficher.", to: "/chat", cta: "Connecter Twitch" });
  else if (twitch.error) issues.push({ text: twitch.error.message, to: "/chat", cta: "Corriger" });
  if (tiktok.status === "hidden") issues.push({ text: "Votre live TikTok est caché aux visiteurs (compte privé ou live restreint).", to: "/alerts", cta: "Voir la solution" });
  if (tiktok.status === "error") issues.push({ text: tiktok.message ?? "TikTok : erreur de connexion.", to: "/alerts", cta: "Voir le détail" });

  const allReady = musicReady && chatReady;

  const recent = activity.filter((e): e is Extract<ActivityEntry, { type: "alert" }> => e.type === "alert").slice(-6).reverse();
  const total = (k: "follows" | "subs" | "messages") => counters.twitch[k] + counters.tiktok[k];
  const gifts = counters.twitch.giftedSubs + counters.tiktok.gifts;
  const hour = new Date().getHours();
  const hello = hour < 5 || hour >= 18 ? "Bonsoir" : "Bonjour";
  const name = twitch.account?.displayName ?? tiktok.account?.displayName ?? null;

  const readyCount = [musicReady, chatReady, tiktokLive || tiktokLocal].filter(Boolean).length;

  return (
    <div className="stagger mx-auto max-w-6xl px-8 pb-10 pt-8">
      {/* --- Bandeau d'accueil : salutation, état du live, actions --- */}
      <section className="card relative overflow-hidden p-0" aria-label="Vue d'ensemble">
        <div
          className="pointer-events-none absolute inset-0"
          aria-hidden="true"
          style={{ background: "radial-gradient(70% 140% at 0% 0%, rgb(var(--signal-600) / 0.22), transparent 60%), radial-gradient(50% 120% at 100% 0%, rgb(var(--nova) / 0.12), transparent 65%)" }}
        />
        <BatGlyph className="pointer-events-none absolute -right-8 top-1/2 w-[300px] -translate-y-1/2 text-fg opacity-[0.035]" />
        <div className="relative flex flex-wrap items-end justify-between gap-6 px-7 pb-6 pt-7">
          <div className="min-w-0">
            <p className="text-[11px] font-semibold uppercase tracking-[0.12em] text-accent">Vue d'ensemble</p>
            <h1 className="mt-1.5 font-display text-[30px] font-semibold leading-tight tracking-[-0.02em] text-fg">
              {hello}
              {name ? ` ${name}` : ""}
            </h1>
            <p className="mt-1.5 max-w-xl text-sm text-muted">Musique, chat Twitch et TikTok : tout ce qui part vers votre live, en un coup d'œil.</p>
            <div className="mt-4 flex flex-wrap items-center gap-2">
              <ReadyChip label="Musique" ready={musicReady} />
              <ReadyChip label="Chat Twitch" ready={chatReady} />
              <ReadyChip label={tiktokLocal ? "TikTok (outil externe)" : tiktokLive ? "TikTok en live" : "TikTok"} ready={tiktokLive || tiktokLocal} live={tiktokLive} />
            </div>
            {/* Jauge : combien des trois sources sont prêtes pour le live. */}
            <div className="mt-3.5 flex items-center gap-3" aria-label={`${readyCount} source${readyCount > 1 ? "s" : ""} prête${readyCount > 1 ? "s" : ""} sur 3`}>
              <div className="flex w-44 gap-1" aria-hidden="true">
                {[0, 1, 2].map((i) => (
                  <span key={i} className={`h-1.5 flex-1 rounded-full transition-colors duration-500 ${i < readyCount ? "bg-gradient-to-r from-signal-500 to-nova shadow-[0_0_10px_rgb(var(--signal-500)/0.6)]" : "bg-base-700"}`} />
                ))}
              </div>
              <span className="text-xs text-muted">{readyCount === 3 ? "Prêt pour le live" : `${readyCount} sur 3 prêt${readyCount > 1 ? "s" : ""}`}</span>
            </div>
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-2">
            <Button onClick={() => navigate("/alerts")} title="Alertes : tester une alerte, régler Twitch et TikTok">
              <Bell size={14} /> Alertes
            </Button>
            <Button variant="primary" onClick={() => void window.batlay.live.openWindow()} title="Chats, compteurs et journal dans une fenêtre séparée">
              <ExternalLink size={14} /> Fenêtre Live
            </Button>
          </div>
        </div>
      </section>

      {/* --- Le live en chiffres --- */}
      <section className="mt-4" aria-label="Le live en chiffres">
        <div className="mb-2.5 flex flex-wrap items-center justify-between gap-3 px-1">
          <h2 className="flex items-center gap-2 font-display text-sm font-semibold text-fg">
            <Activity size={15} className="text-accent" /> Le live en chiffres
            {tiktokLive && <span className="live-badge tag-live rounded-full px-1.5 py-px text-[9px] font-bold uppercase tracking-wider">TikTok en live</span>}
            {tiktokLive && (
              <span className="text-xs font-normal text-muted" title="Spectateurs et likes du live TikTok">
                {formatCount(counters.tiktok.viewers)} spectateurs · {formatCount(counters.tiktok.likes)} likes
              </span>
            )}
          </h2>
          <Button size="sm" variant="ghost" onClick={() => navigate("/live")}>
            Chats et journal <ArrowRight size={13} />
          </Button>
        </div>
        <div className="grid grid-cols-2 gap-3 xl:grid-cols-4">
          <Counter icon={Heart} tint="157 107 255" label="Nouveaux followers" value={total("follows")} />
          <Counter icon={Star} tint="62 224 160" label="Abonnés" value={total("subs")} />
          <Counter icon={Gift} tint="254 44 85" label="Cadeaux" value={gifts} />
          <Counter icon={MessageSquare} tint="38 198 243" label="Messages" value={total("messages")} />
        </div>
      </section>

      <div className="mt-4 grid gap-4 md:grid-cols-2 xl:grid-cols-3 md:[&>*:last-child]:col-span-2 xl:[&>*:last-child]:col-span-1">
        {/* --- Musique --- */}
        <OverlayCard
          icon={<Music2 size={18} />}
          title="Musique"
          status={<StatusPill tone={musicReady ? "ok" : "warn"}>{musicReady ? "Prêt" : "À configurer"}</StatusPill>}
          backdrop={track?.artwork ?? null}
          hero={
            <div className="flex items-center gap-4">
              {track?.artwork ? (
                <img src={track.artwork} alt="" className="h-16 w-16 shrink-0 rounded-xl object-cover shadow-[0_12px_28px_-10px_rgba(0,0,0,0.7)] ring-1 ring-white/10" />
              ) : (
                <div className="flex h-16 w-16 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-base-700 to-base-800 ring-1 ring-base-700">
                  <Music2 size={22} className="text-faint" />
                </div>
              )}
              <div className="min-w-0 flex-1">
                {track ? (
                  <>
                    <p className="truncate font-display text-lg font-semibold leading-tight text-fg" title={track.title}>
                      {track.title}
                    </p>
                    <p className="truncate text-sm text-muted" title={track.artist}>
                      {track.artist}
                    </p>
                    <p className="mt-1 flex items-center gap-1.5 text-xs text-faint">
                      <svg viewBox="0 0 16 16" className={`h-3 w-3 ${track.isPlaying ? "text-accent" : "eq-paused"}`} fill="currentColor" aria-hidden="true">
                        <rect className="eq-bar" x="2" y="7" width="3" height="7" rx="1.5" />
                        <rect className="eq-bar" x="6.5" y="2" width="3" height="12" rx="1.5" />
                        <rect className="eq-bar" x="11" y="5" width="3" height="9" rx="1.5" />
                      </svg>
                      {track.isPlaying ? "En lecture" : "En pause"}
                    </p>
                  </>
                ) : (
                  <>
                    <p className="font-medium text-fg">Aucune musique en cours</p>
                    <p className="text-xs text-muted">Lancez un morceau dans votre lecteur : il apparaîtra ici.</p>
                  </>
                )}
              </div>
            </div>
          }
          checks={musicChecks}
          actions={
            <>
              <Button size="sm" disabled={!mainOverlay || !musicServerUp} onClick={() => mainOverlay && void copyMusicUrl(mainOverlay.id)}>
                <Copy size={14} /> Copier l'URL OBS
              </Button>
              <Button size="sm" variant="primary" onClick={() => navigate("/musique")}>
                Lecture en cours <ArrowRight size={13} />
              </Button>
            </>
          }
        />

        {/* --- Chat --- */}
        <OverlayCard
          icon={<MessageSquare size={18} />}
          title="Chat Twitch"
          status={<StatusPill tone={chatReady || chat.tone === "ok" ? "ok" : "warn"}>{chatReady ? "En direct" : chat.label}</StatusPill>}
          backdrop={null}
          hero={
            <div className="flex items-center gap-4">
              <ChannelAvatar name={channelLabel(twitch)} url={twitch.channel?.profileImageUrl ?? twitch.account?.profileImageUrl ?? null} />
              <div className="min-w-0 flex-1">
                {twitchConnected || twitch.channelLogin ? (
                  <>
                    <p className="truncate font-display text-lg font-semibold leading-tight text-fg" title={channelLabel(twitch)}>
                      {channelLabel(twitch)}
                    </p>
                    <p className="truncate text-sm text-muted">
                      {twitch.channel ? `@${twitch.channel.login}` : twitch.channelLogin ? `@${twitch.channelLogin}` : ""}
                      {isOtherChannel(twitch) ? " · autre chaîne" : ""}
                    </p>
                    <p className="mt-1 flex items-center gap-1.5 text-xs text-faint">
                      <span className={`h-2 w-2 rounded-full ${chatReady ? "dot-live" : "bg-amber-400"}`} aria-hidden="true" />
                      {chat.label}
                    </p>
                  </>
                ) : (
                  <>
                    <p className="font-medium text-fg">Aucune chaîne affichée</p>
                    <p className="text-xs text-muted">Connectez Twitch pour afficher le chat de votre chaîne, ou d'une autre.</p>
                  </>
                )}
              </div>
            </div>
          }
          checks={chatChecks}
          actions={
            <>
              <Button size="sm" disabled={!twitch.server.running} onClick={() => void copyChatUrl()}>
                <Copy size={14} /> Copier l'URL OBS
              </Button>
              <Button size="sm" variant="primary" onClick={() => navigate("/chat")}>
                Chat Twitch <ArrowRight size={13} />
              </Button>
            </>
          }
        />

        {/* --- TikTok --- */}
        <OverlayCard
          icon={<Radio size={18} />}
          title="TikTok"
          status={
            tiktokLive ? (
              <span className="live-badge tag-live rounded-full px-2 py-0.5 text-[11px] font-semibold">● En live</span>
            ) : (
              <StatusPill tone={tiktokOn ? "warn" : "neutral"}>{tiktokLocal ? "Adresse locale" : tiktokOn ? "En attente du live" : "Non branché"}</StatusPill>
            )
          }
          backdrop={null}
          hero={
            <div className="flex items-center gap-4">
              <span className="flex h-16 w-16 shrink-0 items-center justify-center tiktok-avatar rounded-full font-display text-2xl font-bold" aria-hidden="true">
                {(tiktok.account?.displayName ?? tiktok.settings.username ?? "T").slice(0, 1).toUpperCase()}
              </span>
              <div className="min-w-0 flex-1">
                {tiktok.settings.username ? (
                  <>
                    <p className="truncate font-display text-lg font-semibold leading-tight text-fg">{tiktok.account?.displayName ?? `@${tiktok.settings.username}`}</p>
                    {tiktok.account && <p className="truncate text-sm text-muted">@{tiktok.settings.username}</p>}
                    <p className="mt-1 line-clamp-2 text-xs text-faint">{tiktok.message ?? "Follows, Super Fans et cadeaux en alertes, et le chat TikTok."}</p>
                  </>
                ) : (
                  <>
                    <p className="font-medium text-fg">Aucun compte TikTok</p>
                    <p className="text-xs text-muted">Entrez votre pseudo : Batlay se branche tout seul sur votre live.</p>
                  </>
                )}
              </div>
            </div>
          }
          checks={tiktokChecks}
          actions={
            <Button size="sm" variant="primary" onClick={() => navigate("/alerts")}>
              {tiktok.settings.username ? "Réglages TikTok" : "Brancher TikTok"} <ArrowRight size={13} />
            </Button>
          }
        />
      </div>

      <div className="mt-4 grid gap-4 lg:grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)]">
        {/* --- Dernières alertes --- */}
        <section className="card p-5" aria-label="Dernières alertes">
          <div className="flex items-center justify-between gap-3">
            <h2 className="font-display text-base font-semibold text-fg">Dernières alertes</h2>
            <Button size="sm" variant="ghost" onClick={() => navigate("/live")}>
              Tout le journal <ArrowRight size={13} />
            </Button>
          </div>
          {recent.length === 0 ? (
            <div className="surface-inset mt-3">
              <EmptyState
                compact
                icon={Bell}
                title="Pas encore d'alerte depuis l'ouverture de Batlay"
                action={
                  <Button size="sm" onClick={() => navigate("/alerts")}>
                    Tester une alerte
                  </Button>
                }
              >
                Les follows, abonnements et cadeaux de Twitch et TikTok s'afficheront ici.
              </EmptyState>
            </div>
          ) : (
            <ul className="mt-3 space-y-1.5">
              {recent.map((e) => (
                <li key={e.id} className="row-in flex items-center gap-3 rounded-lg px-2 py-1.5 text-sm hover:bg-base-800/50">
                  <span className={`h-2 w-2 shrink-0 rounded-full ${e.source === "tiktok" ? "bg-[#FE2C55]" : e.source === "twitch" ? "bg-[#9146FF]" : "bg-base-700"}`} aria-hidden="true" />
                  <span className="min-w-0 flex-1 truncate">
                    <span className="font-medium text-fg">{e.user}</span> <span className="text-muted">· {alertTitle(e.kind)} · {e.detail}</span>
                  </span>
                  <span className={`shrink-0 text-[11px] ${e.outcome === "shown" ? "text-ok" : "text-warn-strong"}`} title={ALERT_OUTCOME_LABELS[e.outcome]}>
                    {e.outcome === "shown" ? "Affichée" : "Non affichée"}
                  </span>
                  <span className="w-11 shrink-0 text-right text-[11px] tabular-nums text-faint">
                    {new Date(e.at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit" })}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>

      {/* --- Ce qui reste à faire, ou confirmation --- */}
      {issues.length > 0 ? (
        <section className="card p-5" aria-label="À régler">
          <h2 className="flex items-center gap-2 font-display text-base font-semibold text-fg">
            <AlertTriangle size={16} className="text-warn" aria-hidden="true" />
            {issues.length === 1 ? "Une chose à régler" : `${issues.length} choses à régler`}
          </h2>
          <ul className="mt-3 divide-y divide-line">
            {issues.map((issue) => (
              <li key={issue.text} className="flex items-center justify-between gap-4 py-2.5 first:pt-0 last:pb-0">
                <p className="min-w-0 text-sm text-muted">{issue.text}</p>
                <Button size="sm" className="shrink-0" onClick={() => navigate(issue.to)}>
                  {issue.cta}
                </Button>
              </li>
            ))}
          </ul>
        </section>
      ) : (
        <div
          className={`flex items-start gap-3 rounded-2xl border px-4 py-4 text-sm ${
            allReady ? "border-live/30 bg-gradient-to-r from-live/15 via-live/5 to-transparent text-fg" : "border-line text-muted"
          }`}
        >
          {allReady && (
            <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-live/20 text-ok ring-1 ring-live/40">
              <Check size={14} />
            </span>
          )}
          <p>
            {allReady ? "Tout est prêt. " : ""}
            Dans votre logiciel de streaming, ajoutez une source Web par overlay : la musique, le chat et la page des alertes.
            Chacun fonctionne indépendamment des autres.
          </p>
        </div>
      )}
      </div>
    </div>
  );
}

function OverlayCard({
  icon,
  title,
  status,
  backdrop,
  hero,
  checks,
  actions,
}: {
  icon: ReactNode;
  title: string;
  status: ReactNode;
  /** Image floutée qui teinte la carte (pochette du morceau en cours). */
  backdrop: string | null;
  hero: ReactNode;
  checks: Check[];
  actions: ReactNode;
}) {
  return (
    <section className="card flex flex-col overflow-hidden" aria-label={title}>
      {backdrop && (
        <img
          src={backdrop}
          alt=""
          aria-hidden="true"
          className="pointer-events-none absolute -top-8 left-0 h-48 w-full scale-125 object-cover opacity-25 blur-3xl saturate-150"
        />
      )}
      <div className="relative flex flex-1 flex-col p-5">
        <div className="flex items-center justify-between gap-3">
          <h2 className="flex items-center gap-2.5 font-display text-base font-semibold text-fg">
            <span className="flex h-8 w-8 items-center justify-center rounded-[10px] bg-gradient-to-br from-signal-600/25 to-nova/10 text-accent ring-1 ring-inset ring-signal-500/25">{icon}</span>
            {title}
          </h2>
          {status}
        </div>

        <div className="mt-5">{hero}</div>

        <dl className="surface-inset mt-5 space-y-2 px-3.5 py-3 text-[13px]">
          {checks.map(({ label, value, tone }) => (
            <div key={label} className="flex items-center justify-between gap-4">
              <dt className="shrink-0 text-muted">{label}</dt>
              <dd className="flex min-w-0 items-center gap-2 text-fg" title={value}>
                <span className={`h-[7px] w-[7px] shrink-0 rounded-full ${DOT[tone]}`} aria-hidden="true" />
                <span className="truncate">{value}</span>
              </dd>
            </div>
          ))}
        </dl>

        <div className="mt-auto flex flex-wrap justify-end gap-2 pt-4">{actions}</div>
      </div>
    </section>
  );
}

/** Photo de profil de la chaîne, ou son initiale tant qu'elle manque ou ne charge pas. */
function ChannelAvatar({ name, url }: { name: string; url: string | null }) {
  const [failed, setFailed] = useState(false);
  useEffect(() => setFailed(false), [url]);
  if (url && !failed) {
    return <img src={url} alt="" onError={() => setFailed(true)} className="h-16 w-16 shrink-0 rounded-full object-cover ring-1 ring-white/10" />;
  }
  return (
    <span className="btn-primary flex h-16 w-16 shrink-0 items-center justify-center rounded-full font-display text-2xl font-bold" aria-hidden="true">
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** Tuile de statistique de la Vue d'ensemble. */
function Counter(props: { icon: LucideIcon; tint: string; label: string; value: number }) {
  return <StatTile {...props} />;
}

/** Pastille « prêt / à régler » du bandeau d'accueil. */
function ReadyChip({ label, ready, live = false }: { label: string; ready: boolean; live?: boolean }) {
  return (
    <span
      className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-xs font-medium ring-1 ring-inset ${
        ready ? "bg-live/10 text-fg ring-live/25" : "bg-base-800/70 text-muted ring-base-700"
      }`}
    >
      <span className={`h-[7px] w-[7px] rounded-full ${live ? "bg-[#FE2C55]" : ready ? "dot-live" : "bg-amber-400"}`} aria-hidden="true" />
      {label}
    </span>
  );
}
