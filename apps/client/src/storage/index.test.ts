import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("./native", () => ({ nativeStore: { name: "native" } }));
vi.mock("./web", () => ({ webStore: { name: "web" } }));

afterEach(() => {
  delete (globalThis as { Capacitor?: unknown }).Capacitor;
  vi.resetModules();
});

describe("getStore", () => {
  it("uses IndexedDB in a browser", async () => {
    const { getStore } = await import("./index");
    expect(await getStore()).toEqual({ name: "web" });
  });

  it("uses SQLite inside the native shell", async () => {
    (globalThis as { Capacitor?: unknown }).Capacitor = {
      isNativePlatform: () => true,
      getPlatform: () => "ios",
    };
    const { getStore } = await import("./index");
    expect(await getStore()).toEqual({ name: "native" });
  });
});
