import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { registerSW } from "virtual:pwa-register";
import { App } from "./App";
import { app, loadServer, native } from "./api/http";
import * as android from "./android";
import * as macos from "./macos";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import "./index.css";
import { applyTheme } from "./theme";

applyTheme();
// The macOS app draws under the title bar, around the window's buttons.
if (app === "macos") document.documentElement.classList.add("macos");
// The app carries its files in the package; only the web app needs a service worker for offline use.
if (!native) registerSW({ immediate: true });
await loadServer();
if (app) await (app === "android" ? android : macos).listen();
createRoot(document.getElementById("app")!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
