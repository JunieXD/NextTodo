import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { registerSW } from "virtual:pwa-register";
import { App } from "./App";
import { loadAiMode } from "./ai";
import { app, desktopOs, loadServer, native } from "./api/http";
import * as android from "./android";
import * as desktop from "./desktop";
import { loadMode } from "./local";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import { limitBrowserMenu } from "./ui/menu";
import "./index.css";
import { applyTheme } from "./theme";

applyTheme();
limitBrowserMenu();
// The macOS app draws under the title bar, around the window's buttons.
if (app === "desktop" && desktopOs === "macOS") document.documentElement.classList.add("macos");
// The app carries its files in the package; only the web app needs a service worker for offline use.
if (!native) registerSW({ immediate: true });
await Promise.all([loadServer(), loadMode(), loadAiMode()]);
if (app) await (app === "android" ? android : desktop).listen();
createRoot(document.getElementById("app")!).render(
  <StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </StrictMode>,
);
