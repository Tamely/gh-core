// Native only (loaded on first use by index.ts, so none of this reaches the web
// bundle): the session token in the iOS Keychain / Android Keystore, and API
// calls over native HTTP.
import { CapacitorHttp } from "@capacitor/core";
import {
  KeychainAccess,
  SecureStorage,
} from "@aparajita/capacitor-secure-storage";
import type { TokenStore } from "./session";

const KEY = "session-token";
// After the first unlock since boot, so the app works while the phone is locked
// in a pocket; this device only, so the token never reaches iCloud or a backup.
const ACCESS = KeychainAccess.afterFirstUnlockThisDeviceOnly;
const ready = SecureStorage.setSynchronize(false);

export const nativeTokens: TokenStore = {
  async get() {
    await ready;
    try {
      return await SecureStorage.getItem(KEY);
    } catch {
      // Unreadable, e.g. restored onto a new phone without its Keystore key: sign in again.
      return null;
    }
  },
  async set(token) {
    await ready;
    await SecureStorage.set(KEY, token, false, false, ACCESS);
  },
  async clear() {
    await ready;
    await SecureStorage.remove(KEY, false);
  },
};

/**
 * `fetch` over the OS's HTTP stack. The web view's own fetch would be a
 * cross-origin call from capacitor://localhost (CORS, and no access to the
 * `x-session-token` header). Sends no Origin, so the server answers with the
 * token in the body. Bodies are JSON (all auth sends); a network error rejects,
 * like fetch.
 */
export async function nativeFetch(
  input: RequestInfo | URL,
  init: RequestInit = {},
): Promise<Response> {
  const res = await CapacitorHttp.request({
    url: String(input),
    method: init.method ?? "GET",
    headers: Object.fromEntries(new Headers(init.headers)),
    data: typeof init.body === "string" ? JSON.parse(init.body) : undefined,
  });
  // JSON answers arrive parsed; anything else as text.
  const body =
    typeof res.data === "string" ? res.data : JSON.stringify(res.data);
  const empty = [204, 205, 304].includes(res.status);
  return new Response(empty ? null : body, {
    status: res.status,
    headers: res.headers,
  });
}
