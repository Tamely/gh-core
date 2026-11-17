import { createSignal } from "solid-js";
import { CrownMark, Eyebrow } from "./ui";

type Side = "beau" | "park" | null;

/** Beau vs. Parkeaux split. Hover a side and it springs open to 60% (desktop). */
export function Sides() {
  const [focus, setFocus] = createSignal<Side>(null);
  const split = () => (focus() === "beau" ? 60 : focus() === "park" ? 40 : 50);
  const top = () => split() + 7;
  const bottom = () => split() - 7;
  const dim = (side: Side) =>
    focus() && focus() !== side ? "brightness(.55) saturate(.5)" : "none";
  const scale = (side: Side) =>
    focus() === side ? "scale(1.1)" : focus() ? "scale(.8)" : "scale(1)";

  return (
    <section aria-labelledby="sides-title" class="bg-abyss text-white">
      <h2 id="sides-title" class="sr-only">
        Pick your side
      </h2>

      {/* Desktop: diagonal split that reacts to hover. */}
      <div
        class="relative hidden h-[640px] overflow-hidden md:block"
        onMouseLeave={() => setFocus(null)}
      >
        <div
          class="split-panel absolute inset-0"
          onMouseEnter={() => setFocus("beau")}
          style={{
            "clip-path": `polygon(0 0, ${top()}% 0, ${bottom()}% 100%, 0 100%)`,
            filter: dim("beau"),
            background:
              "radial-gradient(70% 90% at 22% 40%, #7b3fc4 0%, #461d7c 38%, #1c0b33 100%)",
          }}
        >
          <div
            class="split-slot absolute inset-y-0 left-0 flex flex-col items-center justify-center gap-4 px-10 text-center"
            style={{ width: `${split()}%` }}
          >
            <div class="split-hero" style={{ transform: scale("beau") }}>
              <CrownMark
                size={180}
                class="float text-gold drop-shadow-[0_0_28px_rgb(253_208_35_/_0.6)]"
              />
            </div>
            <Eyebrow class="text-gold">Restore the realm</Eyebrow>
            <p class="text-[56px] leading-none font-extrabold tracking-[-0.04em]">
              Team Beau
            </p>
            <p class="max-w-sm text-lg text-white/80">
              Win back the cap and get Beau home before graduation.
            </p>
          </div>
        </div>

        <div
          class="split-panel absolute inset-0"
          onMouseEnter={() => setFocus("park")}
          style={{
            "clip-path": `polygon(${top()}% 0, 100% 0, 100% 100%, ${bottom()}% 100%)`,
            filter: dim("park"),
            "background-color": "#121216",
            "background-image":
              "radial-gradient(70% 90% at 78% 40%, rgb(253 208 35 / .22), transparent 60%), repeating-linear-gradient(135deg, transparent 0 46px, rgb(253 208 35 / .07) 46px 70px)",
          }}
        >
          <div
            class="split-slot absolute inset-y-0 flex flex-col items-center justify-center gap-4 px-10 text-center"
            style={{ left: `${split()}%`, width: `${100 - split()}%` }}
          >
            <div class="split-hero" style={{ transform: scale("park") }}>
              <svg
                width="180"
                height="180"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="1.4"
                stroke-linecap="round"
                stroke-linejoin="round"
                aria-hidden="true"
                class="float text-gold drop-shadow-[0_0_28px_rgb(253_208_35_/_0.5)]"
                style={{ "animation-delay": "-2.5s" }}
              >
                <path d="M12 3l9 16H3zM12 9v4M12 16h.01M2 21h20" />
              </svg>
            </div>
            <Eyebrow class="text-gold">Keep the tolls</Eyebrow>
            <p class="text-[56px] leading-none font-extrabold tracking-[-0.04em]">
              Team Parkeaux
            </p>
            <p class="max-w-sm text-lg text-white/80">
              Help the Vespers keep every road a toll road.
            </p>
          </div>
        </div>

        <div
          aria-hidden="true"
          class="split-panel pointer-events-none absolute inset-0 bg-gold"
          style={{
            "clip-path": `polygon(${top() - 0.2}% 0, ${top() + 0.2}% 0, ${bottom() + 0.2}% 100%, ${bottom() - 0.2}% 100%)`,
          }}
        />
      </div>

      {/* Phone: the two sides stacked, no hover. */}
      <div class="md:hidden">
        <div
          class="flex flex-col items-center gap-3 px-6 py-16 text-center"
          style={{
            background:
              "radial-gradient(90% 80% at 30% 30%, #7b3fc4 0%, #461d7c 45%, #1c0b33 100%)",
          }}
        >
          <CrownMark size={110} class="float text-gold" />
          <Eyebrow class="text-gold">Restore the realm</Eyebrow>
          <p class="text-[44px] leading-none font-extrabold tracking-[-0.04em]">
            Team Beau
          </p>
          <p class="text-white/80">
            Win back the cap and get Beau home before graduation.
          </p>
        </div>
        <div class="h-1 bg-gold" aria-hidden="true" />
        <div
          class="flex flex-col items-center gap-3 px-6 py-16 text-center"
          style={{
            "background-color": "#121216",
            "background-image":
              "repeating-linear-gradient(135deg, transparent 0 34px, rgb(253 208 35 / .07) 34px 52px)",
          }}
        >
          <Eyebrow class="text-gold">Keep the tolls</Eyebrow>
          <p class="text-[44px] leading-none font-extrabold tracking-[-0.04em]">
            Team Parkeaux
          </p>
          <p class="text-white/80">
            Help the Vespers keep every road a toll road.
          </p>
        </div>
      </div>

      <div class="mx-auto grid max-w-6xl gap-8 px-6 py-16 md:grid-cols-2 md:py-20">
        <p class="text-[clamp(24px,3vw,36px)] leading-tight font-extrabold tracking-[-0.02em]">
          You pick at the opening ceremony. Then every scan counts.
        </p>
        <p class="text-lg leading-relaxed text-ink-inverse/75">
          Scan the QR codes at workshops, meals, mini-games and sponsor booths
          to score for your side. A side closes to new hackers once it passes
          52%, so neither one runs away with it. The final total decides which
          ending plays at closing.
        </p>
      </div>
    </section>
  );
}
