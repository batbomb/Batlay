import { useEffect, useState } from "react";
import { Navigate, Routes, Route, useLocation } from "react-router-dom";
import { Sidebar } from "@/layouts/Sidebar";
import { TitleBar } from "@/layouts/TitleBar";
import { WhatsNew } from "@/components/WhatsNew";
import { UpdateInstallOverlay } from "@/components/UpdateInstallOverlay";
import { ToastHost } from "@/components/ToastHost";
import { Dashboard } from "@/pages/Dashboard";
import { Home } from "@/pages/Home";
import { Overlays } from "@/pages/Overlays";
import { Editor } from "@/pages/Editor";
import { Connections } from "@/pages/Connections";
import { Twitch } from "@/pages/Twitch";
import { Settings } from "@/pages/Settings";
import { Integrations } from "@/pages/Integrations";
import { Alerts } from "@/pages/Alerts";
import { Live } from "@/pages/Live";
import { AppCommands } from "@/components/AppCommands";
import { useOverlayStore } from "@/stores/useOverlayStore";
import { useSettingsStore } from "@/stores/useSettingsStore";
import { useMusicStore } from "@/stores/useMusicStore";
import { useUpdateStore } from "@/stores/useUpdateStore";
import { useTwitchStore } from "@/stores/useTwitchStore";
import { initPointerEffects } from "@/utils/pointer-effects";
import { applyAccentColor } from "@/theme/accent";
import { applyUiPrefs } from "@/theme/ui-prefs";

export default function App() {
  const loadOverlays = useOverlayStore((s) => s.load);
  const loadSettings = useSettingsStore((s) => s.load);
  const tryRestoreSpotify = useMusicStore((s) => s.tryRestoreSpotifySession);
  const initUpdates = useUpdateStore((s) => s.init);
  const initTwitch = useTwitchStore((s) => s.init);
  const { pathname, search } = useLocation();
  const inEditor = pathname.startsWith("/editor");
  // Fenêtre Live détachée (#/live?window=1) : page seule, sans barre latérale ni tâches de fond
  // (la musique, les overlays et les mises à jour restent gérés par la fenêtre principale).
  const detachedLive = pathname === "/live" && new URLSearchParams(search).get("window") === "1";
  // Dans l'éditeur la barre latérale se réduit à des icônes (la fenêtre fait
  // 1024 px au minimum : sans ça, il ne resterait qu'un canvas minuscule entre
  // les deux panneaux). L'utilisateur peut la rouvrir ; on l'oublie en sortant.
  const [editorSidebarOpen, setEditorSidebarOpen] = useState(false);
  // Hors éditeur : barre réduite au choix de l'utilisateur, mémorisée (préférence d'affichage seulement).
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => {
    try {
      return localStorage.getItem("batlay.sidebar.collapsed") === "1";
    } catch {
      return false;
    }
  });
  const toggleSidebar = () =>
    setSidebarCollapsed((c) => {
      try {
        localStorage.setItem("batlay.sidebar.collapsed", c ? "0" : "1");
      } catch {
        // Stockage indisponible : le choix vaut pour cette session.
      }
      return !c;
    });

  useEffect(() => {
    // Étape 3 du démarrage de Batlay : charger les données sauvegardées
    // et restaurer l'état de connexion lorsque c'est possible.
    if (detachedLive) return;
    loadOverlays();
    loadSettings();
    tryRestoreSpotify();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [loadOverlays, loadSettings, tryRestoreSpotify]);

  // État des mises à jour (poussé par le cœur de Batlay).
  useEffect(() => (detachedLive ? undefined : initUpdates()), [initUpdates]); // eslint-disable-line react-hooks/exhaustive-deps

  // État du module Twitch (poussé par le cœur de Batlay).
  useEffect(() => initTwitch(), [initTwitch]);

  // Petits effets de souris (parallaxe du fond, halo des cartes et boutons).
  useEffect(() => initPointerEffects(), []);

  useEffect(() => {
    if (!inEditor) setEditorSidebarOpen(false);
  }, [inEditor]);

  // Fenêtre détachée : elle ne charge pas les réglages complets, mais applique l'accent, la densité et les animations.
  useEffect(() => {
    if (!detachedLive) return;
    void window.batlay.config.get().then((c) => {
      applyAccentColor(document.documentElement, c.settings.accentColor ?? null);
      applyUiPrefs(document.documentElement, c.settings);
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (detachedLive) {
    return (
      <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-base-950">
        <TitleBar variant="live" title="Fenêtre Live" />
        <main className="relative z-10 min-h-0 flex-1 overflow-hidden">
          <Live />
        </main>
        <ToastHost />
      </div>
    );
  }

  return (
    <div className="relative flex h-screen w-screen flex-col overflow-hidden bg-base-950">
      <div className="app-aurora" aria-hidden="true" />
      {/* Barre de titre Batlay (les fenêtres n'ont plus celle de Windows depuis la 0.5). */}
      <TitleBar />
      <div className="relative z-10 flex min-h-0 flex-1">
      <Sidebar
        compact={inEditor ? !editorSidebarOpen : sidebarCollapsed}
        onToggleCompact={inEditor ? () => setEditorSidebarOpen((open) => !open) : toggleSidebar}
      />
      <main key={pathname} className="page-enter relative z-10 min-w-0 flex-1 overflow-y-auto">
        <Routes>
          <Route path="/" element={<Home />} />
          {/* Section Musique : pages inchangées, seule l'ancienne page d'accueil change d'adresse. */}
          <Route path="/musique" element={<Dashboard />} />
          <Route path="/overlays" element={<Overlays />} />
          <Route path="/editor/:overlayId" element={<Editor />} />
          <Route path="/connections" element={<Connections />} />
          {/* Section Chat */}
          <Route path="/chat" element={<Twitch />} />
          <Route path="/twitch" element={<Navigate to="/chat" replace />} />
          {/* Mode d'emploi multi-logiciels (OBS, TikTok LIVE Studio, Streamlabs, XSplit…). */}
          <Route path="/integrations" element={<Integrations />} />
          <Route path="/alerts" element={<Alerts />} />
          {/* Chats Twitch + TikTok, journal d'activité, compteurs (aussi en fenêtre détachée). */}
          <Route path="/live" element={<Live />} />
          <Route path="/settings" element={<Settings />} />
        </Routes>
      </main>
      </div>
      <AppCommands />
      <WhatsNew />
      <UpdateInstallOverlay />
      <ToastHost />
    </div>
  );
}
