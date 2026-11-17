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

export function Home() {
  const snapshot = readSnapshot();
  const [now, setNow] = createSignal(new Date());
  const timer = setInterval(() => setNow(new Date()), 1000);
  onCleanup(() => clearInterval(timer));

  const countdown = () => hackingCountdown(sampleEvent, now());
  // Re-evaluated every minute is plenty for "up next".
  const minute = () => Math.floor(now().getTime() / 60000);
  const next = () => upNext(sampleSchedule, new Date(minute() * 60000), 4);

  return (
    <div class="flex flex-col gap-4">
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

      <Show when={countdown().value}>
        <section
          aria-live="off"
          class="flex items-center justify-between rounded-[28px] bg-beau p-5 text-white"
        >
          <div class="flex flex-col gap-0.5">
            <p class="text-[13px] font-semibold text-beau-soft">
              {countdown().label}
            </p>
            <p class="text-[34px] font-extrabold tracking-[-0.02em] tabular-nums">
              {countdown().value}
            </p>
          </div>
          <CrownIcon size={40} class="text-gold" />
        </section>
      </Show>

      <div class="flex items-baseline justify-between px-1 pt-1">
        <h2 class="text-xl font-bold tracking-[-0.01em]">Up next</h2>
        <Link
          href="/schedule"
          class="text-sm font-bold text-beau no-underline dark:text-beau-glow"
        >
          Full schedule
        </Link>
      </div>

      <ul class="flex flex-col gap-4">
        <For each={next()}>
          {(item) => (
            <li class="lift flex flex-col gap-2 rounded-[24px] bg-surface p-[18px] dark:bg-abyss-surface">
              <div class="flex items-baseline justify-between">
                <span class={`text-[13px] font-bold ${kindColor[item.kind]}`}>
                  {kindLabel[item.kind]}
                </span>
                <span class="text-sm font-semibold text-muted tabular-nums dark:text-muted-inverse">
                  {formatTime(item.startsAt, sampleEvent.timeZone)}
                </span>
              </div>
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
              <span class="text-[13px] text-muted dark:text-muted-inverse">
                {item.location}
              </span>
            </li>
          )}
        </For>
      </ul>
    </div>
  );
}
