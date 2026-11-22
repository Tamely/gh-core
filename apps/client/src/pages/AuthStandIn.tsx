// STAND-IN, not a real screen: exercises every auth call (PR 5) until Josh's
// sign-in, judge and profile screens use them. Only in dev builds, or a build
// with VITE_AUTH_STANDIN=1 (see App.tsx). Delete once the real screens exist.
import { createSignal, For, onMount } from "solid-js";
import { API_ORIGIN, auth, type Session } from "../auth";
import { isNative, platform } from "../platform";

const field =
  "w-full rounded-xl border border-black/15 bg-transparent px-3 py-2 dark:border-white/20";
const button =
  "rounded-xl bg-beau px-3 py-2 font-bold text-white disabled:opacity-50";

export function AuthStandIn() {
  const [session, setSession] = createSignal<Session>();
  const [log, setLog] = createSignal<string[]>([]);
  const [email, setEmail] = createSignal("");
  const [code, setCode] = createSignal("");
  const [placard, setPlacard] = createSignal("");

  const say = (line: string) => setLog((l) => [line, ...l].slice(0, 20));
  /** Runs one auth call, shows the session it leaves, and logs errors instead of throwing. */
  const run = (label: string, call: () => Promise<Session | Response>) =>
    void call().then(
      async (out) => {
        if (out instanceof Response) say(`${label}: HTTP ${out.status}`);
        else say(`${label}: ${out.status}`);
        setSession(await auth.startup());
      },
      (error: unknown) => say(`${label}: ${String(error)}`),
    );
  const post = (path: string, body: unknown) =>
    auth.api(path, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    });

  onMount(() => run("startup (offline)", auth.startup));

  return (
    <div class="flex flex-col gap-4">
      <h1 class="text-[30px] font-extrabold tracking-[-0.03em]">
        Auth stand-in
      </h1>
      <p class="text-sm text-muted dark:text-muted-inverse">
        {platform()} · API {isNative() ? API_ORIGIN : "same origin"}
      </p>
      <pre class="overflow-x-auto rounded-xl bg-black/5 p-3 text-xs dark:bg-white/10">
        {JSON.stringify(session() ?? "…", null, 2)}
      </pre>

      <div class="flex flex-wrap gap-2">
        <button class={button} onClick={() => run("startup", auth.startup)}>
          Startup (no network)
        </button>
        <button class={button} onClick={() => run("refresh", auth.refresh)}>
          Refresh (/me)
        </button>
        <button
          class={button}
          onClick={() => run("GET /me", () => auth.api("/api/v1/me"))}
        >
          Call /me
        </button>
        <button class={button} onClick={() => run("sign out", auth.signOut)}>
          Sign out
        </button>
      </div>

      <h2 class="text-lg font-bold">Email code</h2>
      <input
        class={field}
        type="email"
        placeholder="you@lsu.edu"
        value={email()}
        onInput={(e) => setEmail(e.currentTarget.value)}
      />
      <button
        class={button}
        onClick={() =>
          run("send code", () => post("/api/v1/auth/code", { email: email() }))
        }
      >
        Send code
      </button>
      <input
        class={field}
        inputmode="numeric"
        autocomplete="one-time-code"
        placeholder="123456"
        value={code()}
        onInput={(e) => setCode(e.currentTarget.value)}
      />
      <button
        class={button}
        onClick={() =>
          run("verify", () =>
            auth.signIn("/api/v1/auth/code/verify", {
              email: email(),
              code: code(),
            }),
          )
        }
      >
        Sign in
      </button>

      <h2 class="text-lg font-bold">Judge placard</h2>
      <input
        class={field}
        placeholder="Placard URL or token"
        value={placard()}
        onInput={(e) => setPlacard(e.currentTarget.value)}
      />
      <button
        class={button}
        onClick={() =>
          run("judge exchange", () =>
            auth.signIn("/api/v1/judge/exchange", {
              // Accepts the whole printed URL (…/judge/<token>) or just the token.
              token: placard().trim().split("/").pop(),
            }),
          )
        }
      >
        Exchange placard
      </button>

      <h2 class="text-lg font-bold">Log</h2>
      <ol class="flex flex-col gap-1 text-xs">
        {/* Newest first. */}
        <For each={log()}>{(line) => <li>{line}</li>}</For>
      </ol>
    </div>
  );
}
