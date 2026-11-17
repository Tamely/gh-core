# Josh: handoff

_Last updated: 2026-11-17_

## Owns
**PR 1 series: client shell.** The SolidJS PWA at `apps/client/` and its Capacitor iOS/Android wrapper: offline shell, PWA, native plugins, store builds, OTA updates.

## Status
- **PR 1.1 (scaffold):** pushed on branch `pr-1.1-scaffold`. The draft PR isn't open yet because the built-in browser isn't signed in to GitHub, so CI hasn't run there.
- **PR 1.2 (app shell):** on branch `pr-1.2-app-shell`, built on top of 1.1 and not committed yet.
  - Done: theme from the style tile, the Allo font, the custom router, storage, five glass tabs, and Home and Schedule from sample data.
  - All checks pass locally (16 tests). The initial shell is 13.17KB gzipped, 38% of the budget.
  - Checked in the browser at phone size, in both themes. The design direction is in [`docs/design.md`](../docs/design.md): the Allo font, round and bouncy iOS feel, Elysiu (light) / Abyssus (dark) themes, District colors, and a registration flow modeled on HackMIT's. PR 1.2 turns it into the Tailwind theme.

### PR plan
| PR | Scope | Blocked on |
|---|---|---|
| 1.1 Scaffold | pnpm workspace, `apps/client` (SolidJS, Vite, strict TypeScript, Tailwind v4), lint/format, CI (lint, type-check, test, build), bundle budget: **initial shell JS + CSS under 35KB gzipped** | none (in review) |
| 1.2 App shell | Small custom router (measured against `@solidjs/router`), navigation, schedule from bundled sample data, storage interface (IndexedDB on web, `localStorage` snapshot for the first render), `shell:rendered` performance mark, design tokens | none (in progress) |
| 1.3 PWA | Workbox service worker (`vite-plugin-pwa`, `injectManifest`), manifest and maskable icons, offline fallback, Lighthouse CI (Performance ≥ 95, Accessibility and Best Practices = 100), installability check | none |
| 1.4 Capacitor | iOS and Android projects, `com.geauxhack.app`, icons and splash, native SQLite storage backend, plugins loaded on demand (only on native), haptics, offline cold-start measurement on physical devices (under 100ms, Airplane Mode) | none |
| 1.5 Scanner + push | `@capacitor-mlkit/barcode-scanning` (camera permissions), `@capacitor/push-notifications` (APNs key, Firebase `google-services.json`, secrets kept out of git) | ops: Apple Developer account, Firebase project |
| 1.6 Store builds | fastlane `ios beta` (local Mac) and `android beta`, signing set up, TestFlight and Play Closed Testing live on physical devices | ops: store accounts, 12 testers |
| 1.7 Deep links | Associated Domains / App Links for `app.geauxhack.org` (`/judge/:token`, `/auth/callback`, `/n/:id`) | infra: hosting the `.well-known` files |
| 1.9 Registration UI | HackMIT-style application lifecycle: public timeline, multi-step application saved as drafts, status card, confirmation (RSVP). See `docs/design.md`. | auth: sign-in; backend: application API |
| 1.8 OTA | `@capgo/capacitor-updater` self-hosted: signed bundles, `manifest.json` with minimum native version, update check on foreground, rollback via `notifyAppReady()`, CI publishing step. Built and tested against a local server first. | infra: CDN bucket for the production setup |

## Decisions
- **SolidJS**, chosen over Preact for the smallest runtime and no virtual DOM.
- **Capacitor: latest stable major.** Not 6: its Android target SDK is below Google Play's current requirement.
- **App ID / bundle ID: `com.geauxhack.app`.** It can't be changed after the first store upload.
- **Production domain: `app.geauxhack.org`.**
- **Router: custom, in `apps/client/src/router.tsx`.** Measured 2026-11-17 by building the same three-route test app three ways (gzip level 9):

  | Variant | Bundle | Router's cost |
  |---|---|---|
  | No router | 3.65KB | — |
  | Custom router | 4.78KB | **1.1KB** |
  | `@solidjs/router` 1.0.3 | 12.85KB | **9.2KB** |

  The first version cost 3.4KB because `<Link>` spread its props onto the `<a>`, which pulls in Solid's spread helper. **Don't spread props onto DOM elements anywhere in the client;** list them explicitly.
