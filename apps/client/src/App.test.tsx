import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "./App";
import { navigate } from "./router";
import { writeSnapshot } from "./storage/snapshot";

afterEach(() => {
  cleanup();
  navigate("/", { replace: true });
  localStorage.clear();
});

describe("App", () => {
  it("opens on Home with the Home tab marked current", () => {
    render(() => <App />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Hey, hacker",
    );
    expect(
      screen.getByRole("link", { name: "Home" }).getAttribute("aria-current"),
    ).toBe("page");
  });

  it("greets the user from the saved snapshot", () => {
    writeSnapshot({ firstName: "Ana" });
    render(() => <App />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Hey, Ana",
    );
  });

  it("switches pages from the tab bar without a reload", () => {
    render(() => <App />);
    fireEvent.click(screen.getByRole("link", { name: "Schedule" }));
    expect(location.pathname).toBe("/schedule");
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Schedule",
    );
    expect(
      screen
        .getByRole("link", { name: "Schedule" })
        .getAttribute("aria-current"),
    ).toBe("page");
  });

  it("shows the fallback for unknown paths", () => {
    navigate("/nowhere");
    render(() => <App />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Wrong turn",
    );
  });
});
