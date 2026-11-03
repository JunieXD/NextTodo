import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { registerSW } from "virtual:pwa-register";
import { App } from "./App";
import { loadServer, native } from "./api/http";
import { listen } from "./native";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import "./index.css";
import { applyTheme } from "./theme";

applyTheme();
// The app carries its files in the package; only the web app needs a service worker for offline use.
if (!native) registerSW({ immediate: true });
await loadServer();
if (native) await listen();
createRoot(document.getElementById("app")!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
