import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";
import { defineConfig } from "vitest/config";

const server = "http://127.0.0.1:8080";

export default defineConfig({
  plugins: [
    react(),
    tailwindcss(),
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
  server: {
    proxy: {
      "/api": { target: server, ws: true },
      "/mcp": server,
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
  },
});
