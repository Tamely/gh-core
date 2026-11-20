# Native apps (iOS and Android)

The iOS and Android apps are Capacitor 8 projects in `apps/client/ios` and `apps/client/android`. They wrap the same build as the PWA (`apps/client/dist`), so the app works offline from the first launch: everything it needs is inside the app.

- **App ID:** `com.geauxhack.app` (can't change after the first store upload)
- **Config:** `apps/client/capacitor.config.ts`
- **Plugins:** `@capacitor-community/sqlite` (storage), `@capacitor/haptics`. Scanning and push arrive in PR 1.5.
- **iOS** uses Swift Package Manager, so there's no CocoaPods step.
- **Android** targets SDK 36 and compiles with Java 21.

## Running the apps

From the repo root:

```bash
pnpm --filter @gh-core/client ios
```

```bash
pnpm --filter @gh-core/client android
```

Each builds the web app, copies it into the native project (`cap sync`), and opens Xcode or Android Studio. Run on a device or simulator from there.

You need Xcode (current version, on the Mac) for iOS, and Android Studio (or the Android SDK plus Java 21) for Android. CI builds an Android debug APK on every PR; iOS is only built locally.

After changing anything in `apps/client/src`, run `pnpm --filter @gh-core/client cap:sync` (or the `ios` / `android` command again) so the native project gets the new build.

## How the web code talks to native

- **Detecting native:** `src/platform.ts` checks `window.Capacitor`, which the shell sets before our scripts run. The web bundle never imports `@capacitor/core`.
- **Plugins are loaded on demand, only on native.** `import("@capacitor/haptics")` and the SQLite module are dynamic imports behind `isNative()`, so they're separate chunks the web never downloads. Keep it that way: a static import of any Capacitor package would put it in the 35KB initial shell.
- **Storage:** `getStore()` in `src/storage/index.ts` returns SQLite on native (`src/storage/native.ts`, one `kv` table) and IndexedDB on the web. Same interface either way.
- **Haptics:** `src/haptics.ts`. A light tap when switching tabs; success/error for scans in PR 1.5. No-ops on the web.

## Measuring offline startup (the under-100ms target)

**Target:** median of 10 cold starts under 100ms, in Airplane Mode, on a physical iPhone and a physical mid-range Android phone. It's measured from the web view's `performance.timeOrigin` to the `shell:rendered` mark (set right after the first render in `src/index.tsx`). It doesn't include the native launch before the web view starts.

Each launch records its time on the device (`src/startup.ts`), so you can do all 10 and read the results once:

1. Install a **release** build on the phone (debug builds are slower).
2. Connect the phone and open the inspector: Safari > Develop > *your iPhone* > Geaux Hack, or `chrome://inspect` for Android.
3. In the console, run `ghClearStartups()` to start fresh.
4. Turn on **Airplane Mode**. Then, 10 times: swipe the app away completely and open it again.
5. Reconnect the inspector. The latest console line reads like `[startup] ios: shell rendered in 41ms. Last 10 launches: … Median 43ms.`
6. Put the device model, the median and the slowest launch in the PR.

## App icon and splash screen

`apps/client/assets/` holds the source images: `icon-only.png`, `icon-foreground.png` and `icon-background.png` (Android adaptive icon), `splash.png` and `splash-dark.png`.

**They're placeholders** (a gold crown on Beau purple), drawn by `scripts/make-placeholder-assets.mjs`. When the real artwork is ready, replace the PNGs (same names; icons 1024×1024, splash 2732×2732) and run:

```bash
pnpm --filter @gh-core/client assets
```

## Not decided yet

- **Native Liquid Glass tab bar on iOS 26.** The tab bar is the CSS version everywhere for now. Using Apple's real material would mean a native tab bar over the web view (or a small custom plugin). Worth revisiting after PR 1.6, once the app is on TestFlight and we can see how the CSS version looks on a real iPhone.
