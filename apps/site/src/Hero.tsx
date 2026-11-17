import { For, Show } from "solid-js";
import {
  APPLY_URL,
  EVENT_DATES,
  REGISTRATION_OPEN,
  SPONSOR_EMAIL,
  VENUE,
} from "./content";
import { NotifyForm } from "./Notify";
import { CrownMark, RollButton, stars } from "./ui";

const sky = stars(40, 7);

/**
 * Full-screen and simple, after HackUTD's hero: one line of context, one
 * heading, and a line drawing of campus on Nicholson across the whole bottom
 * edge. Once it's drawn, Beau's crown falls once and comes to rest tipped over on
 * the rim of the pothole, which flashes gold and then keeps a soft glow.
 * While registration is closed, the only action is HackMIT's "notify me".
 */
export function Hero() {
  return (
    <header
      id="top"
      class="relative isolate flex h-svh min-h-[600px] flex-col overflow-hidden bg-abyss text-ink-inverse"
    >
      <For each={sky}>
        {(star) => (
          <span
            aria-hidden="true"
            class="star absolute -z-10 size-[3px] rounded-full bg-gold"
            style={{
              top: `calc(${star.top} * 0.65)`,
              left: star.left,
              "animation-duration": star.duration,
              "animation-delay": star.delay,
            }}
          />
        )}
      </For>
      {/* Clouds stay in the band above the heading so they never cross the text. */}
      <Cloud class="top-[9%]" style={{ "animation-delay": "-12s" }} />
      <Cloud class="top-[13%] scale-75" style={{ "animation-delay": "-48s" }} />
      <Cloud class="top-[6%] scale-90" style={{ "animation-delay": "-80s" }} />

      <nav class="mx-auto flex w-full max-w-6xl items-center justify-between px-6 py-5">
        <a
          href="#top"
          class="flex items-center gap-2 text-ink-inverse no-underline"
        >
          <CrownMark size={22} class="text-gold" />
          <span class="text-[17px] font-bold tracking-tight">geaux hack</span>
        </a>
        {/* Section links only exist once the full page does. */}
        <Show when={REGISTRATION_OPEN}>
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
        </Show>
      </nav>

      <div class="mx-auto flex w-full max-w-4xl flex-col items-center px-6 pt-[11vh] text-center">
        <p class="text-[15px] font-semibold text-ink-inverse/80 italic">
          WiCS Geaux Hack 2027 at LSU
        </p>
        <h1 class="mt-3 text-[clamp(38px,6vw,72px)] leading-[1.05] font-medium tracking-[-0.025em]">
          Geaux beyond the known
        </h1>
        <p class="mt-4 text-[15px] text-ink-inverse/55">
          {EVENT_DATES ?? "Dates announced soon"} at {VENUE}
        </p>
        <div class="mt-9 flex w-full flex-col items-center">
          <Show
            when={REGISTRATION_OPEN}
            fallback={
              <>
                <p class="mb-4 text-[17px] font-semibold">
                  Hackers, mentors and judges: get an email when registration
                  opens.
                </p>
                <NotifyForm />
                <p class="mt-6 text-[13px] text-ink-inverse/50">
                  Interested in sponsoring?{" "}
                  <a
                    href={`mailto:${SPONSOR_EMAIL}`}
                    class="text-ink-inverse/80 underline underline-offset-4 hover:text-white"
                  >
                    {SPONSOR_EMAIL}
                  </a>
                </p>
              </>
            }
          >
            <RollButton href={APPLY_URL} label="Apply to hack" tone="gold" />
          </Show>
        </div>
      </div>

      <Skyline />
    </header>
  );
}

function Cloud(props: { class: string; style: Record<string, string> }) {
  return (
    <svg
      aria-hidden="true"
      width="132"
      height="72"
      viewBox="-6 -20 132 72"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      class={`drift absolute left-0 -z-10 text-ink-inverse/45 ${props.class}`}
      style={props.style}
    >
      <path d="M22 42h76a14 14 0 0 0-4-27 20 20 0 0 0-37-8 15 15 0 0 0-25 14 11 11 0 0 0-10 21z" />
    </svg>
  );
}

