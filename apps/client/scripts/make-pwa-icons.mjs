// Builds the web app icons in public/icons/ from assets/icon-only.png, the
// same source as the native icons. Run by `pnpm assets` after the native step.
// The crown sits inside the middle 80%, so the same art works as a maskable icon.
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

// sharp comes with @capacitor/assets.
const require = createRequire(import.meta.resolve("@capacitor/assets"));
const sharp = require("sharp");

const source = fileURLToPath(
  new URL("../assets/icon-only.png", import.meta.url),
);
const out = fileURLToPath(new URL("../public/icons/", import.meta.url));
mkdirSync(out, { recursive: true });

const sizes = [
  ["icon-192.png", 192],
  ["icon-512.png", 512],
  ["icon-maskable-512.png", 512],
  // iOS home screen icon.
  ["apple-touch-icon.png", 180],
];

for (const [name, size] of sizes) {
  await sharp(source)
    .resize(size, size)
    .png()
    .toFile(out + name);
  console.log(`wrote public/icons/${name}`);
}
