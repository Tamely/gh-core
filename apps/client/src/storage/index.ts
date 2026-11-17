// One storage interface for the whole app. The web build uses IndexedDB;
// PR 1.4 adds a native SQLite backend behind the same interface.
// The backend is loaded on demand so it never delays the first paint.

export interface KeyValueStore {
  get<T>(key: string): Promise<T | undefined>;
  set<T>(key: string, value: T): Promise<void>;
  del(key: string): Promise<void>;
}

let store: Promise<KeyValueStore> | undefined;

export function getStore(): Promise<KeyValueStore> {
  store ??= import("./web").then((module) => module.webStore);
  return store;
}
