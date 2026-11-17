import type { EventInfo, EventKind, ScheduleItem } from "./types";

export const kindLabel: Record<EventKind, string> = {
  workshop: "Workshop",
  meal: "Meal",
  social: "Social",
  minigame: "Mini-game",
  ceremony: "Ceremony",
  judging: "Judging",
};

/** Tailwind text color per kind. Colored text only: no tinted badges (docs/design.md). */
export const kindColor: Record<EventKind, string> = {
  workshop: "text-track-ai dark:text-beau-glow",
  meal: "text-muted dark:text-muted-inverse",
  social: "text-track-beginner dark:text-green-400",
  minigame: "text-track-web dark:text-blue-400",
  ceremony: "text-beau dark:text-beau-glow",
  judging: "text-ink dark:text-ink-inverse",
};

/** Items that haven't ended yet, soonest first. */
export function upNext(
  items: ScheduleItem[],
  now: Date,
  count: number,
): ScheduleItem[] {
  const t = now.getTime();
  return [...items]
    .filter((item) => Date.parse(item.endsAt) > t)
    .sort((a, b) => Date.parse(a.startsAt) - Date.parse(b.startsAt))
    .slice(0, count);
}

/**
 * Splits the schedule at `now`, both halves in start order. An item counts as
 * past once it has ended; anything still running is upcoming.
 */
export function splitAtNow(
  items: ScheduleItem[],
  now: Date,
): { past: ScheduleItem[]; upcoming: ScheduleItem[] } {
  const t = now.getTime();
  const sorted = [...items].sort(
    (a, b) => Date.parse(a.startsAt) - Date.parse(b.startsAt),
  );
  return {
    past: sorted.filter((item) => Date.parse(item.endsAt) <= t),
    upcoming: sorted.filter((item) => Date.parse(item.endsAt) > t),
  };
}

/** Labels the event day an item falls on, e.g. "Saturday, Feb 20". */
export function dayLabel(iso: string, timeZone: string): string {
  return new Intl.DateTimeFormat("en-US", {
    timeZone,
    weekday: "long",
    month: "short",
    day: "numeric",
  }).format(new Date(iso));
}

/** Groups items by calendar day in the event's time zone, keeping start order. */
export function groupByDay(
  items: ScheduleItem[],
  timeZone: string,
): { day: string; items: ScheduleItem[] }[] {
  const dayFormat = new Intl.DateTimeFormat("en-US", {
    timeZone,
    weekday: "long",
    month: "short",
    day: "numeric",
  });
  const groups = new Map<string, ScheduleItem[]>();
  const sorted = [...items].sort(
    (a, b) => Date.parse(a.startsAt) - Date.parse(b.startsAt),
  );
  for (const item of sorted) {
    const day = dayFormat.format(new Date(item.startsAt));
    const group = groups.get(day);
    if (group) group.push(item);
    else groups.set(day, [item]);
  }
  return [...groups].map(([day, dayItems]) => ({ day, items: dayItems }));
}

export function formatTime(iso: string, timeZone: string): string {
  return new Intl.DateTimeFormat("en-US", {
    timeZone,
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(iso));
}

export type Countdown = { label: string; value: string };

/** "Hacking starts in 2d 04:10:00", "Hacking ends in 21:14:09", or "Hacking is over". */
export function hackingCountdown(event: EventInfo, now: Date): Countdown {
  const start = Date.parse(event.hackingStartsAt);
  const end = Date.parse(event.hackingEndsAt);
  const t = now.getTime();
  if (t >= end) return { label: "Hacking is over", value: "" };
  const [label, target] =
    t < start ? ["Hacking starts in", start] : ["Hacking ends in", end];
  return { label, value: formatDuration(target - t) };
}

function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const days = Math.floor(total / 86400);
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(Math.floor((total % 86400) / 3600))}:${pad(Math.floor((total % 3600) / 60))}:${pad(total % 60)}`;
  return days > 0 ? `${days}d ${clock}` : clock;
}