/**
 * Campus along Nicholson, drawn edge to edge. Strokes use pathLength="1" so
 * each piece can sketch itself in; the delays draw the road first, then the
 * buildings outward from the pothole.
 */
function Skyline() {
  const ink = "sketch fill-none stroke-ink-inverse/80";
  // Shapes are drawn with their base at y=230; +40 sits them on the road at y=270.
  const at = (x: number) => `translate(${x} 40)`;
  const delay = (s: number) => ({ "animation-delay": `${s}s` });

  // Shapes drawn at their own origin; placed below with translate().
  const oakBig =
    "M10 230v-34M0 202c-22 2-32-20-16-32-6-20 18-30 30-18 10-16 36-8 34 10 18 4 16 30-4 34-10 10-34 12-44 6z";
  const oakSmall =
    "M10 230v-26M2 208c-16 0-22-16-10-24-2-16 18-22 26-12 10-10 28-2 24 12 12 6 6 24-8 24-8 6-24 6-32 0z";
  const tower =
    "M0 230V72h40v158M0 72l20-34 20 34M20 38v-16M8 100h24v26H8zM12 230v-60h16v60";
  const hall =
    "M10 230V140h150v90M0 140l85-30 85 30M30 162h14M62 162h14M94 162h14M126 162h14M30 190h14M62 190h14M94 190h14M126 190h14M76 230v-26h18v26";
  const pft =
    "M0 230v-74h300v74M0 156l20-16h260l20 16M30 176h240M30 196h240M60 156v74M120 156v74M180 156v74M240 156v74";

  return (
    <svg
      viewBox="0 0 1600 320"
      preserveAspectRatio="xMidYMax slice"
      // Height follows width at the drawing's 5:1 ratio, so wide screens never
      // crop the top (the tower). Narrow screens crop the sides instead.
      class="absolute inset-x-0 bottom-0 -z-10 h-[max(20vw,200px)] w-full"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      role="img"
      aria-label="Line drawing of the LSU campus along Nicholson Drive. A crown falls into a pothole in the road."
    >
      {/* Road */}
      <path
        pathLength="1"
        class={ink}
        style={delay(0)}
        d="M0 270h744M856 270h744M0 312h1600"
      />
      <path
        class="fill-none stroke-ink-inverse/30"
        stroke-dasharray="14 18"
        d="M0 291h1600"
      />

      {/* Left of the pothole, drawn from the center outward */}
      <path
        pathLength="1"
        class={ink}
        style={delay(0.35)}
        transform={at(612)}
        d={oakSmall}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(0.5)}
        transform={at(420)}
        d={hall}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(0.65)}
        transform={at(352)}
        d={tower}
      />
      <circle
        pathLength="1"
        class={ink}
        style={delay(0.65)}
        cx="372"
        cy="128"
        r="7"
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(0.8)}
        transform={at(262)}
        d={oakSmall}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(0.9)}
        transform={at(70)}
        d={hall}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(1)}
        transform={at(10)}
        d={oakBig}
      />

      {/* Right of the pothole */}
      <path
        pathLength="1"
        class={ink}
        style={delay(0.45)}
        transform={at(890)}
        d={pft}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(0.7)}
        transform={at(1226)}
        d={oakBig}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(0.85)}
        transform={at(1290)}
        d={hall}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(1)}
        transform={at(1490)}
        d={oakSmall}
      />
      <path
        pathLength="1"
        class={ink}
        style={delay(1.05)}
        transform={at(1550)}
        d={oakBig}
      />

      {/* The pothole, its glow when the crown lands, and the falling crown */}
      <ellipse
        class="pothole-glow fill-gold"
        cx="800"
        cy="272"
        rx="56"
        ry="9"
      />
      <ellipse
        pathLength="1"
        class="sketch fill-none stroke-gold"
        style={delay(1.1)}
        stroke-width="1.8"
        cx="800"
        cy="272"
        rx="56"
        ry="9"
      />
      <g class="crown-drop">
        <path
          class="fill-none stroke-gold"
          stroke-width="2"
          d="M786 236h28M788 236l-3-18 9 7 6-12 6 12 9-7-3 18"
        />
      </g>
    </svg>
  );
}
