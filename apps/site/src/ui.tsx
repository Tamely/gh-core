import type { JSX } from "solid-js";

/** A link styled as a button whose label rolls up on hover. */
export function RollButton(props: {
  href: string;
  label: string;
  tone: "gold" | "ghost-light" | "ink";
}) {
  const tones = {
    gold: "bg-gold text-ink",
    "ghost-light": "border-2 border-white/35 text-white hover:border-white/70",
    ink: "bg-ink text-white",
  };
  return (
    <a
      href={props.href}
      class={`press inline-flex items-center rounded-full px-7 py-4 text-[17px] font-extrabold no-underline ${tones[props.tone]}`}
    >
      <span class="roll">
        <span class="roll-in">
          <span>{props.label}</span>
          <span aria-hidden="true">{props.label}</span>
        </span>
      </span>
    </a>
  );
}

export function CrownMark(props: {
  size: number;
  class?: string;
  draw?: boolean;
}) {
  return (
    <svg
      width={props.size}
      height={props.size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="1.4"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      class={props.class}
    >
      <path
        class={props.draw ? "draw" : undefined}
        d="M3 18h18M4 18l-1-11 5 4 4-7 4 7 5-4-1 11M6 21h12"
      />
    </svg>
  );
}

export function Eyebrow(props: { children: JSX.Element; class?: string }) {
  return (
    <p
      class={`text-[13px] font-bold tracking-[0.16em] uppercase ${props.class ?? ""}`}
    >
      {props.children}
    </p>
  );
}

/** Deterministic star positions so the sky doesn't jump between renders. */
export function stars(count: number, seed: number) {
  let s = seed;
  const rand = () => (s = (s * 9301 + 49297) % 233280) / 233280;
  return Array.from({ length: count }, () => ({
    top: `${rand() * 100}%`,
    left: `${rand() * 100}%`,
    size: 2 + Math.round(rand() * 4),
    duration: `${3.5 + rand() * 3}s`,
    delay: `${-rand() * 6}s`,
    gold: rand() > 0.25,
  }));
}
