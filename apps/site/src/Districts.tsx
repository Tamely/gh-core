import { For, type JSX } from "solid-js";
import { Eyebrow } from "./ui";

const districts: {
  name: string;
  track: string;
  vesper: string;
  bg: string;
  fg: string;
  icon: () => JSX.Element;
}[] = [
  {
    name: "The Fragmented City",
    track: "Web and app development",
    vesper:
      "Drexa erased whole sectors from the maps. Rebuild the connections.",
    bg: "hover:bg-track-web focus-within:bg-track-web hover:text-white focus-within:text-white",
    fg: "text-white",
    icon: () => (
      <>
        <circle cx="12" cy="12" r="9" />
        <path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" />
      </>
    ),
  },
  {
    name: "The Neural Nexus",
    track: "AI and machine learning",
    vesper:
      "Nexus built models that steer everyone into toll booths. Take back the choice.",
    bg: "hover:bg-track-ai focus-within:bg-track-ai hover:text-white focus-within:text-white",
    fg: "text-white",
    icon: () => (
      <>
        <rect x="7" y="7" width="10" height="10" rx="2" />
        <path d="M9 3v4M15 3v4M9 17v4M15 17v4M3 9h4M3 15h4M17 9h4M17 15h4" />
      </>
    ),
  },
  {
    name: "The Forgotten Grounds",
    track: "Beginners",
    vesper:
      "Onyx keeps the abandoned workshops. A good place to build your first project.",
    bg: "hover:bg-track-beginner focus-within:bg-track-beginner hover:text-white focus-within:text-white",
    fg: "text-white",
    icon: () => (
      <>
        <circle cx="12" cy="12" r="9" />
        <path d="M15.5 8.5l-2 5-5 2 2-5z" />
      </>
    ),
  },
  {
    name: "The Oddworks",
    track: "Funniest or most useful",
    vesper:
      "Hei Feng runs a lab for ridiculous ideas and genuinely handy tools.",
    bg: "hover:bg-gold focus-within:bg-gold hover:text-ink focus-within:text-ink",
    fg: "text-ink",
    icon: () => (
      <path d="M9 3h6M10 3v6l-5 9a2 2 0 0 0 2 3h10a2 2 0 0 0 2-3l-5-9V3" />
    ),
  },
  {
    name: "The Hidden Territories",
    track: "Sponsor challenges",
    vesper: "Tartereus trades in prompts from beyond the city limits.",
    bg: "hover:bg-track-hidden focus-within:bg-track-hidden hover:text-white focus-within:text-white",
    fg: "text-white",
    icon: () => (
      <>
        <circle cx="8" cy="15" r="4" />
        <path d="M11 12l9-9M17 6l3 3" />
      </>
    ),
  },
];

export function Districts() {
  return (
    <section id="districts" class="bg-abyss text-ink-inverse">
      <div class="mx-auto max-w-6xl px-6 pt-[clamp(80px,12vw,150px)] pb-10">
        <Eyebrow class="text-gold">The tracks</Eyebrow>
        <h2 class="mt-4 max-w-3xl text-[clamp(40px,6vw,80px)] leading-[0.95] font-extrabold tracking-[-0.04em]">
          The Districts of Abyssus
        </h2>
        <p class="mt-6 max-w-xl text-lg text-ink-inverse/70">
          Each track is held by one of Parkeaux’s Vespers. Pick one and build
          your way through it.
        </p>
      </div>
      <ul class="border-t border-white/12 pb-[clamp(60px,8vw,120px)]">
        <For each={districts}>
          {(district) => (
            <li
              tabindex="0"
              class={`district group relative overflow-hidden border-b border-white/12 outline-none ${district.bg}`}
            >
              <svg
                width="220"
                height="220"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.5"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
                class={`watermark pointer-events-none absolute top-1/2 -right-8 -translate-y-1/2 ${district.fg}`}
              >
                {district.icon()}
              </svg>
              <div class="relative mx-auto grid max-w-6xl gap-2 px-6 py-8 md:grid-cols-[minmax(0,7fr)_minmax(0,5fr)] md:items-center md:gap-10">
                <h3
                  class={`text-[clamp(30px,4.6vw,60px)] leading-none font-extrabold tracking-[-0.035em]`}
                >
                  {district.name}
                </h3>
                <div class="flex flex-col gap-1">
                  <span class="text-[15px] font-extrabold">
                    {district.track}
                  </span>
                  <span class="text-[15px] leading-snug opacity-75">
                    {district.vesper}
                  </span>
                </div>
              </div>
            </li>
          )}
        </For>
      </ul>
    </section>
  );
}
