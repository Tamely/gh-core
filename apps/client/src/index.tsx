import { render } from "solid-js/web";
import { App } from "./App";
import "./styles.css";

const root = document.getElementById("root");
if (!root) throw new Error("Missing #root element in index.html");

render(() => <App />, root);

// The offline startup target (under 100ms) is measured from performance.timeOrigin
// to this mark. The DOM is in place once render() returns.
performance.mark("shell:rendered");
