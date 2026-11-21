// The app's one auth instance. On native the Keychain/Keystore code is loaded on
// first use, so the web build never downloads it.
import { isNative } from "../platform";
import { getStore } from "../storage";
import { createAuth, type TokenStore } from "./session";

export type { Auth, Profile, Session } from "./session";

const secure = () => import("./secure").then((module) => module.nativeTokens);
const nativeTokens: TokenStore = {
  get: async () => (await secure()).get(),
  set: async (token) => (await secure()).set(token),
  clear: async () => (await secure()).clear(),
};

export const auth = createAuth({
  store: getStore,
  tokens: isNative() ? nativeTokens : null,
  fetch: (input, init) => fetch(input, init),
  now: () => Math.floor(Date.now() / 1000),
});
