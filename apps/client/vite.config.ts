import tailwindcss from "@tailwindcss/vite";
import { VitePWA } from "vite-plugin-pwa";
import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [
    solid(),
    tailwindcss(),
    VitePWA({
      // Our own service worker (src/sw.ts); the plugin injects the precache list.
      strategies: "injectManifest",
      srcDir: "src",
      filename: "sw.ts",
      // Registered by hand in src/index.tsx, so workbox-window stays out of the bundle.
      injectRegister: false,
      // The plugin adds the manifest and its icons to the precache list itself;
      // listing them here too would duplicate entries, which Workbox rejects.
      injectManifest: {
        globPatterns: [
          "**/*.{js,css,html,woff2}",
          "icons/apple-touch-icon.png",
        ],
      },
      manifest: {
        id: "/",
        name: "Geaux Hack 2027",
        short_name: "Geaux Hack",
        description: "The WiCS Geaux Hack 2027 event app.",
        start_url: "/",
        scope: "/",
        display: "standalone",
        orientation: "portrait",
        background_color: "#FDF7EC",
        theme_color: "#FDF7EC",
        icons: [
          { src: "/icons/icon-192.png", sizes: "192x192", type: "image/png" },
          { src: "/icons/icon-512.png", sizes: "512x512", type: "image/png" },
          {
            src: "/icons/icon-maskable-512.png",
            sizes: "512x512",
            type: "image/png",
            purpose: "maskable",
          },
        ],
      },
    }),
  ],
  build: {
    target: "es2022",
    // The budget check reads this to find exactly what loads at startup.
    manifest: true,
  },
  test: {
    environment: "jsdom",
  },
});
