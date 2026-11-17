import { For, type JSX } from "solid-js";
import { Eyebrow } from "./ui";

const chapters: { n: string; title: string; body: () => JSX.Element }[] = [
  {
    n: "01",
    title: "The Pothole on Nicholson",
    body: () => (
      <>
        <p>
          On his last day before commencement, Beau put on his cap and purple
          gown like a royal cape. Then he stepped into the biggest crater on
          Nicholson Drive and fell straight through it.
        </p>
        <p>He landed under a landmark sign hanging upside down.</p>
        <p
          class="my-2 inline-block rotate-180 rounded-md bg-ink px-4 py-3 text-[clamp(18px,2.4vw,26px)] font-extrabold tracking-wide text-gold"
          aria-label="Welcome to Abyssus. Watch your step."
        >
          WELCOME TO ABYSSUS. WATCH YOUR STEP.
        </p>
        <p>
          The locals saw a tiger in a crown drop out of the sky and named him
          the Lost Tiger King.
        </p>
      </>
    ),
  },
  {
    n: "02",
    title: "Pothole Pete’s Offer",
    body: () => (
      <>
        <p>
          Parkeaux runs Abyssus by charging a toll on every smooth road, and his
          Vespers snatched Beau’s cap on the way down. Every screen in the realm
          lit up at once.
        </p>
        <blockquote class="border-l-0 text-[clamp(22px,3vw,34px)] leading-tight font-extrabold tracking-tight text-beau">
          “You’re going to want this back.”
        </blockquote>
        <p>
          He would return the cap and reopen the portal home, if Beau told
          everyone to stop fixing the streets. Beau looked over at a local
          shoveling gravel into a trench.
        </p>
        <p class="text-[clamp(20px,2.6vw,28px)] font-extrabold tracking-tight">
          “Hey. Got an extra shovel?”
        </p>
        <p>The screens went black.</p>
      </>
    ),
  },
  {
    n: "03",
    title: "Your move",
    body: () => (
      <p>
        At the opening ceremony you choose a side. Every QR code you scan this
        weekend scores for that side, and the final total decides which ending
        plays at closing.
      </p>
    ),
  },
];

export function Story() {
  return (
    <section id="story" class="bg-paper text-ink">
      <div class="mx-auto max-w-6xl px-6 py-[clamp(80px,12vw,160px)]">
        <Eyebrow class="text-muted">The story so far</Eyebrow>
        <ol class="mt-10 flex flex-col">
          <For each={chapters}>
            {(chapter) => (
              <li class="grid gap-x-12 border-t border-line py-12 md:grid-cols-[minmax(0,5fr)_minmax(0,7fr)]">
                <div class="md:sticky md:top-10 md:self-start">
                  <span class="block text-[clamp(72px,12vw,168px)] leading-[0.8] font-extrabold tracking-[-0.06em] text-beau/15">
                    {chapter.n}
                  </span>
                  <h2 class="mt-3 text-[clamp(28px,3.6vw,44px)] leading-[1.05] font-extrabold tracking-[-0.03em]">
                    {chapter.title}
                  </h2>
                </div>
                <div class="mt-6 flex max-w-xl flex-col items-start gap-5 text-[clamp(17px,1.6vw,20px)] leading-relaxed text-ink/85 md:mt-2">
                  {chapter.body()}
                </div>
              </li>
            )}
          </For>
        </ol>
      </div>
    </section>
  );
}
