// Stroke icons drawn on a 24px grid. Decorative by default; label the control, not the icon.
import type { JSX } from "solid-js";

function Icon(props: { size?: number; class?: string; children: JSX.Element }) {
  return (
    <svg
      width={props.size ?? 22}
      height={props.size ?? 22}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
      class={props.class}
    >
      {props.children}
    </svg>
  );
}

type P = { size?: number; class?: string };

export const HomeIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <path d="M3 11l9-7 9 7v9a1 1 0 0 1-1 1h-5v-6h-6v6H4a1 1 0 0 1-1-1z" />
  </Icon>
);
/** A lifebuoy, for asking a mentor for help. */
export const HelpIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <circle cx="12" cy="12" r="9" />
    <circle cx="12" cy="12" r="4" />
    <path d="M5.6 5.6l3.6 3.6M14.8 14.8l3.6 3.6M18.4 5.6l-3.6 3.6M9.2 14.8l-3.6 3.6" />
  </Icon>
);
export const ScanIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3M4 12h16" />
  </Icon>
);
export const MapIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <path d="M9 4L3 6v14l6-2 6 2 6-2V4l-6 2z" />
    <path d="M9 4v14M15 6v14" />
  </Icon>
);
export const FlagIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <path d="M5 21V4M5 4h12l-2.5 4L17 12H5" />
  </Icon>
);
export const PersonIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <circle cx="12" cy="8" r="4" />
    <path d="M4 21a8 8 0 0 1 16 0" />
  </Icon>
);
export const CrownIcon = (p: P) => (
  <Icon size={p.size} class={p.class}>
    <path d="M3 18h18M4 18l-1-11 5 4 4-7 4 7 5-4-1 11" />
  </Icon>
);
