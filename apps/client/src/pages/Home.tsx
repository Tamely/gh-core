import { For, Match, Show, Switch, createSignal, onCleanup } from "solid-js";
import { Link } from "../router";
import { sampleEvent, sampleSchedule } from "../schedule/sample";
import {
  dayLabel,
  formatTime,
  hackingCountdown,
  kindColor,
  kindLabel,
  splitAtNow,
} from "../schedule/schedule";
import type { ScheduleItem } from "../schedule/types";
import { readSnapshot } from "../storage/snapshot";
import { CrownIcon, PersonIcon } from "../ui/icons";

const SOON_MS = 15 * 60 * 1000;
const tz = sampleEvent.timeZone;

type Row =
  | { kind: "day"; label: string }
  | { kind: "now" }
  | {
      kind: "item";
      item: ScheduleItem;
      tone: "past" | "next" | "later";
      last: boolean;
    };

/** Day labels go in wherever the day changes, so the timeline reads top to bottom. */
function toRows(
  items: ScheduleItem[],
  tone: (index: number) => "past" | "next" | "later",
) {
  const rows: Row[] = [];
  let day = "";
  items.forEach((item, index) => {
    const label = dayLabel(item.startsAt, tz);
    if (label !== day) {
      rows.push({ kind: "day", label });
      day = label;
    }
    const next = items[index + 1];
    const last = !next || dayLabel(next.startsAt, tz) !== label;
    rows.push({ kind: "item", item, tone: tone(index), last });
  });
  return rows;
}

/**
 * Home is the schedule: the whole weekend as one timeline. Once the event is
 * under way a "Now" line marks the present and earlier items fold away behind
 * "Show earlier", so the next thing is always near the top.
 */
export function Home() {
  const snapshot = readSnapshot();
  const [now, setNow] = createSignal(new Date());
  const [showEarlier, setShowEarlier] = createSignal(false);
  const timer = setInterval(() => setNow(new Date()), 1000);
  onCleanup(() => clearInterval(timer));

  const countdown = () => hackingCountdown(sampleEvent, now());
  // The timeline only needs to move once a minute.
  const minute = () => Math.floor(now().getTime() / 60000);
  const split = () => splitAtNow(sampleSchedule, new Date(minute() * 60000));
  const startsSoon = (iso: string) => {
    const delta = Date.parse(iso) - minute() * 60000;
    return delta > 0 && delta <= SOON_MS;
  };

  const rows = () => {
    const { past, upcoming } = split();
    const earlier = showEarlier() ? toRows(past, () => "past") : [];
    const nowLine: Row[] = past.length > 0 ? [{ kind: "now" }] : [];
    return [
      ...earlier,
      ...nowLine,
      ...toRows(upcoming, (i) => (i === 0 ? "next" : "later")),
    ];
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

      <section class="flex flex-col gap-3" aria-labelledby="schedule-heading">
        <div class="flex items-baseline justify-between">
          <h2
            id="schedule-heading"
            class="text-xl font-bold tracking-[-0.01em]"
          >
            Schedule
          </h2>
          <Show when={split().past.length > 0}>
            <button
              type="button"
              onClick={() => setShowEarlier(!showEarlier())}
              class="text-sm font-bold text-beau dark:text-beau-glow"
            >
              {showEarlier()
                ? "Hide earlier"
                : `Show earlier (${split().past.length})`}
            </button>
          </Show>
        </div>

        <ol class="flex flex-col">
          <For each={rows()}>
            {(row) => (
              <Switch>
                <Match when={row.kind === "day" && row}>
                  {(day) => (
                    <li class="pt-4 pb-3 text-[13px] font-semibold tracking-[0.1em] text-muted uppercase dark:text-muted-inverse">
                      {day().label}
                    </li>
                  )}
                </Match>
                <Match when={row.kind === "now"}>
                  <li class="flex items-center gap-3 py-3" aria-label="Now">
                    <span class="text-[13px] font-extrabold tracking-[0.1em] text-ink uppercase dark:text-gold">
                      Now
                    </span>
                    <span
                      class="h-0.5 grow rounded-full bg-gold"
                      aria-hidden="true"
                    />
                  </li>
                </Match>
                <Match when={row.kind === "item" && row}>
                  {(entry) => (
                    <TimelineItem
                      item={entry().item}
                      tone={entry().tone}
                      last={entry().last}
                      soon={startsSoon(entry().item.startsAt)}
                    />
                  )}
                </Match>
              </Switch>
            )}
          </For>
        </ol>
      </section>
    </div>
  );
}

function TimelineItem(props: {
  item: ScheduleItem;
  tone: "past" | "next" | "later";
  last: boolean;
  soon: boolean;
}) {
  const dot = () =>
    props.soon
      ? "pulse bg-gold"
      : props.tone === "next"
        ? "bg-beau dark:bg-beau-glow"
        : props.tone === "past"
          ? "bg-muted/40 dark:bg-muted-inverse/40"
          : "border-2 border-beau/40 bg-paper dark:border-beau-glow/50 dark:bg-abyss";
  return (
    <li
      class={`grid grid-cols-[64px_20px_1fr] gap-x-3 ${props.tone === "past" ? "opacity-45" : ""}`}
    >
      <span class="pt-0.5 text-sm font-semibold text-muted tabular-nums dark:text-muted-inverse">
        {formatTime(props.item.startsAt, tz)}
      </span>
      <span class="relative flex justify-center" aria-hidden="true">
        <span class={`relative z-[1] mt-1.5 size-3 rounded-full ${dot()}`} />
        <Show when={!props.last}>
          <span class="absolute top-4 bottom-0 w-0.5 bg-beau/15 dark:bg-beau-glow/20" />
        </Show>
      </span>
      <div class="flex flex-col gap-0.5 pb-7">
        <div class="flex items-baseline justify-between gap-3">
          <span class="text-[19px] leading-tight font-bold">
            {props.item.title}
          </span>
          <Show when={props.item.points}>
            <span class="text-sm font-extrabold text-beau tabular-nums dark:text-beau-glow">
              +{props.item.points}
            </span>
          </Show>
        </div>
        <span class="text-[13px]">
          <span class={`font-bold ${kindColor[props.item.kind]}`}>
            {kindLabel[props.item.kind]}
          </span>
          <span class="text-muted dark:text-muted-inverse">
            {" "}
            in {props.item.location}
          </span>
        </span>
      </div>
    </li>
  );
}
