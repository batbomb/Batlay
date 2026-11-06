import { AlertCircle, CheckCircle2, Info, X } from "lucide-react";
import { useToastStore } from "@/stores/useToastStore";

const KIND_STYLES: Record<string, string> = {
  info: "border-base-700",
  success: "border-live/35",
  error: "border-red-500/40",
};

/** Liseré de couleur à gauche de la notification (signature visuelle 0.5). */
const KIND_BARS: Record<string, string> = {
  info: "bg-gradient-to-b from-signal-400 to-nova",
  success: "bg-ok",
  error: "bg-danger",
};

const KIND_ICONS = {
  info: <Info size={16} className="text-accent" />,
  success: <CheckCircle2 size={16} className="text-ok" />,
  error: <AlertCircle size={16} className="text-danger" />,
} as const;

export function ToastHost() {
  const toasts = useToastStore((s) => s.toasts);
  const dismiss = useToastStore((s) => s.dismiss);

  if (toasts.length === 0) return null;

  return (
    // aria-live : les lecteurs d'écran annoncent les confirmations sans que le focus bouge.
    <div role="status" aria-live="polite" className="fixed bottom-6 right-6 z-50 flex w-[min(380px,calc(100vw-3rem))] flex-col gap-2">
      {toasts.map((t) => (
        <button
          key={t.id}
          onClick={() => dismiss(t.id)}
          title="Fermer"
          className={`relative flex items-start gap-3 overflow-hidden rounded-2xl border bg-base-900/95 py-3 pl-4 pr-3.5 text-left text-sm leading-snug text-fg shadow-pop backdrop-blur-xl transition hover:brightness-110 toast-in ${KIND_STYLES[t.kind]}`}
        >
          <span className={`absolute inset-y-2 left-1.5 w-[3px] rounded-full ${KIND_BARS[t.kind]}`} aria-hidden="true" />
          <span className="mt-0.5 shrink-0">{KIND_ICONS[t.kind]}</span>
          <span className="min-w-0 flex-1">{t.message}</span>
          <X size={14} className="mt-0.5 shrink-0 text-faint" />
        </button>
      ))}
    </div>
  );
}
