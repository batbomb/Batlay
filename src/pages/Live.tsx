import { Activity as PageIcon } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useLocation } from "react-router-dom";
import {
  AlertTriangle,
  ArrowDown,
  CheckCircle2,
  ExternalLink,
  Eye,
  Gift,
  Heart,
  Info,
  MessageSquare,
  Pin,
  PinOff,
  RotateCcw,
  ScrollText,
  Star,
  ThumbsUp,
  XCircle,
  type LucideIcon,
} from "lucide-react";
import { Button, EmptyState, PageHeader, StatusPill } from "@/components/ui";
import { SegmentedField } from "@/components/editor/fields";
import { StatTile } from "@/components/StatTile";
import { useLiveData, useTikTokState } from "@/hooks/useLive";
import { useToastStore } from "@/stores/useToastStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { emoteUrl, readableNameColor } from "../../twitch-overlay/chat-feed";
import {
  ALERT_OUTCOME_LABELS,
  LIVE_PLATFORM_LABELS,
  alertTitle,
  filterActivity,
  formatCount,
  type ActivityEntry,
  type ActivityFilter,
  type LiveChatMessage,
  type LivePlatform,
} from "../../shared/live";
import type { AlertKind } from "../../shared/alerts";

/**
 * Fenêtre « Live » : tout le live au même endroit. Chats Twitch et TikTok
 * (fusionnés, côte à côte, ou un seul), journal d'activité lisible (chaque
 * alerte et ce qui lui est arrivé), compteurs du live. Ouvrable dans une
 * fenêtre séparée (#/live?window=1), à garder au premier plan pendant le live.
 */

type ChatView = "merged" | "split" | "twitch" | "tiktok";
const VIEW_KEY = "batlay.live.view";

const PLATFORM_DOT: Record<LivePlatform, string> = {
  twitch: "bg-[#9146FF]",
  tiktok: "bg-[#FE2C55]",
};

const KIND_ICONS: Record<AlertKind, LucideIcon> = { follow: Heart, subscribe: Star, resub: Star, gift: Gift, tiktokGift: Gift };
const KIND_COLORS: Record<AlertKind, string> = {
  follow: "text-[#9B6BFF]",
  subscribe: "text-ok",
  resub: "text-ok",
  gift: "text-amber-400",
  tiktokGift: "text-[#FE2C55]",
};
const LEVEL_ICONS = { info: Info, success: CheckCircle2, warn: AlertTriangle, error: XCircle } as const;
const LEVEL_COLORS = { info: "text-muted", success: "text-ok", warn: "text-warn-strong", error: "text-danger" } as const;

const time = (at: number, seconds = false) =>
  new Date(at).toLocaleTimeString("fr-FR", { hour: "2-digit", minute: "2-digit", ...(seconds ? { second: "2-digit" } : {}) });

function readView(): ChatView {
  try {
    const v = localStorage.getItem(VIEW_KEY);
    return v === "split" || v === "twitch" || v === "tiktok" ? v : "merged";
  } catch {
    return "merged";
  }
}

