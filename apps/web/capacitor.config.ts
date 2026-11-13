// The Android app: the web app bundled in a Capacitor shell. See docs/android.md.

import type { CapacitorConfig } from "@capacitor/cli";

const config: CapacitorConfig = {
  appId: "cn.juniexd.nexttodo",
  appName: "NextTodo",
  webDir: "dist",
  // The bridge would log every call with its arguments, the password and the token among them.
  loggingBehavior: "none",
  android: {
    path: "../android",
    // A development server is plain HTTP; the app's pages are https://localhost, so its WebSocket
    // counts as mixed content. Set for `cap sync` only when building against one.
    allowMixedContent: process.env.NEXTTODO_DEV_SERVER === "1",
  },
  plugins: {
    // Requests go out through native HTTP, so the server needs no CORS for the app's https://localhost pages.
    CapacitorHttp: { enabled: true },
  },
};

export default config;
