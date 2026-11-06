import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowRight, Cpu, Palette, Radio, RefreshCw, Sparkles } from "lucide-react";
import { Button, Modal } from "@/components/ui";
import { BrandMark } from "@/components/BrandMark";
import { appVersion, shortVersion } from "@/utils/app-version";

/**
 * Accueil de la 0.5, affiché une seule fois : ce qui change, et la seule chose
 * à refaire (reconnecter Twitch et Spotify). Mémorisé dans ce navigateur
 * (WebView2) : simple confort d'affichage, aucune donnée de l'utilisateur.
 */
const SEEN_KEY = "batlay.welcome.0.5";

function alreadySeen(): boolean {
  try {
    return localStorage.getItem(SEEN_KEY) === "1";
  } catch {
    // Stockage indisponible : ne pas réafficher l'accueil à chaque ouverture.
    return true;
  }
}

function markSeen() {
  try {
    localStorage.setItem(SEEN_KEY, "1");
  } catch {
    // Rien à faire : l'accueil se refermera simplement.
  }
}

const HIGHLIGHTS = [
  { icon: Palette, title: "Nouvelle identité", text: "Nouveau logo, barre de titre Batlay, interface repensée en clair comme en sombre." },
  { icon: Cpu, title: "Moteur réécrit en Rust", text: "Plus léger et plus fiable. Tes overlays, adresses OBS et réglages ne changent pas." },
  { icon: Radio, title: "TikTok, trois méthodes", text: "Navigateur intégré, Euler Stream ou adresse locale : au choix dans Alertes." },
] as const;

export function WhatsNew() {
  const [open, setOpen] = useState(() => !alreadySeen());
  const navigate = useNavigate();
  const version = appVersion();
  if (!open) return null;

  const close = (to?: string) => {
    markSeen();
    setOpen(false);
    if (to) navigate(to);
  };

  return (
    <Modal title="Bienvenue dans Batlay 0.5" onClose={() => close()} widthClass="w-[540px]">
      <div className="-mx-6 -mt-6 mb-5 overflow-hidden rounded-t-[20px] border-b border-line px-6 pb-5 pt-7" style={{ background: "radial-gradient(120% 140% at 0% 0%, rgb(var(--signal-600) / 0.28), transparent 60%), radial-gradient(90% 120% at 100% 0%, rgb(var(--nova) / 0.18), transparent 60%)" }}>
        <div className="flex items-center gap-4">
          <BrandMark className="h-14 w-14" playing />
          <div>
            <p className="flex items-center gap-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-accent">
              <Sparkles size={12} /> Nouvelle version{version ? ` · ${shortVersion(version)}` : ""}
            </p>
            <h2 className="mt-1 font-display text-2xl font-semibold text-fg">Bienvenue dans Batlay 0.5</h2>
          </div>
        </div>
      </div>

      <ul className="space-y-3">
        {HIGHLIGHTS.map(({ icon: Icon, title, text }) => (
          <li key={title} className="flex items-start gap-3">
            <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-signal-600/15 text-accent ring-1 ring-inset ring-signal-500/20">
              <Icon size={17} />
            </span>
            <span className="min-w-0">
              <span className="block text-sm font-medium text-fg">{title}</span>
              <span className="block text-xs leading-relaxed text-muted">{text}</span>
            </span>
          </li>
        ))}
      </ul>

      <div className="surface-inset mt-5 flex items-start gap-3 px-3.5 py-3">
        <RefreshCw size={15} className="mt-0.5 shrink-0 text-warn-strong" />
        <p className="text-xs leading-relaxed text-muted">
          <span className="font-medium text-fg">Une seule chose à refaire :</span> reconnecter Twitch et Spotify. Pour ta sécurité, les connexions de
          l'ancienne version ne peuvent pas être relues.
        </p>
      </div>

      <div className="mt-5 flex flex-wrap items-center justify-between gap-2">
        <div className="flex flex-wrap gap-1">
          <Button size="sm" variant="ghost" onClick={() => close("/chat")}>
            Reconnecter Twitch
          </Button>
          <Button size="sm" variant="ghost" onClick={() => close("/connections")}>
            Reconnecter Spotify
          </Button>
        </div>
        <Button variant="primary" onClick={() => close()}>
          C'est parti <ArrowRight size={15} />
        </Button>
      </div>
    </Modal>
  );
}
