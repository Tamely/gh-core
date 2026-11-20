/// <reference lib="webworker" />
// Service worker for the web app (the native apps don't use it: their files
// are already on the device). Built by vite-plugin-pwa with injectManifest.
//
// - Precaches the whole app shell at install: HTML, JS, CSS, the Allo font
//   and icons. After the first visit the app opens with no network at all.
// - Serves index.html for every in-app page, so deep links like /battle
//   also work offline.
// - Leaves /api/ alone; data caching comes with the API.
// - Takes over as soon as it installs, so a new deploy applies on next load.
import { clientsClaim } from "workbox-core";
import {
  cleanupOutdatedCaches,
  createHandlerBoundToURL,
  precacheAndRoute,
} from "workbox-precaching";
import { NavigationRoute, registerRoute } from "workbox-routing";

declare const self: ServiceWorkerGlobalScope & {
  __WB_MANIFEST: (string | { url: string; revision: string | null })[];
};

precacheAndRoute(self.__WB_MANIFEST);
cleanupOutdatedCaches();

registerRoute(
  new NavigationRoute(createHandlerBoundToURL("/index.html"), {
    denylist: [/^\/api\//],
  }),
);

void self.skipWaiting();
clientsClaim();
