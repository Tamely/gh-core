// The native session token in the iOS Keychain / Android Keystore. Loaded only
// on native (see index.ts), so none of this reaches the web bundle.
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
