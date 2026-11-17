import { describe, expect, it } from "vitest";
import { matchPath } from "./router";

describe("matchPath", () => {
  it("matches the root", () => {
    expect(matchPath("/", "/")).toEqual({});
  });

  it("matches static segments and ignores a trailing slash", () => {
    expect(matchPath("/schedule", "/schedule/")).toEqual({});
    expect(matchPath("/schedule", "/map")).toBeNull();
  });

  it("extracts and decodes params", () => {
    expect(matchPath("/judge/:token", "/judge/a%2Fb")).toEqual({
      token: "a/b",
    });
  });

  it("requires the same number of segments", () => {
    expect(matchPath("/judge/:token", "/judge")).toBeNull();
    expect(matchPath("/judge/:token", "/judge/a/b")).toBeNull();
  });
});
