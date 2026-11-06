import { useState } from "react";
import { Link, NavLink, useLocation } from "react-router-dom";
import { Activity, Bell, Cast, Link2, LayoutDashboard, Layers, MessageSquare, Music2, PanelLeftClose, PanelLeftOpen, Plug, SlidersHorizontal, type LucideIcon } from "lucide-react";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useMusicStore } from "@/stores/useMusicStore";
import { useOverlayStore } from "@/stores/useOverlayStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { NAV_GROUPS } from "@/layouts/nav";
import { useCopyObsUrl } from "@/hooks/useCopyObsUrl";
import { pickMainOverlay } from "@/utils/ui-helpers";
import { useTikTokState } from "@/hooks/useLive";
import type { TikTokLiveState } from "../../shared/tiktok";

export const NAV_ICONS: Record<string, LucideIcon> = {
  "/": LayoutDashboard,
  "/live": Activity,
  "/musique": Music2,
  "/overlays": Layers,
  "/integrations": Cast,
  "/alerts": Bell,
  "/connections": Plug,
  "/chat": MessageSquare,
  "/settings": SlidersHorizontal,
};

const SOURCE_LABELS: Record<string, string> = {
  spotify: "Spotify connecté",
  demo: "Mode démo",
  "system-media": "Lecture système",
};

interface SidebarProps {
  /** Version étroite (icônes seules) : rend de la place à l'éditeur. */
  compact: boolean;
  /** Affiché seulement dans l'éditeur, où la barre se réduit toute seule. */
  onToggleCompact?: () => void;
}

