import { For } from "solid-js";
import {
  APP_URL,
  APPLY_URL,
  REGISTRATION_OPEN,
  SPONSOR_EMAIL,
} from "./content";
import { CrownMark, Eyebrow, RollButton } from "./ui";

const faqs: { q: string; a: string }[] = [
  {
    q: "Do I need to know how to code?",
    a: "No. The Forgotten Grounds is a track built for first-timers, with workshops and mentors to get you to a working project by Sunday.",
  },
  {
    q: "Do I need a team?",
    a: "No. Bring one or find one at the event. Team size limits are announced when applications open.",
  },
  {
    q: "Is there travel support?",
    a: "Hackers approved for travel from out of state can claim reimbursement in the Geaux Hack app once they check in.",
  },
  {
    q: "Do I need Discord?",
    a: "No. Your schedule, alerts, team and judging room all live in the Geaux Hack app, which works even when the venue Wi-Fi doesn’t. Discord is there if you want to hang out.",
  },
  {
    q: "How does judging work?",
    a: "Teams present live to a panel in a classroom. The app tells you when you’re two slots away and when to walk to the door.",
  },
];

export function Faq() {
  return (
    <section id="faq" class="bg-paper text-ink">
      <div class="mx-auto grid max-w-6xl gap-10 px-6 py-[clamp(80px,12vw,150px)] md:grid-cols-[minmax(0,4fr)_minmax(0,8fr)]">
        <h2 class="text-[clamp(36px,5vw,64px)] leading-[0.95] font-extrabold tracking-[-0.04em]">
          Questions
        </h2>
        <div class="border-t border-ink/15">
          <For each={faqs}>
            {(item) => (
              <details class="group border-b border-ink/15">
                <summary class="flex cursor-pointer items-center justify-between gap-6 py-6 text-[clamp(19px,2vw,24px)] font-extrabold tracking-[-0.01em]">
                  {item.q}
                  <span
                    aria-hidden="true"
                    class="faq-plus shrink-0 text-3xl leading-none font-semibold text-beau"
                  >
                    +
                  </span>
                </summary>
                <p class="max-w-2xl pb-7 text-lg leading-relaxed text-ink/80">
                  {item.a}
                </p>
              </details>
            )}
          </For>
        </div>
      </div>
    </section>
  );
}

export function Sponsor() {
  return (
    <section id="sponsor" class="bg-gold text-ink">
      <div class="mx-auto flex max-w-6xl flex-col items-start gap-8 px-6 py-[clamp(70px,10vw,130px)] md:flex-row md:items-end md:justify-between">
        <div class="max-w-2xl">
          <Eyebrow>Sponsors</Eyebrow>
          <h2 class="mt-4 text-[clamp(40px,6vw,80px)] leading-[0.92] font-extrabold tracking-[-0.04em]">
            Open a route into the Hidden Territories.
          </h2>
          <p class="mt-6 text-lg text-ink/80">
            Run a sponsor challenge and see what students build with your tools
            in one weekend.
          </p>
        </div>
        <RollButton
          href={`mailto:${SPONSOR_EMAIL}`}
          label="Email the team"
          tone="ink"
        />
      </div>
    </section>
  );
}

export function Footer() {
  return (
    <footer class="bg-abyss text-ink-inverse">
      <div class="mx-auto flex max-w-6xl flex-col gap-10 px-6 py-16 md:flex-row md:items-end md:justify-between">
        <div class="flex flex-col gap-3">
          <a
            href="#top"
            class="flex items-center gap-2 text-ink-inverse no-underline"
          >
            <CrownMark size={30} class="text-gold" />
            <span class="text-2xl font-extrabold tracking-tight">
              Geaux Hack
            </span>
          </a>
          <p class="text-[15px] text-ink-inverse/60">
            Made by Women in Computer Science at LSU.
          </p>
        </div>
        <div class="flex flex-wrap gap-x-8 gap-y-3 text-[15px] font-semibold text-ink-inverse/80">
          <a
            href={REGISTRATION_OPEN ? APPLY_URL : "#notify-email"}
            class="no-underline hover:text-white"
          >
            {REGISTRATION_OPEN ? "Apply" : "Get notified"}
          </a>
          <a href={APP_URL} class="no-underline hover:text-white">
            Open the app
          </a>
          <a href={`${APP_URL}/privacy`} class="no-underline hover:text-white">
            Privacy
          </a>
          <a
            href={`mailto:${SPONSOR_EMAIL}`}
            class="no-underline hover:text-white"
          >
            Sponsor
          </a>
        </div>
      </div>
    </footer>
  );
}