export function Live() {
  const { search } = useLocation();
  const detached = new URLSearchParams(search).get("window") === "1";
  const push = useToastStore((s) => s.push);
  const twitch = useTwitchStore((s) => s.state);
  const tiktok = useTikTokState();
  const { activity, chat, counters } = useLiveData();
  const [view, setView] = useState<ChatView>(readView);
  const [onTop, setOnTop] = useState(false);
  const [filter, setFilter] = useState<ActivityFilter>("all");
  const [platform, setPlatform] = useState<LivePlatform | "all">("all");

  useEffect(() => {
    try {
      localStorage.setItem(VIEW_KEY, view);
    } catch {
      // Stockage indisponible : le choix vaut pour cette session seulement.
    }
  }, [view]);

  const twitchStatus = twitch.status !== "connected" ? { tone: "neutral" as const, label: "Twitch non connecté" } : twitch.chat === "live" ? { tone: "ok" as const, label: "Chat Twitch en direct" } : { tone: "warn" as const, label: "Chat Twitch : connexion…" };
  const tiktokStatus =
    tiktok.status === "live" ? { tone: "ok" as const, label: "TikTok en live" } : tiktok.status === "idle" ? { tone: "neutral" as const, label: "TikTok non connecté" } : tiktok.status === "hidden" ? { tone: "warn" as const, label: "TikTok : live caché" } : tiktok.status === "error" ? { tone: "warn" as const, label: "TikTok : erreur" } : { tone: "neutral" as const, label: "TikTok : en attente du live" };

  const entries = useMemo(() => filterActivity(activity, filter, platform).slice().reverse(), [activity, filter, platform]);
  const tw = counters.twitch;
  const tt = counters.tiktok;

  return (
    <div className={`flex h-full flex-col ${detached ? "px-4 py-4" : "mx-auto max-w-7xl px-8 py-10"}`}>
      {detached ? (
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <h1 className="font-display text-lg font-bold text-fg">Live</h1>
            <StatusPill tone={twitchStatus.tone}>{twitchStatus.label}</StatusPill>
            <StatusPill tone={tiktokStatus.tone}>{tiktokStatus.label}</StatusPill>
          </div>
          <div className="flex items-center gap-2">
            <Button size="sm" variant={onTop ? "primary" : "secondary"} onClick={() => void window.batlay.live.setAlwaysOnTop(!onTop).then(setOnTop)} title="Garder cette fenêtre au-dessus des autres (jeu, OBS…)">
              {onTop ? <PinOff size={14} /> : <Pin size={14} />} {onTop ? "Ne plus épingler" : "Toujours visible"}
            </Button>
            <ResetButton onReset={() => push("Compteurs et journal remis à zéro.", "success")} />
          </div>
        </div>
      ) : (
        <PageHeader icon={PageIcon} eyebrow="Diffusion"
          title="Live"
          subtitle="Vos chats Twitch et TikTok, les follows, abonnements et cadeaux, et ce qui s'est passé pour chaque alerte."
          actions={
            <>
              <ResetButton onReset={() => push("Compteurs et journal remis à zéro.", "success")} />
              <Button variant="primary" onClick={() => void window.batlay.live.openWindow()} title="Fenêtre séparée, à placer sur un second écran pendant le live">
                <ExternalLink size={14} /> Ouvrir dans une fenêtre
              </Button>
            </>
          }
        />
      )}

      {!detached && (
        <div className="mt-4 flex flex-wrap gap-2">
          <StatusPill tone={twitchStatus.tone}>{twitchStatus.label}</StatusPill>
          <StatusPill tone={tiktokStatus.tone}>{tiktokStatus.label}</StatusPill>
        </div>
      )}

      {/* --- Compteurs --- */}
      <div className={`grid grid-cols-2 gap-3 ${detached ? "mt-3" : "mt-5"} xl:grid-cols-4`}>
        <Stat icon={Heart} tint="157 107 255" label="Nouveaux followers" twitch={tw.follows} tiktok={tt.follows} />
        <Stat icon={Star} tint="62 224 160" label="Abonnés" twitch={tw.subs} tiktok={tt.subs} hint="Twitch : abonnements et réabonnements · TikTok : Super Fans" />
        <Stat icon={Gift} tint="254 44 85" label="Cadeaux" twitch={tw.giftedSubs} tiktok={tt.gifts} hint="Twitch : abonnements offerts · TikTok : cadeaux (quantité)" />
        <Stat icon={MessageSquare} tint="38 198 243" label="Messages" twitch={tw.messages} tiktok={tt.messages} />
      </div>
      {(tiktok.status === "live" || tt.likes > 0) && (
        <div className="mt-2 flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg border border-line px-3 py-2 text-xs text-muted" aria-label="Audience TikTok">
          <span className="font-medium text-fg">TikTok en direct</span>
          <span className="flex items-center gap-1.5" title="Spectateurs sur votre live TikTok en ce moment">
            <Eye size={13} className="text-accent" /> <b className="tabular-nums text-fg">{formatCount(tt.viewers)}</b> spectateurs
          </span>
          <span className="flex items-center gap-1.5" title="Likes du live TikTok (total donné par TikTok)">
            <ThumbsUp size={13} className="text-[#FE2C55]" /> <b className="tabular-nums text-fg">{formatCount(tt.likes)}</b> likes
          </span>
        </div>
      )}
      <p className="mt-1.5 text-[11px] text-faint">Depuis {time(counters.since)} (ouverture de Batlay ou dernière remise à zéro). Les tests ne sont pas comptés.</p>

      {/* --- Chats + journal --- */}
      <div className={`mt-4 grid min-h-0 flex-1 gap-4 ${view === "split" ? "lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]" : "lg:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]"}`}>
        <section className="card flex min-h-[320px] flex-col p-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="font-display text-sm font-semibold text-fg">Chat</h2>
            <div className="w-full max-w-sm">
              <SegmentedField<ChatView>
                value={view}
                onChange={setView}
                options={[
                  { value: "merged", label: "Fusionné", title: "Les deux chats dans une seule liste, par ordre d'arrivée" },
                  { value: "split", label: "Côte à côte" },
                  { value: "twitch", label: "Twitch" },
                  { value: "tiktok", label: "TikTok" },
                ]}
              />
            </div>
          </div>
          {view === "split" ? (
            <div className="mt-3 grid min-h-0 flex-1 gap-3 md:grid-cols-2">
              <ChatList messages={chat} only="twitch" title="Twitch" />
              <ChatList messages={chat} only="tiktok" title="TikTok" />
            </div>
          ) : (
            <div className="mt-3 flex min-h-0 flex-1">
              <ChatList messages={chat} only={view === "merged" ? null : view} />
            </div>
          )}
        </section>

        <section className="card flex min-h-[320px] flex-col p-4">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <h2 className="font-display text-sm font-semibold text-fg">Journal</h2>
            <select
              value={platform}
              onChange={(e) => setPlatform(e.target.value as LivePlatform | "all")}
              aria-label="Plateforme"
              className="rounded-md border border-base-700 bg-base-800 px-2 py-1 text-[11px] text-fg outline-none"
            >
              <option value="all">Toutes les plateformes</option>
              <option value="twitch">Twitch</option>
              <option value="tiktok">TikTok</option>
            </select>
          </div>
          <div className="mt-2">
            <SegmentedField<ActivityFilter>
              value={filter}
              onChange={setFilter}
              options={[
                { value: "all", label: "Tout" },
                { value: "alerts", label: "Alertes" },
                { value: "system", label: "Connexions" },
              ]}
            />
          </div>
          <ol className="mt-3 min-h-0 flex-1 space-y-1.5 overflow-y-auto pr-1" aria-label="Journal d'activité">
            {entries.length === 0 ? (
              <li className="surface-inset">
                <EmptyState compact icon={ScrollText} title="Rien pour l'instant">
                  Les follows, abonnements, cadeaux et l'état des connexions s'afficheront ici, du plus récent au plus ancien.
                </EmptyState>
              </li>
            ) : (
              entries.map((e) => <ActivityRow key={e.id} entry={e} onReplay={(id) => void window.batlay.live.replay(id).then((r) => push(r.shown ? "Alerte rejouée." : "Aucune page d'alertes ouverte dans votre logiciel de streaming.", r.shown ? "success" : "error"))} />)
            )}
          </ol>
        </section>
      </div>
    </div>
  );
}

