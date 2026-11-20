import { render } from "solid-js/web";
import { App } from "./App";
import { isNative, platform } from "./platform";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing #root element in index.html");

render(() => <App />, root);

// The offline startup target (under 100ms) is measured from performance.timeOrigin
// to this mark. The DOM is in place once render() returns.
performance.mark("shell:rendered");

// In the native apps, keep a log of launch times (see src/startup.ts).
if (isNative()) {
  const name = platform();
  void import("./startup").then(({ recordStartup, clearStartups }) => {
    recordStartup(name);
    // Type ghClearStartups() in the inspector console before a new round.
    (globalThis as { ghClearStartups?: () => void }).ghClearStartups =
      clearStartups;
  });
}
