import { beforeEach, describe, expect, it } from "vitest";
import { readSnapshot, writeSnapshot } from "./snapshot";

describe("snapshot", () => {
  beforeEach(() => localStorage.clear());

  it("returns null when nothing is saved", () => {
    expect(readSnapshot()).toBeNull();
  });

  it("round-trips what was written", () => {
    writeSnapshot({ firstName: "Ana", team: "beau" });
    expect(readSnapshot()).toMatchObject({
      v: 1,
      firstName: "Ana",
      team: "beau",
    });
  });

  it("ignores corrupt or old-version data", () => {
    localStorage.setItem("gh:snapshot:v1", "{not json");
    expect(readSnapshot()).toBeNull();
    localStorage.setItem("gh:snapshot:v1", JSON.stringify({ v: 0 }));
    expect(readSnapshot()).toBeNull();
  });
});
