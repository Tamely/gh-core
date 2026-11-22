import { describe, expect, it, vi } from "vitest";

// A plain function, not vi.fn: vitest reports a vi.fn's rejected promise as a
// failure even when the caller handles it.
const sent: unknown[] = [];
let reply: () => Promise<unknown> = async () => undefined;
vi.mock("@capacitor/core", () => ({
  CapacitorHttp: {
    request: async (options: unknown) => {
      sent.push(options);
      return reply();
    },
  },
}));
vi.mock("@aparajita/capacitor-secure-storage", () => ({
  KeychainAccess: { afterFirstUnlockThisDeviceOnly: 3 },
  SecureStorage: { setSynchronize: async () => {} },
}));

const { nativeFetch } = await import("./native");

const answer = (status: number, data: unknown, headers = {}) => {
  reply = async () => ({ status, data, headers, url: "" });
};

describe("nativeFetch", () => {
  it("sends JSON as data and hands back a Response the session code can read", async () => {
    answer(200, { id: "u1", token: "t" }, { "x-session-token": "renewed" });
    const res = await nativeFetch("https://api.test/api/v1/auth/code/verify", {
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: "Bearer x",
      },
      body: JSON.stringify({ email: "a@b.c", code: "123456" }),
    });
    expect(sent.at(-1)).toEqual({
      url: "https://api.test/api/v1/auth/code/verify",
      method: "POST",
      headers: {
        "content-type": "application/json",
        authorization: "Bearer x",
      },
      data: { email: "a@b.c", code: "123456" },
    });
    expect(res.status).toBe(200);
    expect(res.headers.get("x-session-token")).toBe("renewed");
    expect(await res.json()).toEqual({ id: "u1", token: "t" });
  });

  it("handles empty answers (logout's 204) and plain-text bodies", async () => {
    answer(204, "");
    expect(
      (await nativeFetch("https://api.test/api/v1/auth/logout")).status,
    ).toBe(204);

    answer(401, "");
    const res = await nativeFetch("https://api.test/api/v1/me");
    expect(res.status).toBe(401);
    expect(await res.text()).toBe("");
  });

  it("rejects like fetch when offline, so the session treats it as no network", async () => {
    const offline = new TypeError("offline");
    reply = () => Promise.reject(offline);
    await expect(nativeFetch("https://api.test/api/v1/me")).rejects.toBe(
      offline,
    );
  });
});
