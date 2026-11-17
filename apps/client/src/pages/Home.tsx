import { For, Show, createSignal, onCleanup } from "solid-js";
import { Link } from "../router";
import { sampleEvent, sampleSchedule } from "../schedule/sample";
import {
  formatTime,
  hackingCountdown,
  kindColor,
  kindLabel,
  upNext,
} from "../schedule/schedule";
import { readSnapshot } from "../storage/snapshot";
import { CrownIcon, PersonIcon } from "../ui/icons";

const SOON_MS = 15 * 60 * 1000;

export function Home() {
  const snapshot = readSnapshot();
  const [now, setNow] = createSignal(new Date());
  const timer = setInterval(() => setNow(new Date()), 1000);
  onCleanup(() => clearInterval(timer));

  const countdown = () => hackingCountdown(sampleEvent, now());
  // Re-evaluated every minute is plenty for "up next".
  const minute = () => Math.floor(now().getTime() / 60000);
  const next = () => upNext(sampleSchedule, new Date(minute() * 60000), 4);
  const startsSoon = (iso: string) => {
    const delta = Date.parse(iso) - minute() * 60000;
    return delta > 0 && delta <= SOON_MS;
  };

  return (
    <div class="flex flex-col gap-10">
      <header class="flex items-center justify-between">
        <div class="flex flex-col gap-0.5">
          <p class="text-[13px] font-semibold tracking-[0.1em] text-muted uppercase dark:text-muted-inverse">
            Geaux Hack 2027
          </p>
          <h1 class="text-[30px] font-extrabold tracking-[-0.03em]">
            Hey, {snapshot?.firstName ?? "hacker"}
          </h1>
        </div>
        <Link
          href="/profile"
          aria-label="Profile and pass"
          class="press flex size-12 items-center justify-center rounded-full bg-beau text-gold"
        >
          <PersonIcon />
        </Link>
      </header>

      {/* The countdown is set as type on the page, not boxed in a card. */}
      <Show when={countdown().value}>
        <section class="relative" aria-label={countdown().label}>
          <CrownIcon
            size={150}
            class="pointer-events-none absolute -top-9 -right-6 text-gold opacity-25 dark:opacity-20"
          />
          <p class="relative text-[15px] font-semibold text-muted dark:text-muted-inverse">
            {countdown().label}
          </p>
          <p class="relative text-[52px] leading-none font-extrabold tracking-[-0.04em] text-beau tabular-nums dark:text-beau-glow">
            {countdown().value}
          </p>
        </section>
      </Show>

      <section class="flex flex-col gap-5">
        <div class="flex items-baseline justify-between">
          <h2 class="text-xl font-bold tracking-[-0.01em]">Up next</h2>
          <Link
            href="/schedule"
            class="text-sm font-bold text-beau no-underline dark:text-beau-glow"
          >
            Full schedule
          </Link>
        </div>

        {/* A timeline: time, then a dot on a line, then the details. */}
        <ol class="flex flex-col">
          <For each={next()}>
            {(item, index) => (
              <li class="grid grid-cols-[64px_20px_1fr] gap-x-3">
                <span class="pt-0.5 text-sm font-semibold text-muted tabular-nums dark:text-muted-inverse">
                  {formatTime(item.startsAt, sampleEvent.timeZone)}
                </span>
                <span class="relative flex justify-center" aria-hidden="true">
                  <span
                    class={`relative z-[1] mt-1.5 size-3 rounded-full ${
                      startsSoon(item.startsAt)
                        ? "pulse bg-gold"
                        : index() === 0
                          ? "bg-beau dark:bg-beau-glow"
                          : "border-2 border-beau/40 bg-paper dark:border-beau-glow/50 dark:bg-abyss"
                    }`}
                  />
                  <Show when={index() < next().length - 1}>
                    <span class="absolute top-4 bottom-0 w-0.5 bg-beau/15 dark:bg-beau-glow/20" />
                  </Show>
                </span>
                <div class="flex flex-col gap-0.5 pb-7">
                  <div class="flex items-baseline justify-between gap-3">
                    <span class="text-[19px] leading-tight font-bold">
                      {item.title}
                    </span>
                    <Show when={item.points}>
                      <span class="text-sm font-extrabold text-beau tabular-nums dark:text-beau-glow">
                        +{item.points}
                      </span>
                    </Show>
                  </div>
                  <span class="text-[13px]">
                    <span class={`font-bold ${kindColor[item.kind]}`}>
                      {kindLabel[item.kind]}
                    </span>
                    <span class="text-muted dark:text-muted-inverse">
                      {" "}
                      in {item.location}
                    </span>
                  </span>
                </div>
              </li>
            )}
          </For>
        </ol>
      </section>
    </div>
  );
}
