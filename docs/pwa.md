# Web app (PWA)

`app.geauxhack.com` is an installable PWA. After one visit it opens with no network at all, deep links included. The native apps use the same build but don't need any of this: their files are already on the device.

## How offline works

- **Service worker:** `apps/client/src/sw.ts`, built by `vite-plugin-pwa` in `injectManifest` mode.
  - At install it precaches the whole app shell: `index.html`, the JS and CSS, the Allo font, the icons and the manifest (about 12 files).
  - Every in-app page (`/battle`, `/judge/:token`, …) is served `index.html` from the cache, so the router takes it from there.
  - `/api/` requests pass straight through. Caching data comes with the API.
  - A new deploy takes over as soon as its service worker installs, and the new version shows on the next load.
- **Registration:** by hand in `src/index.tsx`, after the page loads and only on the web. This keeps `workbox-window` out of the startup bundle.
- **Type-checking:** the worker has its own `tsconfig.sw.json`, because worker types clash with the page's DOM types.

## Install

- **Manifest:** generated from `vite.config.ts` (name, colors, `display: standalone`, icons).
- **Icons:** `public/icons/`, made from `assets/icon-only.png` by `scripts/make-pwa-icons.mjs`. `pnpm --filter @gh-core/client assets` rebuilds them along with the native ones. The crown stays inside the middle 80%, so the same art works as the maskable icon.
- **iPhone:** `index.html` has the `apple-touch-icon` and `apple-mobile-web-app-*` tags for Add to Home Screen.

## Checks (all in CI)

- **`pnpm pwa:check`** (`scripts/check-pwa.mjs`) fails the build if:
  - the manifest is missing anything browsers need to offer Install;
  - an icon file is missing;
  - the service worker doesn't precache `index.html`, the font, or any file the first screen needs.

  Lighthouse 12 removed its PWA category, so this replaces it.
- **Lighthouse CI** (`lighthouserc.json`): Performance ≥ 95, Accessibility = 100 and Best Practices = 100, median of 3 runs on the production build. It runs in the **Lighthouse** CI job, and the reports are attached to each run.
  - Running it locally (`pnpm --filter @gh-core/client lighthouse`) needs Chrome. On the Windows dev PC it couldn't start Edge, so CI is where it runs for now.

## Checking offline by hand

1. `pnpm --filter @gh-core/client build`, then `pnpm --filter @gh-core/client preview`.
2. Open the page once and wait a second for the service worker to install.
3. Stop the preview server, then reload or open any in-app path. It should still load.