- **Lighthouse:** the PWA category was removed in Lighthouse 12. It's replaced with Performance ≥ 95, Accessibility = 100, Best Practices = 100, plus installability checks.
- **100ms offline startup** is measured from WebView `performance.timeOrigin` to the `shell:rendered` mark. It's the median of 10 cold starts in Airplane Mode on a physical iPhone and a physical mid-range Android phone.
- **Google Play is a personal account**, so 12 testers have to be opted in for 14 days in a row. The 14-day clock must **start by 2026-12-15**.
- **iOS builds happen on Josh's Mac** through fastlane.
- **Hosting is decided later.** Until then, PR 1.1–1.6 and the local version of 1.8 don't depend on it.
- **Font: Allo** (SIL Open Font License, self-hosted). It's cut down to a **50KB** Latin subset with weight 400–800, roundness fixed at 80 and optical size fixed at 24. Sizes were measured; see `docs/design.md`.
- **Visual style:** HackUTD-level simplicity and motion. Style tile: https://claude.ai/artifact/LPhptxiLDtmHg42fyfagLa
- **Animation uses CSS `linear()` spring easing only**, with no JavaScript animation library, to stay inside the 35KB budget. Everything respects `prefers-reduced-motion`.
- **Registration copies HackMIT's flow structure only.** Their visuals, wording and code are not used.
- **Toolchain (PR 1.1):** Node 24, pnpm 11.18, Vite 8, Tailwind 4.3, ESLint 10, Prettier 3, Vitest 5.
  - **Solid 1.9, not 2.0.** Solid 2.0.0 came out on 2026-11-17 and `vite-plugin-solid` only supports it in a pre-release. Revisit once the plugin's 3.0 is stable.
  - **TypeScript 6.0.3, not 7.** `typescript-eslint` supports TypeScript below 6.1 only.
  - **pnpm's minimum release age stays on.** Pin to versions at least a day old rather than adding exceptions to `pnpm-workspace.yaml`. That's why `typescript-eslint` is 8.72.2.
