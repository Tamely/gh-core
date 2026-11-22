// The client side of auth (auth PR 5): keeps a session across launches so the
// app can start offline. The server still checks every request.
//
// Web: the session is the HttpOnly cookie, which JavaScript can't read.
// Native: the app runs at capacitor://localhost, so API calls go to `base` through
// native HTTP (no cookies, no CORS). The token comes in the response body and
// lives in the Keychain / Keystore (native.ts).
// Both: the profile (id, roles, exp, roomId; nothing secret) is kept in the
// app's KeyValueStore, so startup can tell signed in from expired with no
// network call.
import type { KeyValueStore } from "../storage";

export type Profile = {
  id: string;
  roles: string[];
  /** Unix seconds. */
  exp: number;
  /** Judges only. */
  roomId?: string;
};

/**
 * "expired": the session ran out while offline. Show cached content read-only
 * and ask to sign in again once online.
 */
export type Session =
  | { status: "signed-out" }
  | { status: "signed-in" | "expired"; profile: Profile };

export interface TokenStore {
  get(): Promise<string | null>;
  set(token: string): Promise<void>;
  clear(): Promise<void>;
}

export type Deps = {
  store: () => Promise<KeyValueStore>;
  /** Native only. `null` on the web, where the cookie is the session. */
  tokens: TokenStore | null;
  fetch: typeof fetch;
  /** The API's origin: "" on the web (same origin), e.g. "https://app.geauxhack.org" on native. */
  base: string;
  /** Unix seconds. */
  now: () => number;
};

const PROFILE = "auth:profile";

/** The token's `exp`, read without checking the signature (the server does that). */
function expOf(token: string): number | undefined {
  try {
    const payload = token.split(".")[1] ?? "";
    const json = atob(payload.replace(/-/g, "+").replace(/_/g, "/"));
    return (JSON.parse(json) as { exp?: number }).exp;
  } catch {
    return undefined;
  }
}

export function createAuth({ store, tokens, fetch, base, now }: Deps) {
  const read = async () => (await store()).get<Profile>(PROFILE);
  const keep = async ({
    id,
    roles,
    exp,
    roomId,
  }: Profile): Promise<Session> => {
    const profile: Profile = { id, roles, exp, ...(roomId ? { roomId } : {}) };
    await (await store()).set(PROFILE, profile);
    return { status: "signed-in", profile };
  };
  const forget = async (): Promise<Session> => {
    await (await store()).del(PROFILE);
    await tokens?.clear();
    return { status: "signed-out" };
  };

  /** No network: what the last session left on this device. */
  async function startup(): Promise<Session> {
    const profile = await read();
    // A native profile without its token (e.g. the Keychain was wiped) is no session.
    if (!profile || (tokens && !(await tokens.get())))
      return { status: "signed-out" };
    return { status: profile.exp > now() ? "signed-in" : "expired", profile };
  }

  /** An API call with the session attached. A 401 means the session is over. */
  async function api(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    const token = await tokens?.get();
    if (token) headers.set("authorization", `Bearer ${token}`);
    const res = await fetch(base + path, {
      ...init,
      headers,
      credentials: "same-origin",
    });
    // Native sliding renewal; on the web the server re-sets the cookie itself.
    const renewed = res.headers.get("x-session-token");
    const profile = renewed && tokens ? await read() : undefined;
    if (renewed && tokens && profile) {
      await tokens.set(renewed);
      await keep({ ...profile, exp: expOf(renewed) ?? profile.exp });
    }
    if (res.status === 401) await forget();
    return res;
  }

  return {
    startup,
    api,

    /**
     * Code sign-in (`/api/v1/auth/code/verify`), the OAuth finish or the judge
     * exchange (`/api/v1/judge/exchange`). A rejection leaves any current
     * session as it was, so a bad placard doesn't sign anyone out.
     */
    async signIn(path: string, body: unknown): Promise<Session | Response> {
      const res = await fetch(base + path, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
        credentials: "same-origin",
      });
      if (!res.ok) return res;
      const signedIn = (await res.json()) as Profile & { token?: string };
      if (tokens) {
        // The server sends the token in the body to anything but our web origin.
        if (!signedIn.token)
          throw new Error("native sign-in answered without a token");
        await tokens.set(signedIn.token);
      }
      return keep(signedIn);
    },

    /**
     * Online: the server's view of the session (`/me`), which also picks up
     * web cookie renewals. Offline: same as startup.
     */
    async refresh(): Promise<Session> {
      let res: Response;
      try {
        res = await api("/api/v1/me");
      } catch {
        return startup();
      }
      if (res.ok) return keep((await res.json()) as Profile);
      return res.status === 401 ? { status: "signed-out" } : startup();
    },

    /** Revokes the session on the server if it can be reached, and always forgets it here. */
    async signOut(): Promise<Session> {
      try {
        await api("/api/v1/auth/logout", { method: "POST" });
      } catch {
        // Offline: the token is gone from this device either way.
      }
      return forget();
    },
  };
}

export type Auth = ReturnType<typeof createAuth>;
