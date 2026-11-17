import { Districts } from "./Districts";
import { Hero } from "./Hero";
import { Faq, Footer, Sponsor, Timeline } from "./Info";
import { Sides } from "./Sides";
import { Story } from "./Story";

export function App() {
  return (
    <>
      <Hero />
      <main>
        <Story />
        <Sides />
        <Districts />
        <Timeline />
        <Faq />
        <Sponsor />
      </main>
      <Footer />
    </>
  );
}