- **Bundle budget check:** `apps/client/scripts/check-budget.mjs` reads Vite's build manifest and gzips the entry chunk, every chunk it imports statically, and their CSS. Lazy `import()` chunks aren't counted. Used instead of `size-limit` because a filename glob can't tell entry chunks from lazy ones.
- **Commit hook turns on automatically:** `pnpm install` runs `prepare`, which sets `core.hooksPath` to `.githooks`.
- **Allo in the repo:** `apps/client/public/fonts/allo-latin.woff2` (49.5KB) with `OFL.txt` beside it, built by `apps/client/scripts/subset-allo.py`. It's preloaded in `index.html`. The fallback font is Arial adjusted to Allo's metrics (`size-adjust: 107%`, the same as Inter's, since Allo is partly based on Inter), so text doesn't shift when Allo loads.
- **Times always display in the event's zone** (`America/Chicago`), whatever the device's zone, using `Intl.DateTimeFormat`.
- **The web build never includes native plugins.** Every Capacitor plugin is imported on demand, only on native.

## Provides to others
- **Reserved client routes** (unstable until 1.2 merges): `/` (Home, which is also the schedule; `/schedule` still opens it), `/scan`, `/map`, `/battle`, `/help`, `/profile`, `/judge/:token`, `/auth/callback`, `/n/:short_id`, `/privacy`.
- **Storage interface** (1.2): a key-value and table store with a web backend (IndexedDB) and a native backend (SQLite). The auth PR's token storage and the scanner's local scan queue should use it, or sit next to it, rather than adding a second storage layer.
- **Client calls to the API are same-origin** (`/api/v1/...`). On native they use `Authorization: Bearer`; on web, cookies. This matches the auth ticket.
- **Signing details for deep links** (1.6–1.7): the Apple Team ID and the SHA-256 fingerprint of the Android signing certificate, for the `.well-known` files.

## Needs from others
| For | What's needed | Why / blocks | By |
|---|---|---|---|
| ops | Apple Developer Program enrollment and an App Store Connect app record for `com.geauxhack.app` | PR 1.5 (APNs key), PR 1.6 (TestFlight) | 2026-12-01 |
| ops | Google Play Console personal account (identity verified) and a Play app record for `com.geauxhack.app` | PR 1.6 | 2026-12-01 |
| ops | **12 or more testers** with Google account emails who will stay opted in to Closed Testing for 14 days in a row | Google Play production access; the clock must start by 12-15 | 2026-12-10 |
| ops | Firebase project with an Android app `com.geauxhack.app`; APNs auth key (.p8) uploaded to Firebase | PR 1.5 push | 2026-12-05 |
| ops | Privacy policy text, to be published at `https://app.geauxhack.org/privacy` | Both stores require a privacy policy URL | 2026-12-08 |
| infra | Host the PWA at `app.geauxhack.org`, with `/api/*` and `/api/v1/ws` routed to the Rust server on the **same origin** | Production PWA; same-origin cookies for auth | before the January mock run |
| infra | Serve `/.well-known/apple-app-site-association` and `/.well-known/assetlinks.json` with content type `application/json` and no redirects. Josh's agent supplies the file contents. | PR 1.7 deep links | when hosting exists |
| infra | Serve `sw.js` and `index.html` with `Cache-Control: no-cache`; hashed assets cached as `immutable` | Service worker updates propagate to users | when hosting exists |
| infra | CDN bucket and URL for OTA bundles plus `manifest.json`; a CI secret allowed to upload to it | PR 1.8 production setup | before the January mock run |
| backend | **Schedule endpoint** returning `EventInfo` and `ScheduleItem[]` as defined in `apps/client/src/schedule/types.ts`. Times are ISO 8601 with offset, and an IANA `timeZone` comes with the event. Tell me if you need a different shape. Until then the client uses bundled sample data (`schedule/sample.ts`). | Swapping sample data for real calls | not blocking |
| backend | **Current-user profile** with at least `firstName` and `team`. The client keeps those two in a `localStorage` snapshot (`gh:snapshot:v1`) so the first screen paints instantly. | Personalized Home | not blocking |
| ops | **Real event dates.** The sample data uses Feb 20–21, 2027 as a placeholder. | Countdown and schedule | before registration opens |
| design / art | SVG illustrations of Beau and the six Vespers (plus simple versions for small sizes), District icons, the Elysiu/Abyssus portal key art, and the app icon | PR 1.2 visuals, PR 1.4 icon and splash | 2026-12-05 |
| backend | Application API: draft, submit, status, decision, confirmation deadline, waitlist release | PR 1.9 | not blocking |
| backend | **Storyline features not in the architecture spec:** picking Team Beau or Team Parkeaux, point QR codes and side quests (a signed points ledger, scannable offline, like entitlements), a live team leaderboard over WebSocket, and the ending reveal decided by final points | Main event-app features | needs an owner |
| backend | **Team pick endpoint with a 52% cap.** It locks the team row in the same transaction that inserts the pick, and rejects (409) a pick that would join a team already above 52%. It also publishes the current split over WebSocket so the picker's lock state updates live. Picks are final. Rules are in `docs/design.md`, "Picking a side". | Team picker screen | before the January mock run |
| database | Tables for applications and their statuses, team alliance, and a points ledger. (These aren't in the schema ticket.) | PR 1.9 and the story features | needs an owner |
| auth | Which secure-storage plugin is used for the native token, so PR 1.4 doesn't add a conflicting one | PR 1.4 | not blocking |

## Done for others
| For | What was done | Link |
|---|---|---|
