// Fails the build if the initial shell is over budget.
//
// "Initial shell" = the HTML entry chunk, every chunk it imports statically, and
// their CSS, i.e. everything the browser must download before first render.
// Lazily imported chunks (dynamic import()) are not counted.
//
// Sizes are gzip level 9. Reads dist/.vite/manifest.json, so run after `vite build`.
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { gzipSync } from "node:zlib";

const BUDGET_BYTES = 35 * 1024;
const dist = fileURLToPath(new URL("../dist/", import.meta.url));

let manifest;
try {
  manifest = JSON.parse(readFileSync(`${dist}.vite/manifest.json`, "utf8"));
} catch {
  console.error("No build manifest found. Run `pnpm build` first.");
  process.exit(1);
}

const entryKey = Object.keys(manifest).find((key) => manifest[key].isEntry);
if (!entryKey) {
  console.error("No entry chunk found in the build manifest.");
  process.exit(1);
}

const files = new Set();
const seen = new Set();
const visit = (key) => {
  if (seen.has(key)) return;
  seen.add(key);
  const chunk = manifest[key];
  files.add(chunk.file);
  for (const css of chunk.css ?? []) files.add(css);
  for (const imported of chunk.imports ?? []) visit(imported);
};
visit(entryKey);

let total = 0;
const rows = [...files].sort().map((file) => {
  const gz = gzipSync(readFileSync(dist + file), { level: 9 }).length;
  total += gz;
  return { file, gzipKB: (gz / 1024).toFixed(2) };
});

console.table(rows);
const used = ((total / BUDGET_BYTES) * 100).toFixed(0);
const summary = `Initial shell: ${(total / 1024).toFixed(2)} KB gzipped of ${BUDGET_BYTES / 1024} KB budget (${used}%).`;

if (total > BUDGET_BYTES) {
  console.error(`${summary} Over budget.`);
  process.exit(1);
}
console.log(summary);
