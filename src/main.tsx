// En premier : window.batlay (pont vers le cœur de Batlay) existe avant tout le reste.
import "./platform/bridge";
import React from "react";
import ReactDOM from "react-dom/client";
import { HashRouter } from "react-router-dom";
import App from "./App";
import { OverlayMediaContext } from "@/components/OverlayMedia";
import { DASHBOARD_MEDIA_ENV } from "@/components/media-cache";
import "@fontsource-variable/inter";
// Identité 0.5 : titres en Sora. Space Grotesk reste chargée : les aperçus d'overlays l'utilisent.
import "@fontsource-variable/sora";
import "@fontsource-variable/space-grotesk";
import "@fontsource-variable/jetbrains-mono";
import "./styles/globals.css";
import { initTheme } from "./theme/apply-theme";

// Thème posé AVANT le premier rendu React : sinon la première image serait
// dessinée dans le mauvais thème (flash). Lecture synchrone : l'application
// l'a déjà résolu (« system » compris) et posé dans la page avant son chargement.
initTheme();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <HashRouter>
      <OverlayMediaContext.Provider value={DASHBOARD_MEDIA_ENV}>
        <App />
      </OverlayMediaContext.Provider>
    </HashRouter>
  </React.StrictMode>
);
