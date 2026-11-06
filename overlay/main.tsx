import React from "react";
import ReactDOM from "react-dom/client";
import { OverlayApp } from "./OverlayApp";
// Polices de Batlay proposées dans l'éditeur (fichiers servis par le serveur local, rien d'extérieur).
import "@fontsource-variable/inter";
import "@fontsource-variable/space-grotesk";
import "@fontsource-variable/jetbrains-mono";

ReactDOM.createRoot(document.getElementById("overlay-root")!).render(
  <React.StrictMode>
    <OverlayApp />
  </React.StrictMode>
);
