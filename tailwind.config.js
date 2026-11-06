/**
 * Les couleurs de l'interface (base, fg, muted...) ne sont PAS des valeurs
 * fixes : elles pointent vers des variables CSS RGB posées sur <html> par
 * src/theme/apply-theme.ts, à partir de shared/theme.ts (source de
 * vérité unique dark/light). Le format `rgb(var(--x) / <alpha-value>)` garde
 * fonctionnels les modificateurs d'opacité (`bg-base-800/60`, `border-live/40`).
 *
 * tests/theme-tailwind.test.ts vérifie que cette liste reste synchronisée
 * avec les tokens.
 */
const token = (name) => `rgb(var(--${name}) / <alpha-value>)`;

/** @type {import('tailwindcss').Config} */
export default {
  darkMode: "class",
  content: ["./index.html", "./overlay.html", "./src/**/*.{ts,tsx}", "./overlay/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        // Palette Batlay 0.5 (« Nova ») : encre profonde, accent indigo + second ton cyan.
        base: {
          950: token("base-950"),
          900: token("base-900"),
          800: token("base-800"),
          700: token("base-700"),
          600: token("base-600"),
        },
        line: token("line"),
        fg: token("fg"),
        muted: token("muted"),
        faint: token("faint"),
        // Texte de statut lisible sur les surfaces du thème actif (les
        // couleurs de fond / bordure ci-dessous restent, elles, identiques).
        accent: token("accent"),
        ok: token("ok"),
        danger: token("danger"),
        "danger-soft": token("danger-soft"),
        warn: token("warn"),
        "warn-strong": token("warn-strong"),
        // Accents de marque (boutons, focus...) : violet Batlay par défaut,
        // mais personnalisable par l'utilisateur (Settings > Apparence) —
        // voir src/theme/accent.ts. Indépendant du thème clair/sombre :
        // les valeurs par défaut sont posées une fois dans globals.css.
        signal: {
          400: token("signal-400"),
          500: token("signal-500"),
          600: token("signal-600"),
        },
        // Second ton « nova » de l'identité 0.5 (dérivé de l'accent, voir src/theme/accent.ts).
        nova: token("nova"),
        live: "#37E29A",
        // Fond de « scène » derrière un aperçu d'overlay : il représente ce que
        // verra le spectateur, donc jamais thématisé.
        stage: "#0A0A0F",
      },
      fontFamily: {
        // Identité 0.5 : Sora pour les titres et les chiffres (Space Grotesk reste chargée pour les overlays).
        display: ["'Sora Variable'", "'Sora'", "'Inter Variable'", "system-ui", "sans-serif"],
        body: ["'Inter Variable'", "'Inter'", "system-ui", "sans-serif"],
        mono: ["'JetBrains Mono Variable'", "'JetBrains Mono'", "monospace"],
      },
      borderRadius: {
        xl2: "1.25rem",
      },
      boxShadow: {
        glow: "0 0 24px -6px rgb(var(--signal-500) / 0.5)",
        pop: "var(--shadow-pop)",
      },
    },
  },
  plugins: [],
};
