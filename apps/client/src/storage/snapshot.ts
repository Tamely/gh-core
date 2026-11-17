// A tiny copy of the user's state in localStorage, read synchronously so the
// first screen can paint before IndexedDB or SQLite has opened. The full data
// lives in the KeyValueStore; this is only what the first paint needs.

export type Team = "beau" | "parkeaux";

export type Snapshot = {
  v: 1;
  firstName?: string;
  team?: Team;
  savedAt: string;
};

const KEY = "gh:snapshot:v1";

export function readSnapshot(): Snapshot | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const parsed = JSON.parse(raw) as Partial<Snapshot>;
    return parsed.v === 1 ? (parsed as Snapshot) : null;
  } catch {
    return null;
  }
}

export function writeSnapshot(data: Omit<Snapshot, "v" | "savedAt">): void {
  try {
    const snapshot: Snapshot = {
      ...data,
      v: 1,
      savedAt: new Date().toISOString(),
    };
    localStorage.setItem(KEY, JSON.stringify(snapshot));
  } catch {
    // Storage can be full or blocked (private browsing). The app still works without it.
  }
}
