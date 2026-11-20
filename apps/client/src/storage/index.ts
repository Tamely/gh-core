// One storage interface for the whole app: SQLite in the native apps,
// IndexedDB on the web. The backend is loaded on demand so it never delays
// the first paint, and each build only downloads the backend it uses.
import { isNative } from "../platform";

export interface KeyValueStore {
  get<T>(key: string): Promise<T | undefined>;
  set<T>(key: string, value: T): Promise<void>;
  del(key: string): Promise<void>;
}

let store: Promise<KeyValueStore> | undefined;

export function getStore(): Promise<KeyValueStore> {
  store ??= isNative()
    ? import("./native").then((module) => module.nativeStore)
    : import("./web").then((module) => module.webStore);
  return store;
}
