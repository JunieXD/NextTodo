import { execFileSync } from "node:child_process";
import { mkdirSync, readdirSync, writeFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import type { Plugin } from "vite";
import { VitePWA } from "vite-plugin-pwa";
import { defineConfig } from "vitest/config";

const server = "http://127.0.0.1:8080";

const git = (...args: string[]) => execFileSync("git", ["-C", "../..", ...args], { encoding: "utf8" }).trim();
/** The build's version: the commit it is built from. */
const version = git("rev-parse", "--short", "HEAD");
/** The native version of the Android app this build needs, as apps/android/app/build.gradle counts
 * it: the commits that changed the native part. */
const native = Number(git("rev-list", "--count", "HEAD", "--", "apps/android", "apps/web/capacitor.config.ts", "apps/web/package.json"));

/** Writes `app/web.json`, from which the Android app downloads newer builds (docs/android.md#更新):
 * the version, the native version needed, and the files. */
function webJson(): Plugin {
  let dir = "";
  return {
    name: "web-json",
    apply: "build",
    configResolved(config) {
      dir = resolve(config.root, config.build.outDir);
    },
    closeBundle: {
      order: "post",
      handler() {
        const files = readdirSync(dir, { recursive: true, withFileTypes: true })
          .filter((e) => e.isFile())
          .map((e) => relative(dir, join(e.parentPath, e.name)))
          .sort();
        mkdirSync(join(dir, "app"), { recursive: true });
        writeFileSync(join(dir, "app/web.json"), JSON.stringify({ version, native, files }));
      },
    },
  };
}

export default defineConfig({
  define: { "import.meta.env.WEB_VERSION": JSON.stringify(version) },
  plugins: [
    react(),
    tailwindcss(),
    webJson(),
    VitePWA({
      registerType: "autoUpdate",
      includeAssets: ["icon.svg", "apple-touch-icon.png"],
      manifest: {
        name: "NextTodo",
        short_name: "NextTodo",
        description: "按先后关系安排的待办",
        lang: "zh-CN",
        start_url: "/",
        display: "standalone",
        background_color: "#F5FBF6",
        theme_color: "#F5FBF6",
        icons: [
          { src: "icon-192.png", sizes: "192x192", type: "image/png" },
          { src: "icon-512.png", sizes: "512x512", type: "image/png" },
          { src: "icon-maskable-512.png", sizes: "512x512", type: "image/png", purpose: "maskable" },
        ],
      },
      workbox: {
        // The API, sync and MCP never come from the cache.
        // Matched against the path and query: `/api`, `/api/…`, `/api?…`, and the same for `/mcp`.
        navigateFallbackDenylist: [/^\/(api|mcp)([/?]|$)/],
        globPatterns: ["**/*.{js,css,html,svg,png,webmanifest}"],
      },
    }),
  ],
  build: {
    // Vite's default, Baseline Widely Available, with Chromium 99 for 111: the WebView of Huawei phones
    // without Google's services (docs/web.md#浏览器).
    target: ["chrome99", "edge111", "firefox114", "safari16.4", "ios16.4"],
  },
  server: {
    proxy: {
      "/api": { target: server, ws: true },
      "/mcp": server,
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
    setupFiles: ["src/test-setup.ts"],
  },
});
