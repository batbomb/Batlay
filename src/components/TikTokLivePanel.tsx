import { useEffect, useState, type FormEvent } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  Eye,
  EyeOff,
  KeyRound,
  Loader2,
  LogIn,
  LogOut,
  MinusCircle,
  Radio,
  Stethoscope,
  Trash2,
  XCircle,
} from "lucide-react";
import { Button, SectionTitle, StatusPill } from "@/components/ui";
import { SelectField, ToggleField } from "@/components/editor/fields";
import { useToastStore } from "@/stores/useToastStore";
import {
  initialTikTokState,
  normalizeTikTokMode,
  parseTikTokUsername,
  TIKTOK_MODES,
  type TikTokDiagnostic,
  type TikTokLiveState,
  type TikTokMode,
} from "../../shared/tiktok";

const DIAG_ICONS = {
  ok: <CheckCircle2 size={14} className="mt-0.5 shrink-0 text-ok" />,
  warn: <AlertTriangle size={14} className="mt-0.5 shrink-0 text-warn-strong" />,
  fail: <XCircle size={14} className="mt-0.5 shrink-0 text-danger" />,
  skip: <MinusCircle size={14} className="mt-0.5 shrink-0 text-faint" />,
} as const;

/**
 * Carte « TikTok » de la page Alertes. Trois moyens de connexion, au choix :
 *   - navigateur intégré (défaut) : Batlay lit la page du live ouverte dans une
 *     fenêtre invisible, sans service tiers ;
 *   - Euler Stream : un service tiers signe la connexion, Batlay lit le flux ;
 *   - adresse locale : un autre outil (TikFinity, Streamer.bot…) envoie les
 *     événements à Batlay, qui ne se connecte pas lui-même.
 */

const STATUS: Record<TikTokLiveState["status"], { tone: "ok" | "warn" | "neutral"; label: string }> = {
  idle: { tone: "neutral", label: "Déconnecté" },
  connecting: { tone: "neutral", label: "Connexion…" },
  live: { tone: "ok", label: "En live" },
  waiting: { tone: "warn", label: "En attente du live" },
  hidden: { tone: "warn", label: "Live caché aux visiteurs" },
  error: { tone: "warn", label: "Erreur" },
};

const GIFT_MIN_OPTIONS = [
  { value: 1, label: "Tous les cadeaux" },
  { value: 5, label: "5 diamants et plus" },
  { value: 10, label: "10 diamants et plus" },
  { value: 50, label: "50 diamants et plus" },
  { value: 100, label: "100 diamants et plus" },
  { value: 500, label: "500 diamants et plus" },
  { value: 1000, label: "1 000 diamants et plus" },
];

const inputClass = "min-w-0 flex-1 rounded-lg border border-base-700 bg-base-800 px-3 py-2 text-sm text-fg outline-none focus:border-signal-500";

function untilText(at: number | null, now: number): string | null {
  if (!at) return null;
  const s = Math.max(0, Math.round((at - now) / 1000));
  return s >= 60 ? `nouvel essai dans ${Math.ceil(s / 60)} min` : `nouvel essai dans ${s} s`;
}

