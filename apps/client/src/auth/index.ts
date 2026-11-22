// The app's one auth instance. On native the Keychain / native HTTP code is
// loaded on first use, so the web build never downloads it.
import { isNative } from "../platform";
import { getStore } from "../storage";
import { createAuth, type TokenStore } from "./session";

export type { Auth, Profile, Session } from "./session";

const native = () => import("./native");
const nativeTokens: TokenStore = {
  get: async () => (await native()).nativeTokens.get(),
  set: async (token) => (await native()).nativeTokens.set(token),
  clear: async () => (await native()).nativeTokens.clear(),
};

/** Native builds call the API here; set VITE_API_ORIGIN to try a dev server. */
export const API_ORIGIN: string =
  import.meta.env.VITE_API_ORIGIN ?? "https://app.geauxhack.org";

export const auth = createAuth({
  store: getStore,
  now: () => Math.floor(Date.now() / 1000),
  ...(isNative()
    ? {
        tokens: nativeTokens,
        fetch: async (input, init) => (await native()).nativeFetch(input, init),
        base: API_ORIGIN,
      }
    : { tokens: null, fetch: (input, init) => fetch(input, init), base: "" }),
});
