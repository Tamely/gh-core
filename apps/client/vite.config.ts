import tailwindcss from "@tailwindcss/vite";
import solid from "vite-plugin-solid";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [solid(), tailwindcss()],
  build: {
    target: "es2022",
    // The budget check reads this to find exactly what loads at startup.
    manifest: true,
  },
  test: {
    environment: "jsdom",
  },
});
