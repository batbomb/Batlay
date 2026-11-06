import { useMemo } from "react";
import { Bell, Copy, ExternalLink, Gift, Heart, MessageSquare, Minimize2, Moon, Plus, SkipForward, Star, Sun, SunMoon } from "lucide-react";
import { CommandPalette, type Command } from "@/components/CommandPalette";
import { NAV_GROUPS } from "@/layouts/nav";
import { NAV_ICONS } from "@/layouts/Sidebar";
import { useOverlayStore } from "@/stores/useOverlayStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { useToastStore } from "@/stores/useToastStore";
import { useCopyObsUrl } from "@/hooks/useCopyObsUrl";
import { pickMainOverlay } from "@/utils/ui-helpers";
import { ALERT_KIND_LABELS, type AlertKind } from "../../shared/alerts";

/** Les commandes de la palette (Ctrl+K) : pages, URL à copier, alertes de test, apparence. */
export function AppCommands() {
  const { overlays, activeOverlayId } = useOverlayStore((s) => ({ overlays: s.overlays, activeOverlayId: s.activeOverlayId }));
  const { settings, update } = useSettingsStore((s) => ({ settings: s.settings, update: s.update }));
  const chatUrl = useTwitchStore((s) => s.state.server.overlayUrl);
  const push = useToastStore((s) => s.push);
  const copyObsUrl = useCopyObsUrl();

  const commands = useMemo<Command[]>(() => {
    const copy = async (text: string | undefined, ok: string) => {
      if (!text) return push("Adresse indisponible pour le moment.", "error");
      try {
        await navigator.clipboard.writeText(text);
        push(ok, "success");
      } catch {
        push("Copie impossible.", "error");
      }
    };
    const list: Command[] = NAV_GROUPS.flatMap((g) =>
      g.items.map((item) => {
        const Icon = NAV_ICONS[item.to];
        return { id: `go:${item.to}`, title: item.label, group: "Aller à", icon: Icon ? <Icon size={15} /> : undefined, run: ({ navigate }) => navigate(item.to) };
      }),
    );
    const main = pickMainOverlay(overlays, activeOverlayId);
    list.push(
      { id: "live-window", title: "Ouvrir la fenêtre Live", group: "Live", keywords: "chat journal compteurs second ecran", icon: <ExternalLink size={15} />, run: () => void window.batlay.live.openWindow() },
      ...(main ? [{ id: "copy-music", title: `Copier l'URL OBS « ${main.name} »`, group: "Copier", keywords: "musique overlay lien url obs source", icon: <Copy size={15} />, run: () => void copyObsUrl(main.id) } satisfies Command] : []),
      ...overlays
        .filter((o) => o.id !== main?.id)
        .map((o): Command => ({ id: `copy:${o.id}`, title: `Copier l'URL OBS « ${o.name} »`, group: "Copier", keywords: "musique overlay lien url obs", icon: <Copy size={15} />, run: () => void copyObsUrl(o.id) })),
      { id: "copy-chat", title: "Copier l'URL du chat", group: "Copier", keywords: "twitch tiktok lien url obs", icon: <MessageSquare size={15} />, run: () => copy(chatUrl, "URL du chat copiée.") },
      {
        id: "copy-alerts",
        title: "Copier l'URL des alertes",
        group: "Copier",
        keywords: "follow abonnement cadeau lien url obs",
        icon: <Bell size={15} />,
        run: async () => copy((await window.batlay.alerts.get()).urls.page, "URL des alertes copiée."),
      },
      { id: "new-overlay", title: "Créer un overlay", group: "Overlays", keywords: "nouveau ajouter musique", icon: <Plus size={15} />, run: ({ navigate }) => navigate("/overlays") },
    );
    const testIcons: Record<AlertKind, JSX.Element> = { follow: <Heart size={15} />, subscribe: <Star size={15} />, resub: <Star size={15} />, gift: <Gift size={15} />, tiktokGift: <Gift size={15} /> };
    (Object.keys(ALERT_KIND_LABELS) as AlertKind[]).forEach((k) =>
      list.push({
        id: `test:${k}`,
        title: `Tester l'alerte « ${ALERT_KIND_LABELS[k]} »`,
        group: "Alertes",
        keywords: "essai test alerte",
        icon: testIcons[k],
        run: async () => {
          const r = await window.batlay.alerts.test(k);
          push(r.shown ? `Alerte « ${ALERT_KIND_LABELS[k]} » envoyée.` : "Aucune page d'alertes ouverte dans votre logiciel de streaming.", r.shown ? "success" : "error");
        },
      }),
    );
    list.push({ id: "skip-alert", title: "Passer l'alerte en cours", group: "Alertes", keywords: "arreter stop vider file", icon: <SkipForward size={15} />, run: () => void window.batlay.alerts.skip() });
    if (settings) {
      list.push(
        { id: "theme-dark", title: "Thème sombre", group: "Apparence", keywords: "nuit noir dark", icon: <Moon size={15} />, run: () => void update({ theme: "dark" }) },
        { id: "theme-light", title: "Thème clair", group: "Apparence", keywords: "jour blanc light", icon: <Sun size={15} />, run: () => void update({ theme: "light" }) },
        { id: "theme-system", title: "Thème de Windows", group: "Apparence", keywords: "systeme auto automatique", icon: <SunMoon size={15} />, run: () => void update({ theme: "system" }) },
        {
          id: "density",
          title: settings.density === "compact" ? "Densité confortable" : "Densité compacte",
          group: "Apparence",
          keywords: "taille petit grand interface",
          icon: <Minimize2 size={15} />,
          run: () => void update({ density: settings.density === "compact" ? "comfortable" : "compact" }),
        },
      );
    }
    return list;
  }, [overlays, activeOverlayId, settings, update, chatUrl, push, copyObsUrl]);

  return <CommandPalette commands={commands} />;
}
