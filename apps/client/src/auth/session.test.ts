import { describe, expect, it } from "vitest";
import type { KeyValueStore } from "../storage";
import { createAuth, type TokenStore } from "./session";

const NOW = 1_800_000_000;
const DAY = 86_400;
const profile = { id: "u1", roles: ["HACKER"], exp: NOW + 14 * DAY };

function memoryStore(): KeyValueStore {
  const data = new Map<string, unknown>();
  return {
    get: async <T>(key: string) => data.get(key) as T | undefined,
    set: async (key, value) => void data.set(key, value),
    del: async (key) => void data.delete(key),
  };
}

function memoryTokens(): TokenStore & { value: string | null } {
  return {
    value: null,
    async get() {
      return this.value;
    },
    async set(token) {
      this.value = token;
    },
    async clear() {
      this.value = null;
    },
  };
}

/** A JWT-shaped token; only the payload's exp matters here. */
const jwt = (exp: number) =>
  `h.${btoa(JSON.stringify({ exp })).replace(/=+$/, "")}.s`;

type Call = { path: string; init: RequestInit };

const API = "https://api.test";

function setup(native: boolean, answer: (path: string) => Response | Error) {
  const calls: Call[] = [];
  let now = NOW;
  const store = memoryStore();
  const tokens = memoryTokens();
  const auth = createAuth({
    store: async () => store,
    tokens: native ? tokens : null,
    base: native ? API : "",
    now: () => now,
    fetch: async (input, init = {}) => {
      // Tests name paths; native calls must go to the API's origin.
      const url = String(input);
      if (native !== url.startsWith(API))
        throw new Error(`wrong origin: ${url}`);
      const path = native ? url.slice(API.length) : url;
      calls.push({ path, init });
      const res = answer(path);
      if (res instanceof Error) throw res;
      return res;
    },
  });
  const last = () => calls[calls.length - 1] as Call;
  return { auth, calls, last, tokens, at: (t: number) => (now = t) };
}

const json = (body: unknown, headers: Record<string, string> = {}) =>
  new Response(JSON.stringify(body), {
    headers: { "content-type": "application/json", ...headers },
  });
const status = (code: number) => new Response(null, { status: code });
const offline = () => new TypeError("Failed to fetch");