export function TikTokLivePanel({
  sourceEnabled,
  onSourceEnabled,
  hookUrl,
  onRegenerateKey,
  copy,
}: {
  sourceEnabled: boolean;
  onSourceEnabled: (on: boolean) => void;
  hookUrl: string;
  onRegenerateKey: () => Promise<void>;
  copy: (text: string, message: string) => Promise<void>;
}) {
  const push = useToastStore((s) => s.push);
  const [state, setState] = useState<TikTokLiveState>(() => initialTikTokState());
  const [input, setInput] = useState("");
  const [now, setNow] = useState(() => Date.now());
  const [diag, setDiag] = useState<{ diagnostic: TikTokDiagnostic; report: string } | null>(null);
  const [diagnosing, setDiagnosing] = useState(false);
  const [pageVisible, setPageVisible] = useState(false);
  const [keyInput, setKeyInput] = useState("");
  const [savingKey, setSavingKey] = useState(false);

  useEffect(() => window.batlay.tiktok.onPageVisible?.(setPageVisible), []);

  async function togglePage() {
    const ok = await window.batlay.tiktok.showPage(!pageVisible).catch(() => false);
    if (!ok) push("Aucune page TikTok ouverte pour le moment : elle s'ouvre quand Batlay se branche sur votre live.", "info");
  }

  async function runDiagnostic() {
    setDiagnosing(true);
    try {
      setDiag(await window.batlay.tiktok.diagnose());
    } catch {
      push("Diagnostic impossible.", "error");
    } finally {
      setDiagnosing(false);
    }
  }

  useEffect(() => {
    let alive = true;
    void window.batlay.tiktok
      .getState()
      .then((s) => {
        if (!alive) return;
        setState(s);
        if (s.settings.username) setInput(`@${s.settings.username}`);
      })
      .catch(() => undefined);
    const off = window.batlay.tiktok.onState((s) => alive && setState(s));
    return () => {
      alive = false;
      off();
    };
  }, []);

  // Compte à rebours du prochain essai.
  useEffect(() => {
    if (!state.retryAt) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [state.retryAt]);

  const mode = normalizeTikTokMode(state.settings.mode);
  const connects = mode !== "local";
  const active = connects && state.settings.enabled;
  const parsed = parseTikTokUsername(input);
  const account = state.account && state.account.username === state.settings.username ? state.account : null;
  const status = state.status === "waiting" && account ? { tone: "neutral" as const, label: "Hors live" } : STATUS[state.status];

  async function connect(e: FormEvent) {
    e.preventDefault();
    if (!parsed) {
      push("Pseudo TikTok non valable : lettres, chiffres, « _ » et « . » (exemple : @mon.pseudo).", "error");
      return;
    }
    const ok = await window.batlay.tiktok.connect(parsed);
    if (!ok) push("Pseudo TikTok non valable.", "error");
  }

  async function chooseMode(next: TikTokMode) {
    if (next === mode) return;
    setDiag(null);
    try {
      const settings = await window.batlay.tiktok.updateSettings({ mode: next });
      setState((s) => ({ ...s, settings }));
      const label = TIKTOK_MODES.find((m) => m.value === next)?.label ?? next;
      push(
        next === "local"
          ? "Adresse locale : Batlay ne se connecte plus lui-même à TikTok. Collez l'adresse dans votre outil."
          : state.settings.enabled
            ? `Méthode « ${label} » : Batlay se rebranche.`
            : `Méthode « ${label} » choisie.`,
        "success",
      );
    } catch {
      push("Changement de méthode impossible.", "error");
    }
  }

  async function saveKey(key: string | null) {
    setSavingKey(true);
    try {
      const ok = await window.batlay.tiktok.setEulerKey(key);
      if (!ok) {
        push("Clé refusée : vérifiez-la (lettres, chiffres, - _ . = + /). Elle doit aussi pouvoir être chiffrée par Windows.", "error");
        return;
      }
      setKeyInput("");
      setState((s) => ({ ...s, eulerKey: key !== null }));
      push(key === null ? "Clé Euler retirée." : "Clé Euler enregistrée, chiffrée sur ce PC.", "success");
    } catch {
      push("Enregistrement de la clé impossible.", "error");
    } finally {
      setSavingKey(false);
    }
  }

  return (
    <section className="card p-6">
      <SectionTitle actions={<ToggleField label="Activé" checked={sourceEnabled} onChange={onSourceEnabled} />}>TikTok</SectionTitle>

      {/* --- Choix de la méthode de connexion --- */}
      <div className="mt-3">
        <span id="tiktok-mode-label" className="mb-1.5 block text-[11px] text-muted">
          Méthode de connexion
        </span>
        <div role="radiogroup" aria-labelledby="tiktok-mode-label" className="grid gap-1.5">
          {TIKTOK_MODES.map((option) => {
            const selected = option.value === mode;
            return (
              <button
                key={option.value}
                type="button"
                role="radio"
                aria-checked={selected}
                onClick={() => void chooseMode(option.value)}
                className={`flex items-center gap-3 rounded-xl border px-3 py-2.5 text-left transition ${
                  selected ? "border-signal-500/60 bg-signal-600/10 shadow-[0_0_0_1px_rgb(var(--signal-500)/0.2)]" : "border-base-700 bg-base-800/60 hover:border-base-600"
                }`}
              >
                {/* Bouton radio dessiné : anneau, point d'accent quand la méthode est choisie. */}
                <span
                  className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border-2 transition ${selected ? "border-signal-500" : "border-base-600"}`}
                  aria-hidden
                >
                  {selected && <span className="h-1.5 w-1.5 rounded-full bg-signal-500" />}
                </span>
                <span className="min-w-0">
                  <span className="block text-xs font-medium text-fg">{option.label}</span>
                  <span className="block text-[11px] leading-snug text-muted">{option.hint}</span>
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* --- Pseudo et état de la connexion (navigateur intégré, Euler) --- */}
      {connects &&
        (!active ? (
          <form className="mt-4 space-y-2" onSubmit={(e) => void connect(e)}>
            <label className="block">
              <span className="mb-1 block text-[11px] text-muted">Votre pseudo TikTok</span>
              <div className="flex items-center gap-2">
                <input
                  value={input}
                  onChange={(e) => setInput(e.target.value)}
                  placeholder="@pseudo"
                  autoComplete="off"
                  spellCheck={false}
                  aria-label="Pseudo TikTok"
                  className={inputClass}
                />
                <Button type="submit" size="sm" variant="primary" disabled={!parsed}>
                  <Radio size={14} /> Se connecter
                </Button>
              </div>
            </label>
            <p className="text-xs text-muted">
              Pas de mot de passe : le pseudo suffit. Batlay attend votre live et s'y branche tout seul, puis affiche les follows, les nouveaux
              Super Fans (abonnés) et les cadeaux.
            </p>
            {state.status === "idle" && state.settings.username && <p className="text-[11px] text-faint">Dernier compte suivi : @{state.settings.username}.</p>}
          </form>
        ) : (
          <div className="mt-4 space-y-2 text-sm">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex min-w-0 items-center gap-2">
                <span className="min-w-0 truncate">
                  {account && <span className="font-medium text-fg">{account.displayName} </span>}
                  <span className={account ? "text-xs text-muted" : "font-medium text-fg"}>@{state.settings.username}</span>
                </span>
                {account && <StatusPill tone="ok">Compte trouvé</StatusPill>}
                <StatusPill tone={status.tone}>{status.label}</StatusPill>
              </div>
              <Button size="sm" variant="ghost" onClick={() => void window.batlay.tiktok.disconnect()} title="Batlay arrête de suivre ce live">
                <LogOut size={14} /> Déconnecter
              </Button>
            </div>
            {state.message && (
              <p className={`whitespace-pre-line text-xs ${state.status === "error" ? "text-warn-strong" : "text-muted"}`}>
                {state.message}
                {(state.status === "waiting" || state.status === "hidden") && untilText(state.retryAt, now) && (
                  <span className="mt-0.5 block text-[11px] text-faint">{untilText(state.retryAt, now)?.replace(/^n/, "N")}.</span>
                )}
              </p>
            )}
            <div className="flex flex-wrap gap-2">
              {(state.status === "error" || state.status === "waiting" || state.status === "hidden") && (
                <Button size="sm" onClick={() => void window.batlay.tiktok.connect(state.settings.username ?? "")}>
                  Réessayer maintenant
                </Button>
              )}
              {mode === "browser" && (state.status === "live" || state.status === "connecting" || pageVisible) && (
                <Button
                  size="sm"
                  variant="ghost"
                  onClick={() => void togglePage()}
                  title="Montre la page TikTok ouverte en arrière-plan (utile si TikTok demande une vérification). La fermer la remet en arrière-plan, sans couper le live."
                >
                  {pageVisible ? <EyeOff size={14} /> : <Eye size={14} />} {pageVisible ? "Remasquer la page TikTok" : "Afficher la page TikTok"}
                </Button>
              )}
            </div>
            {state.detail && state.status !== "live" && (
              <details className="text-[11px] text-faint">
                <summary className="cursor-pointer hover:text-fg">Détails techniques (à copier si le live n'est pas détecté)</summary>
                <p className="mt-1 whitespace-pre-wrap break-words font-mono">{state.detail}</p>
                <Button size="sm" variant="ghost" className="mt-1" onClick={() => void copy(state.detail ?? "", "Détails copiés.")}>
                  <Copy size={14} /> Copier
                </Button>
              </details>
            )}
          </div>
        ))}

      {/* --- Navigateur intégré : session TikTok facultative (compte privé) --- */}
      {mode === "browser" && (
        <div className="mt-4 rounded-lg border border-base-700 px-3 py-2.5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex items-center gap-2 text-xs">
              <span className="text-muted">Compte TikTok dans Batlay</span>
              <StatusPill tone={state.loggedIn ? "ok" : "neutral"}>{state.loggedIn ? "Connecté" : "Non connecté"}</StatusPill>
            </div>
            {state.loggedIn ? (
              <Button size="sm" variant="ghost" onClick={() => void window.batlay.tiktok.logout().then(() => push("Session TikTok effacée de Batlay.", "success"))}>
                <LogOut size={14} /> Se déconnecter de TikTok
              </Button>
            ) : (
              <Button size="sm" onClick={() => void window.batlay.tiktok.login()}>
                <LogIn size={14} /> Se connecter à TikTok
              </Button>
            )}
          </div>
          <p className="mt-1.5 text-[11px] text-faint">
            Nécessaire pour un compte privé. La connexion se fait sur la page officielle de TikTok (le plus simple : code QR avec l'appli TikTok) :
            Batlay ne voit jamais votre mot de passe. La session reste sur ce PC et n'est envoyée à aucun service tiers.
          </p>
        </div>
      )}

      {/* --- Euler Stream : avertissement et clé API facultative --- */}
      {mode === "euler" && (
        <div className="mt-4 space-y-3 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2.5">
          <div className="flex items-start gap-2 text-xs text-muted">
            <AlertTriangle size={14} className="mt-0.5 shrink-0 text-warn-strong" />
            <p>
              <span className="font-medium text-fg">Service tiers.</span> Euler Stream reçoit l'identifiant de votre live (et votre clé, si vous en mettez
              une) pour signer la connexion ; Batlay lit ensuite le flux lui-même. Votre session TikTok ne lui est <span className="text-fg">jamais</span>{" "}
              envoyée : un compte privé ou un live restreint ne peut donc pas être lu par cette méthode. Le service peut limiter le nombre de
              connexions, changer ou s'arrêter.
            </p>
          </div>
          <div>
            <div className="mb-1 flex items-center gap-2">
              <span className="text-[11px] text-muted">Clé API Euler (facultative)</span>
              <StatusPill tone={state.eulerKey ? "ok" : "neutral"}>{state.eulerKey ? "Enregistrée" : "Aucune"}</StatusPill>
            </div>
            <form
              className="flex items-center gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (keyInput.trim()) void saveKey(keyInput.trim());
              }}
            >
              <input
                type="password"
                value={keyInput}
                onChange={(e) => setKeyInput(e.target.value)}
                placeholder={state.eulerKey ? "Remplacer la clé…" : "Collez votre clé"}
                autoComplete="off"
                spellCheck={false}
                aria-label="Clé API Euler Stream"
                className={inputClass}
              />
              <Button type="submit" size="sm" disabled={!keyInput.trim() || savingKey}>
                <KeyRound size={14} /> Enregistrer
              </Button>
              {state.eulerKey && (
                <Button size="sm" variant="ghost" disabled={savingKey} onClick={() => void saveKey(null)} title="Retire la clé de ce PC">
                  <Trash2 size={14} />
                </Button>
              )}
            </form>
            <p className="mt-1 text-[11px] text-faint">
              Sans clé, les limites gratuites d'Euler s'appliquent. Une clé gratuite (compte sur eulerstream.com) les relève. Elle est chiffrée par
              Windows sur ce PC et n'est envoyée qu'à Euler.
            </p>
          </div>
        </div>
      )}

      {/* --- Adresse locale : l'outil de l'utilisateur appelle Batlay --- */}
      {mode === "local" && (
        <div className="mt-4 space-y-2 text-xs text-muted">
          <p>
            Batlay ne se connecte pas à TikTok. Dans votre outil (TikFinity, Streamer.bot…), créez une action « requête web » vers cette adresse pour
            chaque événement : Batlay affiche l'alerte.
          </p>
          <div className="flex items-center gap-2">
            <input
              readOnly
              value={hookUrl}
              aria-label="Adresse locale des alertes TikTok"
              onFocus={(e) => e.currentTarget.select()}
              className="min-w-0 flex-1 rounded-lg border border-base-700 bg-base-800 px-3 py-2 font-mono text-[11px] text-fg outline-none focus:border-signal-500"
            />
            <Button size="sm" onClick={() => void copy(hookUrl, "Adresse copiée.")}>
              <Copy size={14} /> Copier
            </Button>
          </div>
          <ul className="space-y-1.5 font-mono text-[11px] text-fg">
            <li>{hookUrl}?type=follow&amp;user=NOM</li>
            <li>{hookUrl}?type=tiktokGift&amp;user=NOM&amp;gift=Rose&amp;amount=5</li>
          </ul>
          <p>Requête GET ou POST (JSON). Types : follow, subscribe, resub, gift, tiktokGift.</p>
          <div className="flex items-center justify-between gap-2">
            <p className="text-[11px] text-faint">La clé à la fin de l'adresse empêche quiconque d'autre de déclencher vos alertes.</p>
            <Button size="sm" variant="ghost" title="L'ancienne adresse cesse de fonctionner" onClick={() => void onRegenerateKey()}>
              <KeyRound size={14} /> Nouvelle clé
            </Button>
          </div>
        </div>
      )}

      {/* --- Diagnostic : chaque étape vérifiée, rapport à copier --- */}
      {connects && state.settings.username && (
        <div className="mt-4 rounded-lg border border-base-700 px-3 py-2.5">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className="text-xs text-muted">Ça ne marche pas ? Vérifiez chaque étape.</span>
            <Button size="sm" onClick={() => void runDiagnostic()} disabled={diagnosing}>
              {diagnosing ? <Loader2 size={14} className="animate-spin" /> : <Stethoscope size={14} />} {diagnosing ? "Vérification…" : "Diagnostic"}
            </Button>
          </div>
          {diag && (
            <div className="mt-2">
              <ul className="space-y-1.5">
                {diag.diagnostic.steps.map((step) => (
                  <li key={step.label} className="flex items-start gap-2 text-xs">
                    {DIAG_ICONS[step.status]}
                    <span className="min-w-0">
                      <span className="font-medium text-fg">{step.label}</span> <span className="break-words text-muted">· {step.detail}</span>
                    </span>
                  </li>
                ))}
              </ul>
              <Button size="sm" variant="ghost" className="mt-2" onClick={() => void copy(diag.report, "Rapport copié : collez-le dans votre message.")}>
                <Copy size={14} /> Copier le rapport
              </Button>
            </div>
          )}
        </div>
      )}

      {connects && (
        <div className="mt-4">
          <SelectField<number>
            label="Alertes de cadeaux"
            value={state.settings.giftMinDiamonds}
            options={
              GIFT_MIN_OPTIONS.some((o) => o.value === state.settings.giftMinDiamonds)
                ? GIFT_MIN_OPTIONS
                : [...GIFT_MIN_OPTIONS, { value: state.settings.giftMinDiamonds, label: `${state.settings.giftMinDiamonds} diamants et plus` }]
            }
            onChange={(giftMinDiamonds) => void window.batlay.tiktok.updateSettings({ giftMinDiamonds }).then((settings) => setState((s) => ({ ...s, settings })))}
          />
          <p className="mt-1 text-[11px] text-faint">Valeur du cadeau, quantité comprise : 1 Rose = 1 diamant. Une série de cadeaux donne une seule alerte.</p>
        </div>
      )}

      <details className="mt-3 text-xs text-muted">
        <summary className="cursor-pointer hover:text-fg">À savoir</summary>
        <div className="mt-2 space-y-2">
          <p>
            TikTok ne propose pas d'accès officiel aux événements d'un live : chaque méthode lit seulement (rien n'est écrit dans votre live), peut
            cesser de fonctionner si TikTok change son site, et n'est pas autorisée par TikTok.
          </p>
          <p>
            <span className="text-fg">Navigateur intégré</span> : Batlay ouvre la page de votre live dans une fenêtre invisible (rien à l'écran, son
            coupé) et décode lui-même ce que TikTok envoie à cette page. Aucun service tiers : votre pseudo ne part que vers tiktok.com.
          </p>
          <p>
            <span className="text-fg">Euler Stream</span> : la même lecture du flux, sans fenêtre, mais la connexion est signée par ce service tiers.
            Utile si la page invisible est bloquée chez vous.
          </p>
          <p>
            <span className="text-fg">Adresse locale</span> : un autre outil se connecte à TikTok et prévient Batlay. Une seule méthode est active à la
            fois : pas d'alertes en double.
          </p>
        </div>
      </details>
    </section>
  );
}
