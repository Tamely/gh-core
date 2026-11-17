import { Show } from "solid-js";
import { REGISTRATION_OPEN } from "./content";
import { Districts } from "./Districts";
import { Hero } from "./Hero";
import { Faq, Footer, Sponsor } from "./Info";
import { Sides } from "./Sides";
import { Story } from "./Story";

/**
 * Before registration opens the site is a single screen, like HackMIT's
 * between events: the hero with a "notify me" field and nothing to scroll to.
 * The full page appears once REGISTRATION_OPEN is flipped in content.ts.
 */
export function App() {
  return (
    <>
      <Hero />
      <Show when={REGISTRATION_OPEN}>
        <main>
          <Story />
          <Sides />
          <Districts />
          <Faq />
          <Sponsor />
        </main>
        <Footer />
      </Show>
    </>
  );
}