function ResetButton({ onReset }: { onReset: () => void }) {
  return (
    <Button size="sm" variant="ghost" onClick={() => void window.batlay.live.reset().then(onReset)} title="Remet les compteurs à zéro et vide le journal (les chats restent)">
      <RotateCcw size={14} /> Remettre à zéro
    </Button>
  );
}

function Stat({ icon, tint, label, twitch, tiktok, hint }: { icon: LucideIcon; tint: string; label: string; twitch: number; tiktok: number; hint?: string }) {
  return (
    <StatTile
      icon={icon}
      tint={tint}
      label={label}
      value={twitch + tiktok}
      title={hint}
      footer={
      <div className="flex gap-3 text-[11px] tabular-nums text-faint">
        <span className="flex items-center gap-1">
          <span className={`h-1.5 w-1.5 rounded-full ${PLATFORM_DOT.twitch}`} /> Twitch {twitch}
        </span>
        <span className="flex items-center gap-1">
          <span className={`h-1.5 w-1.5 rounded-full ${PLATFORM_DOT.tiktok}`} /> TikTok {tiktok}
        </span>
      </div>
      }
    />
  );
}

function ActivityRow({ entry, onReplay }: { entry: ActivityEntry; onReplay: (id: string) => void }) {
  if (entry.type === "system") {
    const Icon = LEVEL_ICONS[entry.level];
    return (
      <li className="row-in flex items-start gap-2 rounded-lg px-2 py-1.5 text-xs">
        <Icon size={14} className={`mt-0.5 shrink-0 ${LEVEL_COLORS[entry.level]}`} />
        <div className="min-w-0 flex-1">
          <span className="text-muted">{entry.text}</span>
          <div className="text-[10px] text-faint">
            {time(entry.at, true)} · {LIVE_PLATFORM_LABELS[entry.source]}
          </div>
        </div>
      </li>
    );
  }
  const Icon = KIND_ICONS[entry.kind];
  const shown = entry.outcome === "shown";
  return (
    <li className="row-in group flex items-start gap-2 rounded-lg border border-base-700/60 bg-base-800/40 px-2.5 py-2 text-xs transition hover:border-base-700 hover:bg-base-800/70">
      <Icon size={15} className={`mt-0.5 shrink-0 ${KIND_COLORS[entry.kind]}`} />
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-baseline gap-x-1.5">
          <span className="font-semibold text-fg">{alertTitle(entry.kind)}</span>
          <span className="text-[10px] uppercase tracking-wide text-faint">{LIVE_PLATFORM_LABELS[entry.source]}</span>
        </div>
        <div className="truncate text-fg">
          <span className="font-medium">{entry.user}</span> <span className="text-muted">{entry.detail}</span>
        </div>
        <div className={`mt-0.5 text-[10px] ${shown ? "text-ok" : "text-warn-strong"}`}>
          {time(entry.at, true)} · {ALERT_OUTCOME_LABELS[entry.outcome]}
        </div>
      </div>
      {entry.event && (
        <button
          type="button"
          onClick={() => onReplay(entry.id)}
          title="Rejouer cette alerte dans le logiciel de streaming"
          className="shrink-0 rounded-md px-1.5 py-1 text-[10px] text-muted opacity-70 transition hover:bg-base-700 hover:text-fg group-hover:opacity-100"
        >
          Rejouer
        </button>
      )}
    </li>
  );
}

