import { For } from "solid-js";
import { APPLY_URL, EVENT_DATES, VENUE } from "./content";
import { CrownMark, stars } from "./ui";

const sky = stars(34, 7);

/**
 * Kept deliberately simple, after HackUTD's hero: one line of context, one
 * heading, a thin line drawing along the bottom. The drawing is campus on
 * Nicholson Drive, with Beau's crown dropping into the pothole.
 */
export function Hero() {
  return (
    <header
      id="top"
      class="relative isolate overflow-hidden bg-abyss text-ink-inverse"
    >
      <For each={sky}>
        {(star) => (
          <span
            aria-hidden="true"
            class="star absolute -z-10 size-[3px] rounded-full bg-gold"
            style={{
              top: `calc(${star.top} * 0.7)`,
              left: star.left,
              "animation-duration": star.duration,
              "animation-delay": star.delay,
            }}
          />
        )}
      </For>

      <nav class="mx-auto flex max-w-6xl items-center justify-between px-6 py-5">
        <a
          href="#top"
          class="flex items-center gap-2 text-ink-inverse no-underline"
        >
          <CrownMark size={22} class="text-gold" />
          <span class="text-[17px] font-bold tracking-tight">geaux hack</span>
        </a>
        <div class="flex items-center gap-8">
          <div class="hidden items-center gap-7 text-[13px] font-semibold tracking-[0.08em] text-ink-inverse/75 uppercase md:flex">
            <a href="#story" class="no-underline hover:text-white">
              Story
            </a>
            <a href="#districts" class="no-underline hover:text-white">
              Tracks
            </a>
            <a href="#faq" class="no-underline hover:text-white">
              FAQ
            </a>
            <a href="#sponsor" class="no-underline hover:text-white">
              Sponsor
            </a>
          </div>
          <a
            href={APPLY_URL}
            class="press rounded-full bg-gold px-5 py-2 text-[13px] font-extrabold tracking-[0.06em] text-ink uppercase no-underline"
          >
            Apply
          </a>
        </div>
      </nav>

      <div class="mx-auto flex max-w-4xl flex-col items-center px-6 pt-[14vh] text-center">
        <p class="text-[15px] font-semibold text-ink-inverse/80 italic">
          WiCS Geaux Hack 2027 at LSU
        </p>
        <h1 class="mt-3 text-[clamp(36px,5.6vw,64px)] leading-[1.08] font-medium tracking-[-0.02em]">
          Geaux beyond the known
        </h1>
        <p class="mt-4 text-[15px] text-ink-inverse/55">
          {EVENT_DATES ?? "Dates announced soon"} at {VENUE}
        </p>
      </div>

      <Skyline />
    </header>
  );
}

/** Thin white line art that draws itself in. Every stroke uses pathLength="1". */
function Skyline() {
  const line = "sketch fill-none stroke-ink-inverse/80";
  return (
    <svg
      viewBox="0 0 1200 300"
      preserveAspectRatio="xMidYMax meet"
      class="mx-auto mt-[8vh] block w-full max-w-6xl"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      role="img"
      aria-label="Line drawing of the LSU campus along Nicholson Drive, with a crown falling into a pothole"
    >
      {/* Oaks on the left */}
      <path
        pathLength="1"
        class={line}
        d="M70 250v-34M60 222c-22 2-32-20-16-32-6-20 18-30 30-18 10-16 36-8 34 10 18 4 16 30-4 34-10 10-34 12-44 6z"
      />
      <path
        pathLength="1"
        class={line}
        d="M170 250v-26M162 228c-16 0-22-16-10-24-2-16 18-22 26-12 10-10 28-2 24 12 12 6 6 24-8 24-8 6-24 6-32 0z"
      />

      {/* Memorial Tower */}
      <path
        pathLength="1"
        class={line}
        d="M268 250V92h40v158M268 92l20-34 20 34M288 58v-16M276 120h24v26h-24zM280 250v-60h16v60"
      />
      <circle pathLength="1" class={line} cx="288" cy="108" r="7" />

      {/* Classroom buildings with rows of windows */}
      <path
        pathLength="1"
        class={line}
        d="M340 250V160h150v90M330 160l85-30 85 30M360 182h14M392 182h14M424 182h14M456 182h14M360 210h14M392 210h14M424 210h14M456 210h14M406 250v-26h18v26"
      />

      {/* Crown falling into the pothole */}
      <g class="float">
        <path
          pathLength="1"
          class="sketch fill-none stroke-gold"
          stroke-width="2"
          d="M586 176h28M588 176l-3-18 9 7 6-12 6 12 9-7-3 18"
        />
      </g>

      {/* PFT Hall: long and low */}
      <path
        pathLength="1"
        class={line}
        d="M700 250v-74h300v74M700 176l20-16h260l20 16M730 196h240M730 216h240M760 176v74M820 176v74M880 176v74M940 176v74"
      />

      {/* Oak on the right */}
      <path
        pathLength="1"
        class={line}
        d="M1080 250v-38M1070 216c-24 2-36-22-18-36-6-22 20-32 34-20 12-18 40-8 38 12 20 6 16 34-6 38-12 10-38 12-48 6z"
      />

      {/* Nicholson Drive and the pothole */}
      <path pathLength="1" class={line} d="M0 250h548M652 250h548M0 290h1200" />
      <path
        pathLength="1"
        class="fill-none stroke-ink-inverse/35"
        stroke-dasharray="0.012 0.02"
        d="M0 270h1200"
      />
      <ellipse
        pathLength="1"
        class="sketch fill-none stroke-gold"
        stroke-width="1.8"
        cx="600"
        cy="252"
        rx="52"
        ry="9"
      />
    </svg>
  );
}
