import { createStore, del, get, set } from "idb-keyval";
import type { KeyValueStore } from "./index";

const db = createStore("gh-core", "kv");

export const webStore: KeyValueStore = {
  get: (key) => get(key, db),
  set: (key, value) => set(key, value, db),
  del: (key) => del(key, db),
};
