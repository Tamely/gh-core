import { Match, Switch, createSignal } from "solid-js";
import { NOTIFY_ENDPOINT } from "./content";

type State = "idle" | "sending" | "done" | "error";

/** "Tell me when registration opens": one email field, like HackMIT's page between events. */
export function NotifyForm() {
  const [state, setState] = createSignal<State>("idle");

  const submit = async (event: SubmitEvent) => {
    event.preventDefault();
    const form = event.currentTarget as HTMLFormElement;
    const email = new FormData(form).get("email");
    setState("sending");
    try {
      const response = await fetch(NOTIFY_ENDPOINT, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ email }),
      });
      setState(response.ok ? "done" : "error");
    } catch {
      setState("error");
    }
  };

  return (
    <Switch>
      <Match when={state() === "done"}>
        <p role="status" class="text-[17px] font-semibold text-gold">
          You’re on the list. We’ll email you when registration opens.
        </p>
      </Match>
      <Match when={state() !== "done"}>
        <form
          id="notify"
          onSubmit={submit}
          class="flex w-full max-w-md flex-col gap-3 sm:flex-row"
        >
          <label class="sr-only" for="notify-email">
            Email address
          </label>
          <input
            id="notify-email"
            name="email"
            type="email"
            required
            autocomplete="email"
            placeholder="you@school.edu"
            class="min-w-0 grow rounded-full border border-white/25 bg-white/5 px-5 py-3 text-[16px] text-white placeholder:text-white/40 focus:border-gold focus:outline-none"
          />
          <button
            type="submit"
            disabled={state() === "sending"}
            class="press rounded-full bg-gold px-6 py-3 text-[15px] font-extrabold text-ink disabled:opacity-60"
          >
            {state() === "sending" ? "Sending…" : "Notify me"}
          </button>
        </form>
        <p
          role="status"
          class={`mt-3 text-[13px] ${state() === "error" ? "text-gold" : "text-ink-inverse/50"}`}
        >
          {state() === "error"
            ? "That didn’t go through. Try again in a bit."
            : "We’ll only email you about Geaux Hack."}
        </p>
      </Match>
    </Switch>
  );
}
