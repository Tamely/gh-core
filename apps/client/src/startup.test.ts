import { afterEach, describe, expect, it, vi } from "vitest";
import { clearStartups, median, recordStartup } from "./startup";

afterEach(() => {
  localStorage.clear();
  performance.clearMarks();
  vi.restoreAllMocks();
});

describe("median", () => {
  it("handles odd and even counts", () => {
    expect(median([90, 40, 70])).toBe(70);
    expect(median([40, 60, 80, 100])).toBe(70);
  });
});

describe("recordStartup", () => {
  it("keeps the last 20 launches and logs the median", () => {
    const log = vi.spyOn(console, "info").mockImplementation(() => {});
    localStorage.setItem(
      "gh:startup:v1",
      JSON.stringify(Array.from({ length: 20 }, (_, i) => i)),
    );
    performance.mark("shell:rendered");
    recordStartup("ios");

    const runs = JSON.parse(localStorage.getItem("gh:startup:v1") ?? "[]");
    expect(runs).toHaveLength(20);
    expect(runs[0]).toBe(1);
    expect(log.mock.calls[0]?.[0]).toMatch(
      /^\[startup\] ios: shell rendered in \d+ms/,
    );

    clearStartups();
    expect(localStorage.getItem("gh:startup:v1")).toBeNull();
  });
});
