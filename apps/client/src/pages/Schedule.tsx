import { For, Show } from "solid-js";
import { sampleEvent, sampleSchedule } from "../schedule/sample";
import {
  formatTime,
  groupByDay,
  kindColor,
  kindLabel,
} from "../schedule/schedule";

export function Schedule() {
  const days = groupByDay(sampleSchedule, sampleEvent.timeZone);
  return (
    <div class="flex flex-col gap-6">
      <h1 class="text-[30px] font-extrabold tracking-[-0.03em]">Schedule</h1>
      <For each={days}>
        {(day) => (
          <section class="flex flex-col gap-3">
            <h2 class="px-1 text-[13px] font-semibold tracking-[0.1em] text-muted uppercase dark:text-muted-inverse">
              {day.day}
            </h2>
            <ul class="flex flex-col rounded-[24px] bg-surface px-[18px] dark:bg-abyss-surface">
              <For each={day.items}>
                {(item) => (
                  <li class="flex gap-4 border-b border-line py-3.5 last:border-b-0 dark:border-abyss-line">
                    <span class="w-[72px] shrink-0 pt-0.5 text-sm font-semibold text-muted tabular-nums dark:text-muted-inverse">
                      {formatTime(item.startsAt, sampleEvent.timeZone)}
                    </span>
                    <div class="flex min-w-0 grow flex-col gap-0.5">
                      <span class="text-base leading-snug font-bold">
                        {item.title}
                      </span>
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
                    <Show when={item.points}>
                      <span class="pt-0.5 text-sm font-extrabold text-beau tabular-nums dark:text-beau-glow">
                        +{item.points}
                      </span>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          </section>
        )}
      </For>
    </div>
  );
}
