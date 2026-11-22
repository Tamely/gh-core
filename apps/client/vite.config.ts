import tailwindcss from "@tailwindcss/vite";
import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [solid(), tailwindcss()],
  server: {
    // `pnpm dev` talks to `auth-dev serve` (server/auth). auth-dev treats
    // http://127.0.0.1:8080 as the web app's origin, so present that to get
    // the web cookie (any other Origin is answered like a native app).
    proxy: {
      "/api": {
        target: "http://127.0.0.1:8080",
        headers: { origin: "http://127.0.0.1:8080" },
      },
    },
  },
  build: {
    target: "es2022",
    // The budget check reads this to find exactly what loads at startup.
    manifest: true,
  },
  test: {
    environment: "jsdom",
  },
});