describe("auth session", () => {
  it("starts offline: signed out, signed in, then expired, with no network calls", async () => {
    const { auth, calls, at } = setup(false, (path) =>
      path === "/api/v1/auth/code/verify" ? json(profile) : offline(),
    );
    expect(await auth.startup()).toEqual({ status: "signed-out" });

    await auth.signIn("/api/v1/auth/code/verify", {
      email: "a@b.c",
      code: "123456",
    });
    const before = calls.length;
    expect(await auth.startup()).toEqual({ status: "signed-in", profile });
    at(profile.exp);
    expect(await auth.startup()).toEqual({ status: "expired", profile });
    expect(calls.length).toBe(before);
  });

  it("web: relies on the cookie, never stores or sends a token", async () => {
    const { auth, last } = setup(false, (path) =>
      path === "/api/v1/me"
        ? json(profile)
        : json({ ...profile, token: "should-not-be-here" }),
    );
    await auth.signIn("/api/v1/auth/code/verify", {});
    await auth.api("/api/v1/me");
    const me = last();
    expect(me.init.credentials).toBe("same-origin");
    expect(new Headers(me.init.headers).has("authorization")).toBe(false);
  });

  it("native: keeps the token from the body and sends it as a Bearer header", async () => {
    const token = jwt(profile.exp);
    const { auth, last, tokens } = setup(true, (path) =>
      path === "/api/v1/me" ? json(profile) : json({ ...profile, token }),
    );
    await auth.signIn("/api/v1/auth/code/verify", {});
    expect(tokens.value).toBe(token);
    await auth.api("/api/v1/me");
    expect(new Headers(last().init.headers).get("authorization")).toBe(
      `Bearer ${token}`,
    );

    // Keychain wiped (or restored onto a new phone): the profile alone is no session.
    await tokens.clear();
    expect(await auth.startup()).toEqual({ status: "signed-out" });
  });

  it("native: refuses a sign-in answer without a token", async () => {
    const { auth } = setup(true, () => json(profile));
    await expect(auth.signIn("/api/v1/auth/code/verify", {})).rejects.toThrow(
      "without a token",
    );
    expect(await auth.startup()).toEqual({ status: "signed-out" });
  });

  it("native: a renewed token replaces the old one and moves exp", async () => {
    const later = profile.exp + 7 * DAY;
    const { auth, tokens } = setup(true, (path) =>
      path === "/api/v1/me"
        ? json(profile, { "x-session-token": jwt(later) })
        : json({ ...profile, token: jwt(profile.exp) }),
    );
    await auth.signIn("/api/v1/auth/code/verify", {});
    await auth.api("/api/v1/me");
    expect(tokens.value).toBe(jwt(later));
    expect(await auth.startup()).toMatchObject({ profile: { exp: later } });
  });

  it("judges keep their room; a rejected placard leaves the current session alone", async () => {
    const judge = {
      id: "judge:s1",
      roles: ["JUDGE_ROOM"],
      exp: NOW + DAY,
      roomId: "PFT-1200",
    };
    let placard = true;
    const { auth } = setup(false, () => (placard ? json(judge) : status(401)));
    expect(
      await auth.signIn("/api/v1/judge/exchange", { token: "good" }),
    ).toEqual({
      status: "signed-in",
      profile: judge,
    });
    placard = false;
    const res = await auth.signIn("/api/v1/judge/exchange", {
      token: "forged",
    });
    expect((res as Response).status).toBe(401);
    expect(await auth.startup()).toEqual({
      status: "signed-in",
      profile: judge,
    });
  });

  it("a 401 from the API ends the session here too", async () => {
    let revoked = false;
    const { auth, tokens } = setup(true, () =>
      revoked ? status(401) : json({ ...profile, token: jwt(profile.exp) }),
    );
    await auth.signIn("/api/v1/auth/code/verify", {});
    revoked = true;
    expect((await auth.api("/api/v1/me")).status).toBe(401);
    expect(tokens.value).toBeNull();
    expect(await auth.startup()).toEqual({ status: "signed-out" });
  });

  it("refresh: offline keeps the local view; online takes the server's", async () => {
    let online = false;
    const fresh = { ...profile, exp: profile.exp + DAY };
    const { auth, at } = setup(false, (path) => {
      if (path === "/api/v1/auth/code/verify") return json(profile);
      return online ? json(fresh) : offline();
    });
    await auth.signIn("/api/v1/auth/code/verify", {});
    at(profile.exp + 1);
    expect(await auth.refresh()).toEqual({ status: "expired", profile });
    online = true;
    expect(await auth.refresh()).toEqual({
      status: "signed-in",
      profile: fresh,
    });
  });

  it("refresh: expired offline, then 401 online, means sign in again", async () => {
    let online = false;
    const { auth, at } = setup(false, (path) => {
      if (path === "/api/v1/auth/code/verify") return json(profile);
      return online ? status(401) : offline();
    });
    await auth.signIn("/api/v1/auth/code/verify", {});
    at(profile.exp);
    expect((await auth.refresh()).status).toBe("expired");
    online = true;
    expect(await auth.refresh()).toEqual({ status: "signed-out" });
  });

  it("sign out revokes on the server and forgets locally, even offline", async () => {
    let online = true;
    const { auth, last, tokens } = setup(true, (path) => {
      if (path === "/api/v1/auth/code/verify")
        return json({ ...profile, token: jwt(profile.exp) });
      return online ? status(204) : offline();
    });
    await auth.signIn("/api/v1/auth/code/verify", {});
    expect(await auth.signOut()).toEqual({ status: "signed-out" });
    expect(last()).toMatchObject({
      path: "/api/v1/auth/logout",
      init: { method: "POST" },
    });
    expect(tokens.value).toBeNull();

    await auth.signIn("/api/v1/auth/code/verify", {});
    online = false;
    expect(await auth.signOut()).toEqual({ status: "signed-out" });
    expect(await auth.startup()).toEqual({ status: "signed-out" });
  });
});