export function Sidebar({ compact, onToggleCompact }: SidebarProps) {
  const { pathname } = useLocation();
  const overlayServerRunning = useSettingsStore((s) => s.overlayServerRunning);
  const activeProviderId = useMusicStore((s) => s.activeProviderId);
  const { overlays, activeOverlayId } = useOverlayStore((s) => ({ overlays: s.overlays, activeOverlayId: s.activeOverlayId }));
  const twitch = useTwitchStore((s) => s.state);
  const tiktok = useTikTokState();
  const tiktokLive = tiktok.status === "live";
  const copyObsUrl = useCopyObsUrl();
  const mainOverlay = pickMainOverlay(overlays, activeOverlayId);

  const sourceLabel = activeProviderId ? (SOURCE_LABELS[activeProviderId] ?? activeProviderId) : "Aucune source";

  return (
    <aside
      className={`relative z-10 flex shrink-0 flex-col border-r border-line/80 bg-base-900/55 backdrop-blur-xl transition-[width] duration-300 ${compact ? "w-[68px]" : "w-[236px]"}`}
    >
      <nav className={`flex flex-1 flex-col gap-0.5 overflow-y-auto pt-4 ${compact ? "px-2.5" : "px-3"}`} aria-label="Navigation principale">
        {NAV_GROUPS.map((group, groupIndex) => (
          <div key={group.id} className={`flex flex-col gap-0.5 ${groupIndex > 0 ? "mt-3 border-t border-line/70 pt-3" : ""}`}>
            {group.items.map(({ to, label }) => {
              const Icon = NAV_ICONS[to];
              return (
                <NavLink
                  key={to}
                  to={to}
                  end={to === "/"}
                  title={compact ? label : undefined}
                  aria-label={compact ? label : undefined}
                  className={({ isActive }) => {
                    // L'éditeur fait partie de « Overlays » : sans ça, aucun onglet ne serait allumé pendant l'édition.
                    const active = isActive || (to === "/overlays" && pathname.startsWith("/editor"));
                    return `group relative flex h-9 items-center rounded-[10px] text-[13px] font-medium transition duration-150 ${
                      compact ? "justify-center px-0" : "gap-3 px-2.5"
                    } ${active ? "nav-item-active" : "text-muted hover:bg-base-800/60 hover:text-fg"}`;
                  }}
                >
                  {({ isActive }) => {
                    const active = isActive || (to === "/overlays" && pathname.startsWith("/editor"));
                    return (
                      <>
                        {/* Repère d'onglet actif : un trait lumineux sur le bord gauche. */}
                        <span
                          className={`nav-indicator absolute top-1/2 h-4 w-[3px] -translate-y-1/2 rounded-full transition-all duration-200 ${
                            compact ? "-left-2.5" : "-left-3"
                          } ${active ? "opacity-100" : "scale-y-0 opacity-0"}`}
                          aria-hidden="true"
                        />
                        <Icon size={17} strokeWidth={active ? 2.2 : 1.9} className={active ? "text-accent" : "transition group-hover:text-fg"} />
                        {!compact && <span className="truncate">{label}</span>}
                        {/* Live TikTok en cours : repère sur l'entrée « Live ». */}
                        {to === "/live" && tiktokLive &&
                          (compact ? (
                            <span className="live-badge absolute right-1.5 top-1.5 h-2 w-2 rounded-full bg-[#FE2C55]" aria-label="En live" />
                          ) : (
                            <span className="live-badge tag-live ml-auto rounded-full px-1.5 py-px text-[9px] font-bold uppercase tracking-wider">Live</span>
                          ))}
                      </>
                    );
                  }}
                </NavLink>
              );
            })}
          </div>
        ))}
      </nav>

      <div className={`space-y-2 pb-3 pt-3 text-xs text-muted ${compact ? "px-2.5" : "px-3"}`}>
        {mainOverlay &&
          (compact ? (
            <button
              onClick={() => void copyObsUrl(mainOverlay.id)}
              className="btn-secondary flex h-9 w-full items-center justify-center rounded-[10px] text-accent transition"
              title={`Copier l'URL OBS de « ${mainOverlay.name} »`}
              aria-label="Copier l'URL OBS"
            >
              <Link2 size={15} />
            </button>
          ) : (
            <button
              onClick={() => void copyObsUrl(mainOverlay.id)}
              className="btn-secondary flex w-full items-center gap-2.5 rounded-[10px] px-2.5 py-2 text-left text-fg transition"
              title={`Copier l'URL OBS de « ${mainOverlay.name} »`}
            >
              <span className="flex h-7 w-7 shrink-0 items-center justify-center rounded-lg bg-signal-600/15 text-accent">
                <Link2 size={14} />
              </span>
              <span className="min-w-0">
                <span className="block text-xs font-medium">Copier l'URL OBS</span>
                <span className="block truncate text-[11px] text-muted">{mainOverlay.name}</span>
              </span>
            </button>
          ))}

        {/* Compte Twitch connecté (photo de profil, ou initiale). */}
        {twitch.account && (
          <Link
            to="/chat"
            title={`Twitch : ${twitch.account.displayName}`}
            aria-label={`Compte Twitch : ${twitch.account.displayName}`}
            className={`flex items-center rounded-[10px] transition hover:bg-base-800/70 ${compact ? "justify-center py-1" : "gap-2.5 px-2 py-1.5"}`}
          >
            <AccountAvatar name={twitch.account.displayName} url={twitch.account.profileImageUrl} />
            {!compact && (
              <span className="min-w-0">
                <span className="block truncate text-xs font-medium text-fg">{twitch.account.displayName}</span>
                <span className="block text-[11px] text-muted">Twitch connecté</span>
              </span>
            )}
          </Link>
        )}

        {/* État du système : chaque ligne mène à la page où corriger le problème. */}
        <div className={compact ? "flex flex-col items-center gap-1 py-1" : "surface-inset px-1.5 py-1.5"}>
          {!compact && <p className="px-1.5 pb-1 pt-0.5 text-[10px] font-semibold uppercase tracking-[0.08em] text-faint">État</p>}
          <StatusLink
            to="/settings"
            compact={compact}
            dotClass={overlayServerRunning ? "dot-live" : "bg-red-500"}
            label={overlayServerRunning ? "Serveur d'overlay en ligne" : "Serveur d'overlay hors ligne"}
          />
          <StatusLink to="/connections" compact={compact} dotClass={activeProviderId ? "dot-live" : "bg-amber-400"} label={sourceLabel} />
          <StatusLink to="/chat" compact={compact} {...chatIndicator(twitch)} />
          <StatusLink to="/alerts" compact={compact} {...tiktokIndicator(tiktok)} />
        </div>

        {onToggleCompact && (
          <button
            onClick={onToggleCompact}
            className={`flex h-8 w-full items-center rounded-lg text-muted transition hover:bg-base-800/70 hover:text-fg ${compact ? "justify-center" : "gap-2 px-2.5"}`}
            title={compact ? "Agrandir la barre latérale" : "Réduire la barre latérale"}
            aria-label={compact ? "Agrandir la barre latérale" : "Réduire la barre latérale"}
          >
            {compact ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
            {!compact && <span className="text-xs">Réduire</span>}
          </button>
        )}
      </div>
    </aside>
  );
}

/** Photo de profil du compte, ou son initiale si elle manque ou ne charge pas. */
function AccountAvatar({ name, url }: { name: string; url: string | null }) {
  const [failed, setFailed] = useState(false);
  if (url && !failed) {
    return <img src={url} alt="" onError={() => setFailed(true)} className="h-7 w-7 shrink-0 rounded-full object-cover ring-2 ring-signal-500/40" />;
  }
  return (
    <span className="brand-tile flex h-7 w-7 shrink-0 items-center justify-center rounded-full text-xs font-bold" aria-hidden="true">
      {name.slice(0, 1).toUpperCase()}
    </span>
  );
}

/** Ligne d'état cliquable : mène à la page où l'on peut corriger le problème. */
function StatusLink({ to, dotClass, label, compact }: { to: string; dotClass: string; label: string; compact: boolean }) {
  return (
    <Link
      to={to}
      title={label}
      aria-label={label}
      className={`flex items-center rounded-md transition hover:bg-base-800/70 hover:text-fg ${compact ? "h-6 w-6 justify-center" : "gap-2.5 px-1.5 py-[5px]"}`}
    >
      <span className={`h-[7px] w-[7px] shrink-0 rounded-full ${dotClass}`} />
      {!compact && <span className="truncate text-[11.5px]">{label}</span>}
    </Link>
  );
}

/** Pastille d'état du chat Twitch (indépendante de l'état de la musique). */
function chatIndicator(t: ReturnType<typeof useTwitchStore.getState>["state"]): { dotClass: string; label: string } {
  if (!t.server.running) return { dotClass: "bg-red-500", label: "Serveur chat hors ligne" };
  if (t.status === "connected" && t.chat === "live") {
    return { dotClass: "dot-live", label: t.channel ? `Chat de ${t.channel.displayName} en direct` : "Chat Twitch en direct" };
  }
  if (t.status === "connected" && t.error) return { dotClass: "bg-amber-400", label: "Chat Twitch : chaîne à corriger" };
  if (t.status === "connected") return { dotClass: "bg-amber-400", label: "Chat Twitch : connexion…" };
  return { dotClass: "bg-amber-400", label: "Chat Twitch déconnecté" };
}

/** Pastille d'état du live TikTok (mène à la carte TikTok de la page Alertes). */
function tiktokIndicator(t: TikTokLiveState): { dotClass: string; label: string } {
  const who = t.settings.username ? `@${t.settings.username}` : "";
  if (t.settings.mode === "local") return { dotClass: "bg-base-700", label: "TikTok : adresse locale (votre outil)" };
  switch (t.status) {
    case "live":
      return { dotClass: "dot-live", label: `TikTok ${who} en live` };
    case "connecting":
      return { dotClass: "bg-amber-400", label: "TikTok : connexion…" };
    case "waiting":
      return { dotClass: "bg-base-700", label: `TikTok : en attente du live` };
    case "hidden":
      return { dotClass: "bg-amber-400", label: "TikTok : live caché aux visiteurs" };
    case "error":
      return { dotClass: "bg-red-500", label: "TikTok : erreur, voir Alertes" };
    default:
      return { dotClass: "bg-base-700", label: "TikTok non branché" };
  }
}
