import { For, type Component } from "solid-js";
import { Link, pathname } from "../router";
import { FlagIcon, HelpIcon, HomeIcon, MapIcon, ScanIcon } from "./icons";

const tabs: {
  href: string;
  label: string;
  icon: Component<{ size?: number }>;
}[] = [
  // Home is also the schedule, so there's no separate Schedule tab.
  { href: "/", label: "Home", icon: HomeIcon },
  { href: "/scan", label: "Scan", icon: ScanIcon },
  { href: "/map", label: "Map", icon: MapIcon },
  { href: "/battle", label: "Battle", icon: FlagIcon },
  { href: "/help", label: "Help", icon: HelpIcon },
];

/** Floating liquid glass tab bar. Same five tabs in both themes (docs/design.md). */
export function TabBar() {
  return (
    <nav
      aria-label="Main"
      class="glass fixed inset-x-4 bottom-[max(16px,env(safe-area-inset-bottom))] z-10 mx-auto flex max-w-md justify-between rounded-full p-[7px]"
    >
      <For each={tabs}>
        {(tab) => {
          const active = () => pathname() === tab.href;
          return (
            <Link
              href={tab.href}
              aria-label={tab.label}
              aria-current={active() ? "page" : undefined}
              class={`press flex size-[52px] items-center justify-center rounded-full ${
                active()
                  ? "bg-beau/14 text-beau dark:bg-beau-glow/22 dark:text-beau-glow"
                  : "text-ink dark:text-ink-inverse/85"
              }`}
            >
              <tab.icon />
            </Link>
          );
        }}
      </For>
    </nav>
  );
}
