// SQLite backend for the native apps. Unlike IndexedDB in a web view, the OS
// doesn't evict it under storage pressure. Loaded only on native (see index.ts),
// so none of this reaches the web bundle.
import {
  CapacitorSQLite,
  SQLiteConnection,
  type SQLiteDBConnection,
} from "@capacitor-community/sqlite";
import type { KeyValueStore } from "./index";

const DB_NAME = "gh-core";
const sqlite = new SQLiteConnection(CapacitorSQLite);

async function open(): Promise<SQLiteDBConnection> {
  // After a web view reload the native side may still hold the connection.
  const consistent = (await sqlite.checkConnectionsConsistency()).result;
  const exists = (await sqlite.isConnection(DB_NAME, false)).result;
  const db =
    consistent && exists
      ? await sqlite.retrieveConnection(DB_NAME, false)
      : await sqlite.createConnection(
          DB_NAME,
          false,
          "no-encryption",
          1,
          false,
        );
  await db.open();
  await db.execute(
    "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL)",
  );
  return db;
}

let connection: Promise<SQLiteDBConnection> | undefined;
const db = () => (connection ??= open());

export const nativeStore: KeyValueStore = {
  async get<T>(key: string) {
    const result = await (
      await db()
    ).query("SELECT value FROM kv WHERE key = ?", [key]);
    const row = result.values?.[0] as { value: string } | undefined;
    return row ? (JSON.parse(row.value) as T) : undefined;
  },
  async set<T>(key: string, value: T) {
    await (
      await db()
    ).run(
      "INSERT INTO kv (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
      [key, JSON.stringify(value)],
    );
  },
  async del(key: string) {
    await (await db()).run("DELETE FROM kv WHERE key = ?", [key]);
  },
};
