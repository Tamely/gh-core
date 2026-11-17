import { cleanup, fireEvent, render, screen } from "@solidjs/testing-library";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { navigate } from "./router";
import { writeSnapshot } from "./storage/snapshot";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  navigate("/", { replace: true });
  localStorage.clear();
});

describe("Home timeline during the event", () => {
  it("folds earlier items behind a button and marks now", () => {
    // Saturday 7:45 PM: check-in, opening and lunch are over.
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2027-02-20T19:45:00-06:00"));
    render(() => <App />);

    expect(screen.getByLabelText("Now")).toBeTruthy();
    expect(screen.queryByText("Check-in")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show earlier (3)" }));
    expect(screen.getByText("Check-in")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Hide earlier" })).toBeTruthy();
  });
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
    fireEvent.click(screen.getByRole("link", { name: "Help" }));
    expect(location.pathname).toBe("/help");
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe("Help");
    expect(
      screen.getByRole("link", { name: "Help" }).getAttribute("aria-current"),
    ).toBe("page");
  });

  it("has no Schedule tab, because Home shows the schedule", () => {
    render(() => <App />);
    expect(screen.queryByRole("link", { name: "Schedule" })).toBeNull();
    expect(screen.getByRole("heading", { name: "Schedule" })).toBeTruthy();
  });

  it("shows the fallback for unknown paths", () => {
    navigate("/nowhere");
    render(() => <App />);
    expect(screen.getByRole("heading", { level: 1 }).textContent).toBe(
      "Wrong turn",
    );
  });
});
