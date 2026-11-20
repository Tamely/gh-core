// Draws PLACEHOLDER app icon and splash sources into apps/client/assets/:
// a gold crown on Beau purple. Replace these PNGs with the real artwork when
// it's ready, then run `pnpm assets` to regenerate every native size.
import { mkdirSync } from "node:fs";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

// sharp comes with @capacitor/assets.
const require = createRequire(import.meta.resolve("@capacitor/assets"));
const sharp = require("sharp");

const out = fileURLToPath(new URL("../assets/", import.meta.url));
mkdirSync(out, { recursive: true });

const crown = (size, color, stroke) => `
  <g transform="translate(${size / 2 - size * 0.28} ${size / 2 - size * 0.3}) scale(${(size * 0.56) / 24})"
     fill="none" stroke="${color}" stroke-width="${stroke}" stroke-linecap="round" stroke-linejoin="round">
    <path d="M3 18h18M4 18l-1-11 5 4 4-7 4 7 5-4-1 11M6 21h12"/>
  </g>`;

const svg = (size, bg, fg, stroke) =>
  Buffer.from(
    `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}">
       ${bg ? `<rect width="100%" height="100%" fill="${bg}"/>` : ""}
       ${fg ? crown(size, fg, stroke) : ""}
     </svg>`,
  );

const files = [
  // iOS and the web: the full icon.
  ["icon-only.png", svg(1024, "#461D7C", "#FDD023", 1.6)],
  // Android adaptive icon: the crown on a transparent layer, plus a plain background.
  ["icon-foreground.png", svg(1024, null, "#FDD023", 1.6)],
  ["icon-background.png", svg(1024, "#461D7C", null)],
  // Splash screens: a small crown on cream (light) and on Abyssus (dark).
  ["splash.png", svg(2732, "#FDF7EC", "#461D7C", 0.5)],
  ["splash-dark.png", svg(2732, "#0E0B14", "#FDD023", 0.5)],
];

for (const [name, data] of files) {
  await sharp(data)
    .png()
    .toFile(out + name);
  console.log(`wrote assets/${name}`);
}
