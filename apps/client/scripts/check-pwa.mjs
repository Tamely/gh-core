// Checks the built app is an installable, offline-capable PWA. Lighthouse 12
// dropped its PWA category, so this replaces those checks. Run after `vite build`.
//
// - The manifest has what browsers need to offer "Install" / "Add to Home Screen".
// - index.html links the manifest and the iOS home screen icon.
// - The service worker exists and precaches index.html, the font, and every
//   file the first screen needs, so the app opens offline after one visit.
import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const dist = fileURLToPath(new URL("../dist/", import.meta.url));
const problems = [];
const check = (ok, message) => ok || problems.push(message);
const read = (path) => readFileSync(dist + path, "utf8");

// Manifest
check(
  existsSync(dist + "manifest.webmanifest"),
  "dist/manifest.webmanifest is missing",
);
const manifest = JSON.parse(read("manifest.webmanifest"));
for (const field of [
  "name",
  "short_name",
  "start_url",
  "display",
  "background_color",
  "theme_color",
]) {
  check(manifest[field], `manifest is missing "${field}"`);
}
check(
  ["standalone", "fullscreen", "minimal-ui"].includes(manifest.display),
  `manifest display "${manifest.display}" won't open as an app`,
);
const icons = manifest.icons ?? [];
for (const size of ["192x192", "512x512"]) {
  check(
    icons.some((icon) => icon.sizes === size),
    `manifest has no ${size} icon`,
  );
}
check(
  icons.some((icon) => icon.purpose?.includes("maskable")),
  "manifest has no maskable icon",
);
for (const icon of icons) {
  check(
    existsSync(dist + icon.src.replace(/^\//, "")),
    `icon ${icon.src} is missing from dist`,
  );
}

// index.html
const html = read("index.html");
check(/rel="manifest"/.test(html), "index.html doesn't link the manifest");
check(
  /rel="apple-touch-icon"/.test(html),
  "index.html has no apple-touch-icon",
);
check(/name="theme-color"/.test(html), "index.html has no theme-color");

// Service worker and its precache list
check(existsSync(dist + "sw.js"), "dist/sw.js is missing");
const sw = read("sw.js");
const precached = new Set(
  [...sw.matchAll(/"url":"([^"]+)"/g)].map((m) => m[1]),
);
const viteManifest = JSON.parse(read(".vite/manifest.json"));
const entry = Object.values(viteManifest).find((chunk) => chunk.isEntry);
const needed = [
  "index.html",
  "fonts/allo-latin.woff2",
  entry.file,
  ...(entry.css ?? []),
];
for (const file of needed) {
  check(
    precached.has(file),
    `service worker doesn't precache ${file}, so it won't load offline`,
  );
}

if (problems.length) {
  console.error(`PWA check failed:\n- ${problems.join("\n- ")}`);
  process.exit(1);
}
console.log(
  `PWA check passed: installable manifest, ${precached.size} files precached for offline.`,
);
