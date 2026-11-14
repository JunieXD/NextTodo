import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { registerSW } from "virtual:pwa-register";
import { App } from "./App";
import { loadAiMode } from "./ai";
import { app, desktopOs, loadServer, native } from "./api/http";
import * as android from "./android";
import * as desktop from "./desktop";
import { loadMode } from "./local";
import { onSignInElsewhere } from "./session";
import { ErrorBoundary } from "./ui/ErrorBoundary";
import { preloadMarkdown } from "./ui/lazy-markdown";
import { limitBrowserMenu } from "./ui/menu";
import { restorePage } from "./views/Panel";
import "./index.css";
import { applyTheme } from "./theme";

applyTheme();
limitBrowserMenu();
// The macOS app draws under the title bar, around the window's buttons.
if (app === "desktop" && desktopOs === "macOS") document.documentElement.classList.add("macos");
if (desktop.inPanel) {
  document.documentElement.classList.add("in-panel");
  restorePage();
  // A sign-in in the main window, local mode's new session too, loads the panel again, whatever it
  // shows, for the server and session it keeps in memory (api/http.ts) to be read anew.
  onSignInElsewhere(() => location.reload());
}
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
preloadMarkdown();