/** Liste de chat qui suit le bas, sauf si l'utilisateur remonte lire (bouton « Nouveaux messages »). */
function ChatList({ messages, only, title }: { messages: LiveChatMessage[]; only: LivePlatform | null; title?: string }) {
  const list = useMemo(() => (only ? messages.filter((m) => m.platform === only) : messages), [messages, only]);
  const ref = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(true);
  const [unseen, setUnseen] = useState(0);
  const lastLen = useRef(list.length);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const added = list.length - lastLen.current;
    lastLen.current = list.length;
    if (stuck) el.scrollTop = el.scrollHeight;
    else if (added > 0) setUnseen((n) => n + added);
  }, [list, stuck]);

  return (
    <div className="relative flex min-h-0 flex-1 flex-col rounded-lg border border-base-700/60 bg-base-900/40">
      {title && (
        <div className="flex items-center gap-1.5 border-b border-base-700/60 px-3 py-1.5 text-[11px] font-medium text-muted">
          <span className={`h-2 w-2 rounded-full ${PLATFORM_DOT[only ?? "twitch"]}`} /> {title}
        </div>
      )}
      <div
        ref={ref}
        onScroll={(e) => {
          const el = e.currentTarget;
          const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
          setStuck(atBottom);
          if (atBottom) setUnseen(0);
        }}
        className="min-h-[220px] flex-1 space-y-1 overflow-y-auto px-3 py-2"
        aria-live="polite"
      >
        {list.length === 0 ? (
          <EmptyState compact icon={MessageSquare} title="Aucun message pour l'instant">
            {only === "tiktok"
              ? "Branchez votre live dans Alertes → TikTok : ses commentaires arriveront ici."
              : only === "twitch"
                ? "Connectez votre chaîne dans Chat Twitch : ses messages arriveront ici."
                : "Les messages de Twitch et de TikTok arriveront ici, par ordre d'arrivée."}
          </EmptyState>
        ) : (
          list.map((m) => <ChatLine key={m.id} message={m} showPlatform={only === null} />)
        )}
      </div>
      {!stuck && unseen > 0 && (
        <button
          type="button"
          onClick={() => {
            setStuck(true);
            setUnseen(0);
          }}
          className="absolute bottom-2 left-1/2 flex -translate-x-1/2 items-center gap-1 rounded-full bg-signal-600 px-3 py-1 text-[11px] font-medium text-white shadow-lg"
        >
          <ArrowDown size={12} /> {unseen} nouveau{unseen > 1 ? "x" : ""} message{unseen > 1 ? "s" : ""}
        </button>
      )}
    </div>
  );
}

function ChatLine({ message: m, showPlatform }: { message: LiveChatMessage; showPlatform: boolean }) {
  return (
    <div className="row-in text-[13px] leading-snug">
      <span className="mr-1.5 align-middle text-[10px] tabular-nums text-faint">{time(m.timestamp)}</span>
      {showPlatform && <span className={`mr-1.5 inline-block h-2 w-2 rounded-full align-middle ${PLATFORM_DOT[m.platform]}`} title={LIVE_PLATFORM_LABELS[m.platform]} />}
      <span className="font-semibold" style={{ color: m.platform === "tiktok" ? "#FE6C8A" : readableNameColor(m.color, m.userId) }}>
        {m.displayName}
      </span>
      <span className="text-muted">: </span>
      <span className="break-words text-fg">
        {m.fragments.map((f, i) =>
          f.type === "emote" ? (
            <img key={i} src={emoteUrl(f.emoteId)} alt={f.text} title={f.text} className="mx-0.5 inline-block h-5 w-auto align-middle" />
          ) : (
            <span key={i} className={f.type === "mention" ? "font-semibold text-accent" : undefined}>
              {f.text}
            </span>
          ),
        )}
      </span>
    </div>
  );
}
