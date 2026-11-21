// The Android and iOS apps: the web app bundled in a Capacitor shell. See docs/android.md and docs/ios.md.

import type { CapacitorConfig } from "@capacitor/cli";

const config: CapacitorConfig = {
  appId: "cn.juniexd.nexttodo",
  appName: "NextTodo",
  webDir: "dist",
  // The bridge would log every call with its arguments, the password and the token among them.
  loggingBehavior: "none",
  android: {
    path: "../android",
    // The app's pages are https://localhost: with a server on plain HTTP (docs/design.md#直接用-http),
    // the sync connection and the attachments' files count as mixed content.
    allowMixedContent: true,
    // Not the keyboard plugin: Android's WebView gets shorter by itself as the keyboard opens.
    includePlugins: ["@capacitor/app", "@capacitor/preferences"],
  },
  ios: {
    path: "../ios",
    // Reminders are the app's own, and their actions must be heard before the page loads (docs/ios.md#通知).
    handleApplicationNotifications: false,
  },
  plugins: {
    // Requests go out through native HTTP, so the server needs no CORS for the app's https://localhost pages.
    CapacitorHttp: { enabled: true },
    // The page's safe area is read from the variables it writes in this mode (index.css).
    SystemBars: { insetsHandling: "css" },
    // iOS's WebView, as Android's does, gets shorter as the keyboard opens, rather than the page being
    // pushed up under the status bar.
    Keyboard: { resize: "native" },
  },
};

export default config;
